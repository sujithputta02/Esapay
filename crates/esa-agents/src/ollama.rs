use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};
use uuid::Uuid;

/// Ollama client with comprehensive cost tracking and caching

#[derive(Clone)]
pub struct OllamaClient {
    base_url: String,
    model: String,
    client: reqwest::Client,
    token_counter: Arc<TokenCounter>,
    cost_tracker: Arc<AICostTracker>,
    concurrency_semaphore: Arc<tokio::sync::Semaphore>,
    consecutive_failures: Arc<AtomicUsize>,
    circuit_open_until: Arc<Mutex<Option<std::time::Instant>>>,
    response_cache: Arc<Mutex<HashMap<String, (OllamaResponse, std::time::Instant)>>>,
}

impl OllamaClient {
    pub fn new(base_url: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(25))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            base_url,
            model,
            client,
            token_counter: Arc::new(TokenCounter::new()),
            cost_tracker: Arc::new(AICostTracker::new()),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            consecutive_failures: Arc::new(AtomicUsize::new(0)),
            circuit_open_until: Arc::new(Mutex::new(None)),
            response_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Build an Ollama client with system proxy discovery disabled (useful in sandboxed test environments).
    pub fn new_without_proxy(base_url: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(25))
            .build()
            .expect("failed to build sandbox-safe Ollama HTTP client");

        Self {
            base_url,
            model,
            client,
            token_counter: Arc::new(TokenCounter::new()),
            cost_tracker: Arc::new(AICostTracker::new()),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            consecutive_failures: Arc::new(AtomicUsize::new(0)),
            circuit_open_until: Arc::new(Mutex::new(None)),
            response_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_cost_tracker(
        base_url: String,
        model: String,
        cost_tracker: Arc<AICostTracker>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(25))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            base_url,
            model,
            client,
            token_counter: Arc::new(TokenCounter::new()),
            cost_tracker,
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            consecutive_failures: Arc::new(AtomicUsize::new(0)),
            circuit_open_until: Arc::new(Mutex::new(None)),
            response_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_cost_tracker_no_proxy(
        base_url: String,
        model: String,
        cost_tracker: Arc<AICostTracker>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(25))
            .build()
            .expect("failed to build sandbox-safe Ollama HTTP client");

        Self {
            base_url,
            model,
            client,
            token_counter: Arc::new(TokenCounter::new()),
            cost_tracker,
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            consecutive_failures: Arc::new(AtomicUsize::new(0)),
            circuit_open_until: Arc::new(Mutex::new(None)),
            response_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn token_counter(&self) -> Arc<TokenCounter> {
        Arc::clone(&self.token_counter)
    }

    pub async fn generate(&self, prompt: String) -> Result<OllamaResponse> {
        self.generate_with_agent("unknown", prompt).await
    }

    pub async fn generate_with_agent(
        &self,
        agent_id: &str,
        prompt: String,
    ) -> Result<OllamaResponse> {
        self.generate_with_context(agent_id, None, prompt).await
    }

    pub async fn generate_with_context(
        &self,
        agent_id: &str,
        merchant_id: Option<&str>,
        prompt: String,
    ) -> Result<OllamaResponse> {
        // Scrub sensitive data before caching, prompting, or logging.
        let sanitized_prompt = crate::privacy::sanitize_prompt(&prompt);

        // 1. Semantic / Deterministic Response Cache Check (Rule 10 & 20)
        let tenant_key = merchant_id.unwrap_or("__global__");
        let cache_key = format!(
            "{}:{}:{}:{}",
            tenant_key,
            self.model,
            agent_id,
            sanitized_prompt.trim()
        );
        if let Ok(cache) = self.response_cache.lock() {
            if let Some((cached_res, timestamp)) = cache.get(&cache_key) {
                if timestamp.elapsed() < std::time::Duration::from_secs(60) {
                    info!(
                        "⚡ AI Cache HIT for agent '{}' (saved upstream inference)",
                        agent_id
                    );
                    return Ok(cached_res.clone());
                }
            }
        }

        // 2. Circuit Breaker Check (Rule 17)
        if let Ok(guard) = self.circuit_open_until.lock() {
            if let Some(open_until) = *guard {
                if std::time::Instant::now() < open_until {
                    return Err(anyhow::anyhow!(
                        "Ollama circuit breaker is OPEN due to repeated upstream failures. Fast-failing to deterministic rules."
                    ));
                }
            }
        }

        // 3. Concurrency Limiter (Rule 12)
        let _permit = self
            .concurrency_semaphore
            .acquire()
            .await
            .map_err(|e| anyhow::anyhow!("Ollama concurrency semaphore error: {}", e))?;

        let start_time = std::time::Instant::now();

        let request = OllamaRequest {
            model: self.model.clone(),
            prompt: sanitized_prompt.clone(),
            stream: false,
            format: Some("json".to_string()),
            options: Some(OllamaOptions {
                temperature: 0.1,
                top_p: 0.85,
                num_predict: 128,
            }),
            keep_alive: Some(
                std::env::var("OLLAMA_KEEP_ALIVE")
                    .map(|v| if v == "-1" { "24h".to_string() } else { v })
                    .unwrap_or_else(|_| "24h".to_string()),
            ),
        };

        info!(
            "🤖 Ollama fast-inference request to model: {} for agent: {}",
            self.model, agent_id
        );

        let mut attempts = 0;
        let max_attempts = 2;
        let mut last_error_text = String::new();

        while attempts < max_attempts {
            attempts += 1;
            let response_result = self
                .client
                .post(format!("{}/api/generate", self.base_url))
                .json(&request)
                .send()
                .await;

            let latency_ms = start_time.elapsed().as_millis() as u64;

            match response_result {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        // Reset circuit breaker upon successful response
                        self.consecutive_failures.store(0, Ordering::SeqCst);
                        if let Ok(mut guard) = self.circuit_open_until.lock() {
                            *guard = None;
                        }

                        let ollama_response: OllamaResponse = response.json().await?;

                        // Save to response cache (Rule 10 & 20)
                        if let Ok(mut cache) = self.response_cache.lock() {
                            if cache.len() > 500 {
                                cache.clear();
                            }
                            cache.insert(
                                cache_key,
                                (ollama_response.clone(), std::time::Instant::now()),
                            );
                        }

                        // Estimate token usage
                        let input_tokens = self.estimate_tokens(&sanitized_prompt);
                        let output_tokens = self.estimate_tokens(&ollama_response.response);

                        // Update legacy token counter
                        self.token_counter.add_request(input_tokens, output_tokens);

                        // Record comprehensive redacted, tenant-scoped metrics
                        self.cost_tracker.record_inference_with_scope(
                            &self.model,
                            agent_id,
                            merchant_id,
                            &sanitized_prompt,
                            &ollama_response.response,
                            latency_ms,
                            None,
                        );

                        info!(
                            "✅ Ollama response received. Tokens: {}+{}={}, Latency: {}ms",
                            input_tokens,
                            output_tokens,
                            input_tokens + output_tokens,
                            latency_ms
                        );

                        return Ok(ollama_response);
                    } else if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                        && attempts < max_attempts
                    {
                        // Upstream 429: Exponential backoff with random jitter (Rule 8 & 16)
                        let jitter = (chrono::Utc::now().timestamp_subsec_millis() % 200) as u64;
                        let delay =
                            std::time::Duration::from_millis(300 * attempts as u64 + jitter);
                        warn!(
                            "⚠️ Upstream 429 rate limit. Backing off for {}ms before retry...",
                            delay.as_millis()
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    } else {
                        let raw_err = response
                            .text()
                            .await
                            .unwrap_or_else(|_| format!("HTTP {}", status));
                        last_error_text = crate::privacy::redact_sensitive_text(&raw_err).redacted;
                        warn!("Ollama error response: {}", last_error_text);
                        break;
                    }
                }
                Err(e) => {
                    last_error_text =
                        crate::privacy::redact_sensitive_text(&e.to_string()).redacted;
                    warn!(
                        "Ollama request attempt {} failed: {}",
                        attempts, last_error_text
                    );
                    if attempts < max_attempts {
                        let delay = std::time::Duration::from_millis(250);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    break;
                }
            }
        }

        // Trip circuit breaker on repeated failures (Rule 17)
        let fails = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        if fails >= 5 {
            if let Ok(mut guard) = self.circuit_open_until.lock() {
                *guard = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
                warn!(
                    "⚠️ Ollama circuit breaker TRIPPED for 30s after {} consecutive failures",
                    fails
                );
            }
        }

        let latency_ms = start_time.elapsed().as_millis() as u64;
        self.cost_tracker.record_inference_with_scope(
            &self.model,
            agent_id,
            merchant_id,
            &sanitized_prompt,
            "",
            latency_ms,
            Some(last_error_text.clone()),
        );

        Err(anyhow::anyhow!(
            "Ollama upstream failure: {}",
            last_error_text
        ))
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        // Rough estimation: ~4 characters per token
        (text.len() / 4).max(1)
    }

    pub fn get_token_stats(&self) -> TokenStats {
        self.token_counter.get_stats()
    }

    pub fn get_cost_tracker(&self) -> Arc<AICostTracker> {
        self.cost_tracker.clone()
    }

    pub fn get_aggregated_cost_metrics(
        &self,
        time_window_hours: Option<i64>,
    ) -> AggregatedCostMetrics {
        self.cost_tracker.get_aggregated_metrics(time_window_hours)
    }

    pub fn get_recent_inference_metrics(&self, limit: usize) -> Vec<InferenceMetrics> {
        self.cost_tracker.get_recent_metrics(limit)
    }

    pub fn get_recent_inference_metrics_for_merchant(
        &self,
        merchant_id: Option<&str>,
        limit: usize,
    ) -> Vec<InferenceMetrics> {
        self.cost_tracker
            .get_recent_metrics_for_merchant(merchant_id, limit)
    }

    pub fn clear_cache(&self) -> usize {
        self.cost_tracker.clear_cache()
    }

    pub fn cache_size(&self) -> usize {
        self.cost_tracker.cache_size()
    }
}

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
    /// Keep model in memory indefinitely (24/7) to avoid cold-boot latency
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_alive: Option<String>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f32,
    top_p: f32,
    num_predict: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaResponse {
    pub model: String,
    pub response: String,
    pub done: bool,
}

pub struct TokenCounter {
    total_requests: AtomicUsize,
    total_input_tokens: AtomicUsize,
    total_output_tokens: AtomicUsize,
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenCounter {
    pub fn new() -> Self {
        Self {
            total_requests: AtomicUsize::new(0),
            total_input_tokens: AtomicUsize::new(0),
            total_output_tokens: AtomicUsize::new(0),
        }
    }

    pub fn add_request(&self, input_tokens: usize, output_tokens: usize) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_input_tokens
            .fetch_add(input_tokens, Ordering::Relaxed);
        self.total_output_tokens
            .fetch_add(output_tokens, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> TokenStats {
        TokenStats {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_input_tokens: self.total_input_tokens.load(Ordering::Relaxed),
            total_output_tokens: self.total_output_tokens.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStats {
    pub total_requests: usize,
    pub total_input_tokens: usize,
    pub total_output_tokens: usize,
}

impl TokenStats {
    pub fn total_tokens(&self) -> usize {
        self.total_input_tokens + self.total_output_tokens
    }

    pub fn average_tokens_per_request(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.total_tokens() as f64 / self.total_requests as f64
        }
    }

    pub fn estimated_cost_usd(&self) -> f64 {
        // Estimated cost per token for local models (very low)
        // For cloud models, this would use actual pricing
        self.total_tokens() as f64 * 0.00001 // $0.01 per 1000 tokens
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryRetentionConfig {
    pub max_records: usize,
    pub retain_redacted_summary: bool,
    pub max_summary_chars: usize,
    pub ttl_seconds: Option<u64>,
}

impl Default for TelemetryRetentionConfig {
    fn default() -> Self {
        Self {
            max_records: 1000,
            retain_redacted_summary: true,
            max_summary_chars: 512,
            ttl_seconds: None,
        }
    }
}

impl TelemetryRetentionConfig {
    pub fn from_env() -> Self {
        let max_records = std::env::var("ESA_AI_TELEMETRY_MAX_RECORDS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .map(|n| n.clamp(1, 10_000))
            .unwrap_or(1000);
        let retain_redacted_summary = std::env::var("ESA_AI_TELEMETRY_RETAIN_SUMMARY")
            .ok()
            .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "false" | "0" | "no"))
            .unwrap_or(true);
        let max_summary_chars = std::env::var("ESA_AI_TELEMETRY_MAX_SUMMARY_CHARS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .map(|n| n.clamp(64, 1024))
            .unwrap_or(512);
        let ttl_seconds = std::env::var("ESA_AI_TELEMETRY_TTL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&s| s > 0);
        Self {
            max_records,
            retain_redacted_summary,
            max_summary_chars,
            ttl_seconds,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceMetrics {
    pub request_id: String,
    #[serde(default = "default_provider_name")]
    pub provider: String,
    #[serde(default = "default_execution_scope")]
    pub execution_scope: String,
    pub merchant_id: Option<String>,
    pub model: String,
    pub agent_id: String,
    pub prompt_hash: String,
    pub redacted_prompt_summary: Option<String>,
    pub redacted_response_summary: Option<String>,
    pub evidence_refs: Vec<String>,
    pub redaction_applied: bool,
    pub prompt_length: usize,
    pub response_length: usize,
    pub input_tokens: usize,
    pub output_tokens: usize,
    #[serde(default)]
    pub queue_wait_ms: Option<u64>,
    #[serde(default)]
    pub gpu_execution_ms: Option<u64>,
    pub latency_ms: u64,
    pub cache_hit: bool,
    pub timestamp: DateTime<Utc>,
    pub error_category: Option<String>,
    pub error: Option<String>,
}

fn default_provider_name() -> String {
    "ollama".to_string()
}

fn default_execution_scope() -> String {
    "system".to_string()
}

impl InferenceMetrics {
    pub fn total_tokens(&self) -> usize {
        self.input_tokens + self.output_tokens
    }

    pub fn estimated_cost_usd(&self) -> f64 {
        self.total_tokens() as f64 * 0.00001
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedCostMetrics {
    pub total_requests: usize,
    pub successful_requests: usize,
    pub failed_requests: usize,
    pub total_tokens: usize,
    pub total_cost_usd: f64,
    pub average_latency_ms: f64,
    pub avg_queue_wait_ms: Option<f64>,
    pub avg_gpu_execution_ms: Option<f64>,
    pub cache_hit_rate: f64,
    pub requests_per_agent: HashMap<String, usize>,
    pub cost_per_agent: HashMap<String, f64>,
    pub requests_per_provider: HashMap<String, usize>,
    pub cost_per_provider: HashMap<String, f64>,
    pub time_window_start: DateTime<Utc>,
    pub time_window_end: DateTime<Utc>,
}

/// AI Cost Tracker - Comprehensive tracking of AI inference costs with privacy redaction
pub struct AICostTracker {
    metrics: Arc<Mutex<Vec<InferenceMetrics>>>,
    #[allow(clippy::type_complexity)]
    cache: Arc<Mutex<HashMap<String, (String, DateTime<Utc>)>>>, // (scope:prompt_hash) -> (redacted_response, timestamp)
    retention: TelemetryRetentionConfig,
}

impl AICostTracker {
    pub fn new() -> Self {
        Self::with_retention(TelemetryRetentionConfig::from_env())
    }

    pub fn with_retention(retention: TelemetryRetentionConfig) -> Self {
        Self {
            metrics: Arc::new(Mutex::new(Vec::new())),
            cache: Arc::new(Mutex::new(HashMap::new())),
            retention,
        }
    }

    pub fn retention_config(&self) -> &TelemetryRetentionConfig {
        &self.retention
    }

    pub fn record_inference(
        &self,
        model: &str,
        agent_id: &str,
        prompt: &str,
        response: &str,
        latency_ms: u64,
        error: Option<String>,
    ) -> String {
        self.record_inference_with_scope(model, agent_id, None, prompt, response, latency_ms, error)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_inference_with_scope(
        &self,
        model: &str,
        agent_id: &str,
        merchant_id: Option<&str>,
        prompt: &str,
        response: &str,
        latency_ms: u64,
        error: Option<String>,
    ) -> String {
        let scope = if merchant_id.is_some() {
            "merchant_scoped"
        } else {
            "system"
        };
        self.record_provider_inference(
            "ollama",
            scope,
            model,
            agent_id,
            merchant_id,
            prompt,
            response,
            latency_ms,
            None,
            None,
            None,
            error,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_provider_inference(
        &self,
        provider: &str,
        execution_scope: &str,
        model: &str,
        agent_id: &str,
        merchant_id: Option<&str>,
        prompt: &str,
        response: &str,
        latency_ms: u64,
        queue_wait_ms: Option<u64>,
        gpu_execution_ms: Option<u64>,
        cache_hit_override: Option<bool>,
        error: Option<String>,
    ) -> String {
        let request_id = Uuid::new_v4().to_string();
        let prompt_redaction = crate::privacy::redact_sensitive_text(prompt);
        let stripped_response = crate::privacy::strip_chain_of_thought(response);
        let response_redaction = crate::privacy::redact_sensitive_text(&stripped_response);
        let redaction_applied =
            prompt_redaction.redaction_count > 0 || response_redaction.redaction_count > 0;

        let prompt_hash = self.hash_prompt(&prompt_redaction.redacted);
        let cache_scope_key = format!(
            "{}:{}:{}",
            provider,
            merchant_id.unwrap_or("__global__"),
            prompt_hash
        );

        let cache_hit = match cache_hit_override {
            Some(v) => v,
            None => {
                if error.is_none() {
                    let mut cache = self.cache.lock().unwrap();
                    if let Some((cached_response, _)) = cache.get(&cache_scope_key) {
                        cached_response == &response_redaction.redacted
                    } else {
                        cache.insert(
                            cache_scope_key,
                            (response_redaction.redacted.clone(), Utc::now()),
                        );
                        false
                    }
                } else {
                    false
                }
            }
        };

        let input_tokens = self.estimate_tokens(&prompt_redaction.redacted);
        let output_tokens = if error.is_none() && !response_redaction.redacted.is_empty() {
            self.estimate_tokens(&response_redaction.redacted)
        } else {
            0
        };

        let redacted_prompt_summary = if self.retention.retain_redacted_summary {
            Some(crate::privacy::truncate_utf8(
                &prompt_redaction.redacted,
                self.retention.max_summary_chars,
            ))
        } else {
            None
        };
        let redacted_response_summary = if self.retention.retain_redacted_summary {
            Some(crate::privacy::truncate_utf8(
                &response_redaction.redacted,
                self.retention.max_summary_chars,
            ))
        } else {
            None
        };

        let evidence_refs = extract_safe_evidence_refs(&response_redaction.redacted);

        let (error_category, sanitized_error) = match error {
            Some(raw_err) => {
                let cat = crate::privacy::categorize_error(&raw_err);
                let scrubbed = crate::privacy::redact_sensitive_text(&raw_err).redacted;
                let bounded = crate::privacy::truncate_utf8(&scrubbed, 256);
                (Some(cat), Some(bounded))
            }
            None => (None, None),
        };

        let metric = InferenceMetrics {
            request_id: request_id.clone(),
            provider: provider.to_string(),
            execution_scope: execution_scope.to_string(),
            merchant_id: merchant_id.map(|s| s.to_string()),
            model: model.to_string(),
            agent_id: agent_id.to_string(),
            prompt_hash,
            redacted_prompt_summary,
            redacted_response_summary,
            evidence_refs,
            redaction_applied,
            prompt_length: prompt.len(),
            response_length: response.len(),
            input_tokens,
            output_tokens,
            queue_wait_ms,
            gpu_execution_ms,
            latency_ms,
            cache_hit,
            timestamp: Utc::now(),
            error_category,
            error: sanitized_error,
        };

        let mut metrics = self.metrics.lock().unwrap();
        let now = Utc::now();
        if let Some(ttl_secs) = self.retention.ttl_seconds {
            let cutoff = now - chrono::Duration::seconds(ttl_secs as i64);
            metrics.retain(|m| m.timestamp >= cutoff);
        }
        metrics.push(metric);

        if metrics.len() > self.retention.max_records {
            let excess = metrics.len() - self.retention.max_records;
            metrics.drain(0..excess);
        }

        request_id
    }

    pub fn get_aggregated_metrics(&self, time_window_hours: Option<i64>) -> AggregatedCostMetrics {
        let metrics = self.metrics.lock().unwrap();
        let now = Utc::now();
        let window_start = time_window_hours
            .map(|hours| now - chrono::Duration::hours(hours))
            .unwrap_or_else(|| metrics.first().map(|m| m.timestamp).unwrap_or(now));

        let filtered_metrics: Vec<&InferenceMetrics> = metrics
            .iter()
            .filter(|m| m.timestamp >= window_start)
            .collect();

        let total_requests = filtered_metrics.len();
        let successful_requests = filtered_metrics
            .iter()
            .filter(|m| m.error.is_none())
            .count();
        let failed_requests = total_requests - successful_requests;

        let total_tokens: usize = filtered_metrics.iter().map(|m| m.total_tokens()).sum();
        let total_cost_usd: f64 = filtered_metrics
            .iter()
            .map(|m| m.estimated_cost_usd())
            .sum();

        let average_latency_ms = if total_requests > 0 {
            filtered_metrics
                .iter()
                .map(|m| m.latency_ms as f64)
                .sum::<f64>()
                / total_requests as f64
        } else {
            0.0
        };

        let cache_hits = filtered_metrics.iter().filter(|m| m.cache_hit).count();
        let cache_hit_rate = if total_requests > 0 {
            cache_hits as f64 / total_requests as f64
        } else {
            0.0
        };

        let mut requests_per_agent = HashMap::new();
        let mut cost_per_agent = HashMap::new();
        let mut requests_per_provider = HashMap::new();
        let mut cost_per_provider = HashMap::new();

        for metric in &filtered_metrics {
            *requests_per_agent
                .entry(metric.agent_id.clone())
                .or_insert(0) += 1;
            *cost_per_agent.entry(metric.agent_id.clone()).or_insert(0.0) +=
                metric.estimated_cost_usd();
            *requests_per_provider
                .entry(metric.provider.clone())
                .or_insert(0) += 1;
            *cost_per_provider
                .entry(metric.provider.clone())
                .or_insert(0.0) += metric.estimated_cost_usd();
        }

        let queue_waits: Vec<u64> = filtered_metrics
            .iter()
            .filter_map(|m| m.queue_wait_ms)
            .collect();
        let avg_queue_wait_ms = if queue_waits.is_empty() {
            None
        } else {
            Some(queue_waits.iter().sum::<u64>() as f64 / queue_waits.len() as f64)
        };

        let gpu_execs: Vec<u64> = filtered_metrics
            .iter()
            .filter_map(|m| m.gpu_execution_ms)
            .collect();
        let avg_gpu_execution_ms = if gpu_execs.is_empty() {
            None
        } else {
            Some(gpu_execs.iter().sum::<u64>() as f64 / gpu_execs.len() as f64)
        };

        AggregatedCostMetrics {
            total_requests,
            successful_requests,
            failed_requests,
            total_tokens,
            total_cost_usd,
            average_latency_ms,
            avg_queue_wait_ms,
            avg_gpu_execution_ms,
            cache_hit_rate,
            requests_per_agent,
            cost_per_agent,
            requests_per_provider,
            cost_per_provider,
            time_window_start: window_start,
            time_window_end: now,
        }
    }

    pub fn get_recent_metrics(&self, limit: usize) -> Vec<InferenceMetrics> {
        self.get_recent_metrics_for_merchant(None, limit)
    }

    pub fn get_recent_metrics_for_merchant(
        &self,
        merchant_id: Option<&str>,
        limit: usize,
    ) -> Vec<InferenceMetrics> {
        let mut metrics = self.metrics.lock().unwrap();
        if let Some(ttl_secs) = self.retention.ttl_seconds {
            let cutoff = Utc::now() - chrono::Duration::seconds(ttl_secs as i64);
            metrics.retain(|m| m.timestamp >= cutoff);
        }
        metrics
            .iter()
            .rev()
            .filter(|m| match merchant_id {
                Some(mid) => m.merchant_id.as_deref() == Some(mid),
                None => true,
            })
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn clear_cache(&self) -> usize {
        let mut cache = self.cache.lock().unwrap();
        let count = cache.len();
        cache.clear();
        count
    }

    pub fn cache_size(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    fn hash_prompt(&self, prompt: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        prompt.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        // Rough estimation: ~4 characters per token
        (text.len() / 4).max(1)
    }
}

impl Default for AICostTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_safe_evidence_refs(redacted_response: &str) -> Vec<String> {
    let trimmed = redacted_response.trim();
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&trimmed[start..=end]) {
            if let Some(arr) = val.get("evidence_refs").and_then(|v| v.as_array()) {
                return arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| crate::privacy::truncate_utf8(s.trim(), 128))
                    .filter(|s| !s.is_empty())
                    .take(16)
                    .collect();
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_stats() {
        let counter = TokenCounter::new();
        counter.add_request(100, 50);
        counter.add_request(200, 75);

        let stats = counter.get_stats();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.total_input_tokens, 300);
        assert_eq!(stats.total_output_tokens, 125);
        assert_eq!(stats.total_tokens(), 425);
    }

    #[test]
    fn test_cost_tracker_basic() {
        let tracker = AICostTracker::new();

        tracker.record_inference(
            "llama2",
            "monitor",
            "test prompt",
            "test response",
            100,
            None,
        );

        let metrics = tracker.get_aggregated_metrics(None);
        assert_eq!(metrics.total_requests, 1);
        assert_eq!(metrics.successful_requests, 1);
        assert_eq!(metrics.failed_requests, 0);
        assert!(metrics.total_tokens > 0);
    }

    #[test]
    fn test_cost_tracker_cache() {
        let tracker = AICostTracker::new();

        // First request
        tracker.record_inference("llama2", "planning", "same prompt", "response", 100, None);

        // Second request with same prompt
        tracker.record_inference("llama2", "planning", "same prompt", "response", 50, None);

        let metrics = tracker.get_aggregated_metrics(None);
        assert_eq!(metrics.total_requests, 2);

        // Second request should be cache hit
        let recent = tracker.get_recent_metrics(2);
        assert!(recent.iter().any(|m| m.cache_hit));
    }

    #[test]
    fn test_cost_tracker_per_agent() {
        let tracker = AICostTracker::new();

        tracker.record_inference("llama2", "monitor", "test1", "resp1", 100, None);
        tracker.record_inference("llama2", "monitor", "test2", "resp2", 100, None);
        tracker.record_inference("llama2", "planning", "test3", "resp3", 100, None);

        let metrics = tracker.get_aggregated_metrics(None);
        assert_eq!(metrics.requests_per_agent.get("monitor"), Some(&2));
        assert_eq!(metrics.requests_per_agent.get("planning"), Some(&1));
        assert!(metrics.cost_per_agent.contains_key("monitor"));
        assert!(metrics.cost_per_agent.contains_key("planning"));
    }

    #[test]
    fn test_cost_tracker_failures() {
        let tracker = AICostTracker::new();

        tracker.record_inference(
            "llama2",
            "diagnosis",
            "test",
            "",
            100,
            Some("Connection timeout".to_string()),
        );

        let metrics = tracker.get_aggregated_metrics(None);
        assert_eq!(metrics.total_requests, 1);
        assert_eq!(metrics.successful_requests, 0);
        assert_eq!(metrics.failed_requests, 1);
    }

    #[test]
    fn test_cache_operations() {
        let tracker = AICostTracker::new();

        tracker.record_inference("llama2", "safety", "prompt1", "resp1", 100, None);
        tracker.record_inference("llama2", "safety", "prompt2", "resp2", 100, None);

        assert_eq!(tracker.cache_size(), 2);

        let cleared = tracker.clear_cache();
        assert_eq!(cleared, 2);
        assert_eq!(tracker.cache_size(), 0);
    }

    #[test]
    fn test_inference_metrics_cost() {
        let metric = InferenceMetrics {
            request_id: "test_123".to_string(),
            provider: "ollama".to_string(),
            execution_scope: "merchant_scoped".to_string(),
            merchant_id: Some("merchant_01".to_string()),
            model: "llama2".to_string(),
            agent_id: "monitor".to_string(),
            prompt_hash: "abc123".to_string(),
            redacted_prompt_summary: Some("test prompt".to_string()),
            redacted_response_summary: Some("test response".to_string()),
            evidence_refs: vec![],
            redaction_applied: false,
            prompt_length: 100,
            response_length: 50,
            input_tokens: 25,
            output_tokens: 12,
            queue_wait_ms: None,
            gpu_execution_ms: None,
            latency_ms: 150,
            cache_hit: false,
            timestamp: Utc::now(),
            error_category: None,
            error: None,
        };

        assert_eq!(metric.total_tokens(), 37);
        assert!(metric.estimated_cost_usd() > 0.0);
    }

    #[test]
    fn test_cost_tracker_redacts_secrets_strips_cot_and_scopes_merchants() {
        let tracker = AICostTracker::new();
        tracker.record_inference_with_scope(
            "llama2",
            "diagnosis",
            Some("merchant_alpha"),
            r#"{"api_key":"esa_live_secret_99999","email":"bob@example.com","p95_latency_ms":250}"#,
            r#"<think>secret chain of thought</think>{"hypothesis":"Latency spike","evidence_refs":["p95_latency_ms"]}"#,
            120,
            None,
        );
        tracker.record_inference_with_scope(
            "llama2",
            "diagnosis",
            Some("merchant_beta"),
            "beta prompt",
            "beta response",
            80,
            None,
        );

        let alpha_only = tracker.get_recent_metrics_for_merchant(Some("merchant_alpha"), 10);
        assert_eq!(alpha_only.len(), 1);
        let p_sum = alpha_only[0].redacted_prompt_summary.as_deref().unwrap();
        let r_sum = alpha_only[0].redacted_response_summary.as_deref().unwrap();
        assert!(!p_sum.contains("esa_live_secret_99999"));
        assert!(!p_sum.contains("bob@example.com"));
        assert!(!r_sum.contains("secret chain of thought"));
        assert!(alpha_only[0].redaction_applied);
        assert_eq!(
            alpha_only[0].evidence_refs,
            vec!["p95_latency_ms".to_string()]
        );
    }

    #[test]
    fn test_cost_tracker_retention_config_omits_summary_and_bounds_records() {
        let tracker = AICostTracker::with_retention(TelemetryRetentionConfig {
            max_records: 2,
            retain_redacted_summary: false,
            max_summary_chars: 128,
            ttl_seconds: None,
        });
        for i in 0..4 {
            tracker.record_inference(
                "llama2",
                "monitor",
                &format!("p{}", i),
                &format!("r{}", i),
                10,
                None,
            );
        }
        let recent = tracker.get_recent_metrics(10);
        assert_eq!(recent.len(), 2);
        assert!(recent[0].redacted_prompt_summary.is_none());
        assert!(recent[0].redacted_response_summary.is_none());
    }

    #[test]
    fn test_aggregated_metrics_calculations() {
        let tracker = AICostTracker::new();

        // Add multiple requests with varying latencies
        for i in 0..5 {
            tracker.record_inference(
                "llama2",
                "monitor",
                &format!("prompt_{}", i),
                &format!("response_{}", i),
                100 + (i * 20),
                None,
            );
        }

        let metrics = tracker.get_aggregated_metrics(None);
        assert_eq!(metrics.total_requests, 5);
        assert!(metrics.average_latency_ms > 0.0);
        assert!(metrics.average_latency_ms >= 100.0);
        assert!(metrics.total_cost_usd > 0.0);
    }
}
