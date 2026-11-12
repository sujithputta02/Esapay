use crate::ollama::{AICostTracker, TokenStats};
use crate::privacy;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use uuid::Uuid;

/// Authoritative Hugging Face model identity pinned for Phase 2 / 2.1.
pub const HF_AUTHORITATIVE_MODEL_ID: &str = "Qwen/Qwen2.5-0.5B-Instruct";
pub const HF_AUTHORITATIVE_MODEL_REVISION: &str = "7ae557604adf67be50417f59c2c2f167def9a775";

/// Identifies which inference provider handled or attempted a request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Ollama,
    HfZerogpu,
    DeterministicFallback,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Ollama => "ollama",
            ProviderKind::HfZerogpu => "hf_zerogpu",
            ProviderKind::DeterministicFallback => "deterministic_fallback",
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Explicit tenant/system scope for an AI inference request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionScope {
    System,
    MerchantScoped { merchant_id: String },
}

impl ExecutionScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionScope::System => "system",
            ExecutionScope::MerchantScoped { .. } => "merchant_scoped",
        }
    }

    /// Validates the scope and returns the normalized merchant ID (`None` for `System`).
    /// Never converts an invalid `MerchantScoped` into `System`.
    pub fn normalized_merchant_id(&self) -> Result<Option<&str>, InferenceError> {
        match self {
            ExecutionScope::System => Ok(None),
            ExecutionScope::MerchantScoped { merchant_id } => {
                let trimmed = merchant_id.trim();
                if trimmed.is_empty() {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidScope,
                        "MerchantScoped execution scope requires a non-empty merchant_id",
                    ));
                }
                if trimmed
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || c == ':')
                {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidScope,
                        "MerchantScoped merchant_id contains invalid whitespace, control, or delimiter characters",
                    ));
                }
                Ok(Some(trimmed))
            }
        }
    }

    /// Returns a strictly isolated cache key prefix. Fails closed on invalid `MerchantScoped`.
    pub fn cache_key_prefix(&self) -> Result<String, InferenceError> {
        match self.normalized_merchant_id()? {
            Some(mid) => Ok(format!("merchant:{}", mid)),
            None => Ok("system:__global__".to_string()),
        }
    }
}

/// Bounded generation limits applied to provider inference calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationLimits {
    pub max_prompt_chars: usize,
    pub max_new_tokens: usize,
    pub max_output_bytes: usize,
    pub temperature: f32,
    pub top_p: f32,
}

impl Default for GenerationLimits {
    fn default() -> Self {
        Self {
            max_prompt_chars: 8192,
            max_new_tokens: 256,
            max_output_bytes: 16 * 1024,
            temperature: 0.1,
            top_p: 0.85,
        }
    }
}

/// Provider-independent structured inference request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    pub request_id: String,
    pub agent_id: String,
    pub model_id: Option<String>,
    pub scope: ExecutionScope,
    pub prompt: String,
    pub limits: GenerationLimits,
}

impl InferenceRequest {
    pub fn new(
        agent_id: impl Into<String>,
        scope: ExecutionScope,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            model_id: None,
            scope,
            prompt: prompt.into(),
            limits: GenerationLimits::default(),
        }
    }

    pub fn with_limits(mut self, limits: GenerationLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }
}

/// Provider-independent normalized inference response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResponse {
    pub request_id: String,
    pub provider: ProviderKind,
    pub model_id: String,
    pub text: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub queue_wait_ms: Option<u64>,
    pub gpu_execution_ms: Option<u64>,
    pub total_latency_ms: u64,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InferenceErrorCategory {
    Unauthorized,
    Forbidden,
    NotFound,
    Timeout,
    RateLimited,
    QuotaExhausted,
    ServiceUnavailable,
    CircuitOpen,
    BadGateway,
    InvalidResponse,
    InvalidScope,
    PromptTooLarge,
    Cancelled,
    Misconfigured,
    ConnectionError,
    UpstreamError,
}

impl InferenceErrorCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            InferenceErrorCategory::Unauthorized => "unauthorized",
            InferenceErrorCategory::Forbidden => "forbidden",
            InferenceErrorCategory::NotFound => "not_found",
            InferenceErrorCategory::Timeout => "timeout",
            InferenceErrorCategory::RateLimited => "rate_limited",
            InferenceErrorCategory::QuotaExhausted => "quota_exhausted",
            InferenceErrorCategory::ServiceUnavailable => "service_unavailable",
            InferenceErrorCategory::CircuitOpen => "circuit_open",
            InferenceErrorCategory::BadGateway => "bad_gateway",
            InferenceErrorCategory::InvalidResponse => "invalid_response",
            InferenceErrorCategory::InvalidScope => "invalid_scope",
            InferenceErrorCategory::PromptTooLarge => "prompt_too_large",
            InferenceErrorCategory::Cancelled => "cancelled",
            InferenceErrorCategory::Misconfigured => "misconfigured",
            InferenceErrorCategory::ConnectionError => "connection_error",
            InferenceErrorCategory::UpstreamError => "upstream_error",
        }
    }
}

/// Sanitized provider error that never exposes credentials or raw upstream secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceError {
    pub category: InferenceErrorCategory,
    pub message: String,
    pub retry_after_secs: Option<u64>,
}

impl InferenceError {
    pub fn new(category: InferenceErrorCategory, message: impl Into<String>) -> Self {
        let raw = message.into();
        let scrubbed = privacy::redact_sensitive_text(&raw).redacted;
        Self {
            category,
            message: privacy::truncate_utf8(&scrubbed, 256),
            retry_after_secs: None,
        }
    }

    pub fn with_retry_after(mut self, secs: Option<u64>) -> Self {
        self.retry_after_secs = secs;
        self
    }
}

impl fmt::Display for InferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.category.as_str(), self.message)
    }
}

impl std::error::Error for InferenceError {}

/// Startup configuration selecting and bounding the active AI inference provider.
#[derive(Clone)]
pub struct AiProviderConfig {
    pub provider: ProviderKind,
    pub ollama_url: String,
    pub ollama_model: String,
    pub hf_url: Option<String>,
    /// Token is intentionally redacted in `Debug` output.
    pub hf_token: Option<String>,
    pub hf_model_id: String,
    pub hf_connect_timeout_secs: u64,
    pub hf_submit_timeout_secs: u64,
    pub hf_total_timeout_secs: u64,
    pub hf_max_concurrency: usize,
    pub hf_max_prompt_chars: usize,
    pub hf_max_output_bytes: usize,
    pub hf_circuit_breaker_cooldown_secs: u64,
}

impl fmt::Debug for AiProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiProviderConfig")
            .field("provider", &self.provider)
            .field("ollama_url", &self.ollama_url)
            .field("ollama_model", &self.ollama_model)
            .field("hf_url", &self.hf_url)
            .field(
                "hf_token",
                &self
                    .hf_token
                    .as_deref()
                    .filter(|t| !t.trim().is_empty())
                    .map(|_| "[REDACTED]"),
            )
            .field("hf_model_id", &self.hf_model_id)
            .field("hf_connect_timeout_secs", &self.hf_connect_timeout_secs)
            .field("hf_submit_timeout_secs", &self.hf_submit_timeout_secs)
            .field("hf_total_timeout_secs", &self.hf_total_timeout_secs)
            .field("hf_max_concurrency", &self.hf_max_concurrency)
            .field("hf_max_prompt_chars", &self.hf_max_prompt_chars)
            .field("hf_max_output_bytes", &self.hf_max_output_bytes)
            .field(
                "hf_circuit_breaker_cooldown_secs",
                &self.hf_circuit_breaker_cooldown_secs,
            )
            .finish()
    }
}

impl Default for AiProviderConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Ollama,
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "qwen2.5:0.5b".to_string(),
            hf_url: None,
            hf_token: None,
            hf_model_id: HF_AUTHORITATIVE_MODEL_ID.to_string(),
            hf_connect_timeout_secs: 5,
            hf_submit_timeout_secs: 10,
            hf_total_timeout_secs: 45,
            hf_max_concurrency: 2,
            hf_max_prompt_chars: 8192,
            hf_max_output_bytes: 16 * 1024,
            hf_circuit_breaker_cooldown_secs: 30,
        }
    }
}

impl AiProviderConfig {
    /// Strict environment configuration loader.
    /// - Absent or empty `ESA_AI_PROVIDER` defaults to `ollama`.
    /// - Only `"ollama"` and `"hf_zerogpu"` are accepted; any other value returns an explicit error.
    /// - Validates Hugging Face URL, token, and authoritative model identity when `hf_zerogpu` is selected.
    pub fn try_from_env() -> Result<Self, InferenceError> {
        let mut cfg = Self::default();
        if let Ok(p) = std::env::var("ESA_AI_PROVIDER") {
            let clean = p.trim().to_ascii_lowercase();
            if !clean.is_empty() {
                cfg.provider = match clean.as_str() {
                    "ollama" => ProviderKind::Ollama,
                    "hf_zerogpu" => ProviderKind::HfZerogpu,
                    other => {
                        return Err(InferenceError::new(
                            InferenceErrorCategory::Misconfigured,
                            format!(
                                "Unsupported ESA_AI_PROVIDER '{}' (supported: ollama, hf_zerogpu)",
                                other
                            ),
                        ));
                    }
                };
            }
        }
        if let Ok(url) = std::env::var("OLLAMA_URL") {
            if !url.trim().is_empty() {
                cfg.ollama_url = url.trim().to_string();
            }
        }
        if let Ok(model) = std::env::var("OLLAMA_MODEL") {
            if !model.trim().is_empty() {
                cfg.ollama_model = model.trim().to_string();
            }
        }
        cfg.hf_url = std::env::var("HF_AI_BRAIN_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty());
        cfg.hf_token = std::env::var("HF_AI_BRAIN_TOKEN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Ok(m) = std::env::var("HF_AI_MODEL_ID") {
            let clean = m.trim();
            if !clean.is_empty() {
                if clean != HF_AUTHORITATIVE_MODEL_ID {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::Misconfigured,
                        format!(
                            "HF_AI_MODEL_ID '{}' does not match authoritative Space model '{}'",
                            clean, HF_AUTHORITATIVE_MODEL_ID
                        ),
                    ));
                }
                cfg.hf_model_id = clean.to_string();
            }
        }
        cfg.hf_connect_timeout_secs = env_u64("HF_AI_CONNECT_TIMEOUT_SECS", 5, 1, 60);
        cfg.hf_submit_timeout_secs = env_u64("HF_AI_SUBMIT_TIMEOUT_SECS", 10, 1, 120);
        cfg.hf_total_timeout_secs = env_u64("HF_AI_TOTAL_TIMEOUT_SECS", 45, 2, 300);
        cfg.hf_max_concurrency = env_usize("HF_AI_MAX_CONCURRENCY", 2, 1, 32);
        cfg.hf_max_prompt_chars = env_usize("HF_AI_MAX_PROMPT_CHARS", 8192, 256, 65_536);
        cfg.hf_max_output_bytes = env_usize("HF_AI_MAX_OUTPUT_BYTES", 16 * 1024, 512, 262_144);
        cfg.hf_circuit_breaker_cooldown_secs =
            env_u64("HF_AI_CIRCUIT_BREAKER_COOLDOWN_SECS", 30, 5, 600);

        cfg.validate(false)?;
        Ok(cfg)
    }

    pub fn validate(&self, allow_insecure_loopback: bool) -> Result<(), InferenceError> {
        if self.provider == ProviderKind::HfZerogpu {
            let raw_url = self.hf_url.as_deref().ok_or_else(|| {
                InferenceError::new(
                    InferenceErrorCategory::Misconfigured,
                    "HF_AI_BRAIN_URL is required when ESA_AI_PROVIDER=hf_zerogpu",
                )
            })?;
            validate_hf_base_url(raw_url, allow_insecure_loopback)?;

            if self.hf_token.as_deref().is_none_or(|t| t.trim().is_empty()) {
                return Err(InferenceError::new(
                    InferenceErrorCategory::Misconfigured,
                    "HF_AI_BRAIN_TOKEN is required when ESA_AI_PROVIDER=hf_zerogpu",
                ));
            }
            if self.hf_model_id != HF_AUTHORITATIVE_MODEL_ID {
                return Err(InferenceError::new(
                    InferenceErrorCategory::Misconfigured,
                    format!(
                        "Configured Hugging Face model '{}' must match authoritative Space model '{}'",
                        self.hf_model_id, HF_AUTHORITATIVE_MODEL_ID
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// Validates a Hugging Face Space base URL.
/// - Requires `https://` for non-test remote endpoints (`http://127.0.0.1` / `http://localhost` allowed only in tests).
/// - Rejects embedded credentials (`user@host`), query strings (`?`), fragments (`#`), and whitespace/control chars.
pub fn validate_hf_base_url(
    raw: &str,
    allow_insecure_loopback_for_tests: bool,
) -> Result<String, InferenceError> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(InferenceError::new(
            InferenceErrorCategory::Misconfigured,
            "HF_AI_BRAIN_URL cannot be empty",
        ));
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(InferenceError::new(
            InferenceErrorCategory::Misconfigured,
            "HF_AI_BRAIN_URL contains invalid whitespace or control characters",
        ));
    }
    if trimmed.contains('?') || trimmed.contains('#') {
        return Err(InferenceError::new(
            InferenceErrorCategory::Misconfigured,
            "HF_AI_BRAIN_URL must not contain query parameters or fragments",
        ));
    }

    let (is_https, rest) = if let Some(r) = trimmed.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(InferenceError::new(
            InferenceErrorCategory::Misconfigured,
            "HF_AI_BRAIN_URL must use https:// scheme",
        ));
    };

    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return Err(InferenceError::new(
            InferenceErrorCategory::Misconfigured,
            "HF_AI_BRAIN_URL is missing a host",
        ));
    }
    if authority.contains('@') {
        return Err(InferenceError::new(
            InferenceErrorCategory::Misconfigured,
            "HF_AI_BRAIN_URL must not contain embedded credentials",
        ));
    }

    let (host, port_opt) = if let Some((h, p)) = authority.rsplit_once(':') {
        if h.contains(':') {
            return Err(InferenceError::new(
                InferenceErrorCategory::Misconfigured,
                "HF_AI_BRAIN_URL has malformed host/port",
            ));
        }
        let _port: u16 = p.parse().map_err(|_| {
            InferenceError::new(
                InferenceErrorCategory::Misconfigured,
                "HF_AI_BRAIN_URL has invalid port number",
            )
        })?;
        (h, Some(p))
    } else {
        (authority, None)
    };

    if host.is_empty() {
        return Err(InferenceError::new(
            InferenceErrorCategory::Misconfigured,
            "HF_AI_BRAIN_URL host cannot be empty",
        ));
    }
    let _ = port_opt;

    if !is_https {
        let is_loopback =
            host.eq_ignore_ascii_case("127.0.0.1") || host.eq_ignore_ascii_case("localhost");
        if !(allow_insecure_loopback_for_tests && is_loopback) {
            return Err(InferenceError::new(
                InferenceErrorCategory::Misconfigured,
                "HF_AI_BRAIN_URL must use https:// for remote Hugging Face endpoints",
            ));
        }
    }

    Ok(trimmed.to_string())
}

fn env_u64(key: &str, default: u64, min: u64, max: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|v| v.clamp(min, max))
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.clamp(min, max))
        .unwrap_or(default)
}

/// Provider-independent async inference interface.
#[async_trait]
pub trait InferenceProvider: Send + Sync {
    fn provider_kind(&self) -> ProviderKind;
    fn model_id(&self) -> &str;
    async fn infer(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError>;
    fn cost_tracker(&self) -> Arc<AICostTracker>;
    fn token_stats(&self) -> TokenStats;
}
