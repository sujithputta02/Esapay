use crate::hf_zerogpu::{HfZeroGpuConfig, HuggingFaceZeroGpuProvider};
use crate::ollama::{AICostTracker, OllamaClient, TokenCounter, TokenStats};
use crate::ollama_provider::OllamaProvider;
use crate::provider::{
    AiProviderConfig, InferenceError, InferenceProvider, InferenceRequest, InferenceResponse,
    ProviderKind,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

/// Fail-closed provider returned when `ESA_AI_PROVIDER` or Hugging Face config is invalid.
/// Never silently switches a misconfigured remote provider to Ollama, and never blocks startup/payments.
#[derive(Clone)]
pub struct MisconfiguredProvider {
    error: InferenceError,
    model_id: String,
    cost_tracker: Arc<AICostTracker>,
    token_counter: Arc<TokenCounter>,
}

impl MisconfiguredProvider {
    pub fn new(
        error: InferenceError,
        model_id: impl Into<String>,
        cost_tracker: Arc<AICostTracker>,
    ) -> Self {
        Self {
            error,
            model_id: model_id.into(),
            cost_tracker,
            token_counter: Arc::new(TokenCounter::new()),
        }
    }
}

#[async_trait]
impl InferenceProvider for MisconfiguredProvider {
    fn provider_kind(&self) -> ProviderKind {
        ProviderKind::DeterministicFallback
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    async fn infer(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        // Still enforce strict scope validation before reporting misconfiguration.
        let _ = request.scope.normalized_merchant_id()?;
        Err(self.error.clone())
    }

    fn cost_tracker(&self) -> Arc<AICostTracker> {
        Arc::clone(&self.cost_tracker)
    }

    fn token_stats(&self) -> TokenStats {
        self.token_counter.get_stats()
    }
}

/// Constructs the configured inference provider from environment variables at startup.
/// Never silently falls back from an invalid `ESA_AI_PROVIDER` or misconfigured `hf_zerogpu` to Ollama.
pub fn build_inference_provider_from_env(
    cost_tracker: Arc<AICostTracker>,
    no_proxy: bool,
) -> Arc<dyn InferenceProvider> {
    match AiProviderConfig::try_from_env() {
        Ok(cfg) => build_inference_provider(&cfg, cost_tracker, no_proxy),
        Err(err) => {
            error!(
                "Invalid AI provider configuration ({}); AI inference will fail closed to deterministic fallback without interrupting payments",
                err
            );
            Arc::new(MisconfiguredProvider::new(
                err,
                "misconfigured",
                cost_tracker,
            ))
        }
    }
}

/// Constructs the configured inference provider from an explicit `AiProviderConfig`.
/// Never fails startup or blocks payment execution; misconfigured remote providers fail closed on `infer()`.
pub fn build_inference_provider(
    config: &AiProviderConfig,
    cost_tracker: Arc<AICostTracker>,
    no_proxy: bool,
) -> Arc<dyn InferenceProvider> {
    match config.provider {
        ProviderKind::HfZerogpu => {
            if let Err(err) = config.validate(false) {
                warn!(
                    "Hugging Face ZeroGPU provider misconfigured ({}); failing closed without switching to Ollama",
                    err
                );
                return Arc::new(MisconfiguredProvider::new(
                    err,
                    config.hf_model_id.clone(),
                    cost_tracker,
                ));
            }
            let base_url = config.hf_url.clone().unwrap_or_default();
            info!(
                "Initialized Hugging Face ZeroGPU provider (model: {}, max_concurrency: {})",
                config.hf_model_id, config.hf_max_concurrency
            );
            let mut hf_cfg = HfZeroGpuConfig::new(
                base_url,
                config.hf_token.clone(),
                config.hf_model_id.clone(),
            );
            hf_cfg.connect_timeout = Duration::from_secs(config.hf_connect_timeout_secs);
            hf_cfg.submit_timeout = Duration::from_secs(config.hf_submit_timeout_secs);
            hf_cfg.total_timeout = Duration::from_secs(config.hf_total_timeout_secs);
            hf_cfg.max_concurrency = config.hf_max_concurrency;
            hf_cfg.max_prompt_chars = config.hf_max_prompt_chars;
            hf_cfg.max_output_bytes = config.hf_max_output_bytes;
            hf_cfg.circuit_breaker_cooldown =
                Duration::from_secs(config.hf_circuit_breaker_cooldown_secs);
            hf_cfg.no_proxy = no_proxy;

            Arc::new(HuggingFaceZeroGpuProvider::new(hf_cfg, cost_tracker))
        }
        ProviderKind::Ollama => {
            info!(
                "Initialized Ollama inference provider (model: {})",
                config.ollama_model
            );
            let client = if no_proxy {
                OllamaClient::with_cost_tracker_no_proxy(
                    config.ollama_url.clone(),
                    config.ollama_model.clone(),
                    cost_tracker,
                )
            } else {
                OllamaClient::with_cost_tracker(
                    config.ollama_url.clone(),
                    config.ollama_model.clone(),
                    cost_tracker,
                )
            };
            Arc::new(OllamaProvider::new(client))
        }
        ProviderKind::DeterministicFallback => Arc::new(MisconfiguredProvider::new(
            InferenceError::new(
                crate::provider::InferenceErrorCategory::Misconfigured,
                "DeterministicFallback cannot be selected as remote provider",
            ),
            "deterministic_rules_v1",
            cost_tracker,
        )),
    }
}
