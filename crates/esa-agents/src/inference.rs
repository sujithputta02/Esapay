use crate::hf_zerogpu::{HfZeroGpuConfig, HuggingFaceZeroGpuProvider};
use crate::ollama::{AICostTracker, OllamaClient};
use crate::ollama_provider::OllamaProvider;
use crate::provider::{AiProviderConfig, InferenceProvider, ProviderKind};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Constructs the configured inference provider at application startup.
/// Never fails startup or blocks payment execution if remote credentials are missing;
/// misconfigured remote providers fail closed on `infer()` so callers fall back safely.
pub fn build_inference_provider(
    config: &AiProviderConfig,
    cost_tracker: Arc<AICostTracker>,
    no_proxy: bool,
) -> Arc<dyn InferenceProvider> {
    match config.provider {
        ProviderKind::HfZerogpu => {
            let base_url = config.hf_url.clone().unwrap_or_default();
            if base_url.is_empty() || config.hf_token.as_ref().is_none_or(|t| t.is_empty()) {
                warn!(
                    "ESA_AI_PROVIDER=hf_zerogpu selected without complete HF_AI_BRAIN_URL/HF_AI_BRAIN_TOKEN; inference calls will fail closed to deterministic fallback"
                );
            } else {
                info!(
                    "Initialized Hugging Face ZeroGPU provider (model: {}, max_concurrency: {})",
                    config.hf_model_id, config.hf_max_concurrency
                );
            }
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
        ProviderKind::Ollama | ProviderKind::DeterministicFallback => {
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
    }
}
