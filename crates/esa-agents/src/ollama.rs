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
}

impl OllamaClient {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url,
            model,
            client: reqwest::Client::new(),
            token_counter: Arc::new(TokenCounter::new()),
            cost_tracker: Arc::new(AICostTracker::new()),
        }
    }

    pub fn with_cost_tracker(
        base_url: String,
        model: String,
        cost_tracker: Arc<AICostTracker>,
    ) -> Self {
        Self {
            base_url,
            model,
            client: reqwest::Client::new(),
            token_counter: Arc::new(TokenCounter::new()),
            cost_tracker,
        }
    }

    pub async fn generate(&self, prompt: String) -> Result<OllamaResponse> {
        self.generate_with_agent("unknown", prompt).await
    }

    pub async fn generate_with_agent(
        &self,
        agent_id: &str,
        prompt: String,
    ) -> Result<OllamaResponse> {
        let start_time = std::time::Instant::now();

        let request = OllamaRequest {
            model: self.model.clone(),
            prompt: prompt.clone(),
            stream: false,
            format: Some("json".to_string()),
            options: Some(OllamaOptions {
                temperature: 0.2,
                top_p: 0.9,
                num_predict: 512,
            }),
        };

        info!(
            "🤖 Ollama request to model: {} for agent: {}",
            self.model, agent_id
        );

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
                if !status.is_success() {
                    let error_text = response.text().await?;
                    warn!("Ollama error response: {}", error_text);

                    // Record failed request
                    self.cost_tracker.record_inference(
                        &self.model,
                        agent_id,
                        &prompt,
                        "",
                        latency_ms,
                        Some(format!("HTTP {}: {}", status, error_text)),
                    );

                    return Err(anyhow::anyhow!("Ollama request failed: {}", error_text));
                }

                let ollama_response: OllamaResponse = response.json().await?;

                // Estimate token usage
                let input_tokens = self.estimate_tokens(&prompt);
                let output_tokens = self.estimate_tokens(&ollama_response.response);

                // Update legacy token counter
                self.token_counter.add_request(input_tokens, output_tokens);

                // Record comprehensive metrics
                self.cost_tracker.record_inference(
                    &self.model,
                    agent_id,
                    &prompt,
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

                Ok(ollama_response)
            }
            Err(e) => {
                warn!("Ollama request failed: {}", e);

                // Record failed request
                self.cost_tracker.record_inference(
                    &self.model,
                    agent_id,
                    &prompt,
                    "",
                    latency_ms,
                    Some(e.to_string()),
                );

                Err(e.into())
            }
        }
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

    pub fn clear_cache(&self) -> usize {
        self.cost_tracker.clear_cache()
    }

    pub fn cache_size(&self) -> usize {
        self.cost_tracker.cache_size()
    }
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max_len).collect::<String>())
    }
}

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
    format: Option<String>,
    options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f32,
    top_p: f32,
    num_predict: usize,
}

#[derive(Debug, Deserialize)]
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
pub struct InferenceMetrics {
    pub request_id: String,
    pub model: String,
    pub agent_id: String,
    pub prompt: String,
    pub response: String,
    pub prompt_length: usize,
    pub response_length: usize,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub latency_ms: u64,
    pub cache_hit: bool,
    pub timestamp: DateTime<Utc>,
    pub error: Option<String>,
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
    pub cache_hit_rate: f64,
    pub requests_per_agent: HashMap<String, usize>,
    pub cost_per_agent: HashMap<String, f64>,
    pub time_window_start: DateTime<Utc>,
    pub time_window_end: DateTime<Utc>,
}

/// AI Cost Tracker - Comprehensive tracking of AI inference costs
pub struct AICostTracker {
    metrics: Arc<Mutex<Vec<InferenceMetrics>>>,
    cache: Arc<Mutex<HashMap<String, (String, DateTime<Utc>)>>>, // prompt_hash -> (response, timestamp)
}

impl AICostTracker {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(Vec::new())),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
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
        let request_id = Uuid::new_v4().to_string();
        let prompt_hash = self.hash_prompt(prompt);

        let cache_hit = if error.is_none() {
            let mut cache = self.cache.lock().unwrap();
            if let Some((cached_response, _)) = cache.get(&prompt_hash) {
                cached_response == response
            } else {
                cache.insert(prompt_hash.clone(), (response.to_string(), Utc::now()));
                false
            }
        } else {
            false
        };

        let input_tokens = self.estimate_tokens(prompt);
        let output_tokens = if error.is_none() {
            self.estimate_tokens(response)
        } else {
            0
        };

        let metric = InferenceMetrics {
            request_id: request_id.clone(),
            model: model.to_string(),
            agent_id: agent_id.to_string(),
            prompt: truncate_text(prompt, 2000),
            response: truncate_text(response, 2000),
            prompt_length: prompt.len(),
            response_length: response.len(),
            input_tokens,
            output_tokens,
            latency_ms,
            cache_hit,
            timestamp: Utc::now(),
            error,
        };

        let mut metrics = self.metrics.lock().unwrap();
        metrics.push(metric);

        // Keep only last 1000 metrics to prevent unbounded growth
        if metrics.len() > 1000 {
            metrics.drain(0..100);
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

        for metric in &filtered_metrics {
            *requests_per_agent
                .entry(metric.agent_id.clone())
                .or_insert(0) += 1;
            *cost_per_agent.entry(metric.agent_id.clone()).or_insert(0.0) +=
                metric.estimated_cost_usd();
        }

        AggregatedCostMetrics {
            total_requests,
            successful_requests,
            failed_requests,
            total_tokens,
            total_cost_usd,
            average_latency_ms,
            cache_hit_rate,
            requests_per_agent,
            cost_per_agent,
            time_window_start: window_start,
            time_window_end: now,
        }
    }

    pub fn get_recent_metrics(&self, limit: usize) -> Vec<InferenceMetrics> {
        let metrics = self.metrics.lock().unwrap();
        metrics.iter().rev().take(limit).cloned().collect()
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
            model: "llama2".to_string(),
            agent_id: "monitor".to_string(),
            prompt: "test prompt".to_string(),
            response: "test response".to_string(),
            prompt_length: 100,
            response_length: 50,
            input_tokens: 25,
            output_tokens: 12,
            latency_ms: 150,
            cache_hit: false,
            timestamp: Utc::now(),
            error: None,
        };

        assert_eq!(metric.total_tokens(), 37);
        assert!(metric.estimated_cost_usd() > 0.0);
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
