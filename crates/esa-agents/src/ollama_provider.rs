use crate::ollama::{AICostTracker, OllamaClient, TokenStats};
use crate::provider::{
    InferenceError, InferenceErrorCategory, InferenceProvider, InferenceRequest, InferenceResponse,
    ProviderKind,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

/// Adapter wrapping `OllamaClient` behind the provider-independent `InferenceProvider` trait.
#[derive(Clone)]
pub struct OllamaProvider {
    client: OllamaClient,
}

impl OllamaProvider {
    pub fn new(client: OllamaClient) -> Self {
        Self { client }
    }

    pub fn inner(&self) -> &OllamaClient {
        &self.client
    }
}

#[async_trait]
impl InferenceProvider for OllamaProvider {
    fn provider_kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }

    fn model_id(&self) -> &str {
        self.client.model()
    }

    async fn infer(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        let prompt_chars = request.prompt.chars().count();
        if prompt_chars > request.limits.max_prompt_chars {
            return Err(InferenceError::new(
                InferenceErrorCategory::PromptTooLarge,
                format!(
                    "Prompt length ({} chars) exceeds limit ({})",
                    prompt_chars, request.limits.max_prompt_chars
                ),
            ));
        }

        let start = Instant::now();
        let merchant_id = request.scope.merchant_id();
        let res = self
            .client
            .generate_with_context(&request.agent_id, merchant_id, request.prompt.clone())
            .await
            .map_err(|e| map_ollama_error(&e.to_string()))?;

        let latency_ms = start.elapsed().as_millis() as u64;
        let input_tokens = (request.prompt.len() / 4).max(1);
        let output_tokens = (res.response.len() / 4).max(1);

        Ok(InferenceResponse {
            request_id: request.request_id,
            provider: ProviderKind::Ollama,
            model_id: self.client.model().to_string(),
            text: res.response,
            input_tokens,
            output_tokens,
            queue_wait_ms: None,
            gpu_execution_ms: None,
            total_latency_ms: latency_ms,
            cache_hit: false,
        })
    }

    fn cost_tracker(&self) -> Arc<AICostTracker> {
        self.client.get_cost_tracker()
    }

    fn token_stats(&self) -> TokenStats {
        self.client.get_token_stats()
    }
}

fn map_ollama_error(msg: &str) -> InferenceError {
    let lower = msg.to_ascii_lowercase();
    let cat = if lower.contains("circuit breaker") {
        InferenceErrorCategory::CircuitOpen
    } else if lower.contains("timeout") || lower.contains("timed out") {
        InferenceErrorCategory::Timeout
    } else if lower.contains("429") || lower.contains("rate limit") {
        InferenceErrorCategory::RateLimited
    } else if lower.contains("connect") || lower.contains("refused") {
        InferenceErrorCategory::ConnectionError
    } else {
        InferenceErrorCategory::UpstreamError
    };
    InferenceError::new(cat, msg)
}
