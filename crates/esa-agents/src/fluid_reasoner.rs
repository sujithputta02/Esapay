use crate::diagnosis::Diagnosis;
use crate::monitor::Condition;
use esa_core::{EsaError, EsaResult};
use reqwest::Client;
use std::time::Duration;
use tracing::info;

/// Client for the ARC-AGI-3 Fluid Reasoning Engine service.
#[derive(Clone, Debug)]
pub struct FluidReasonerClient {
    client: Client,
    endpoint: String,
}

impl FluidReasonerClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_millis(300))
            .build()
            .unwrap_or_default();
        Self {
            client,
            endpoint: endpoint.into(),
        }
    }

    pub fn default_local() -> Self {
        let endpoint = std::env::var("FLUID_REASONER_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:5005".to_string());
        Self::new(endpoint)
    }

    pub async fn is_available(&self) -> bool {
        let url = format!("{}/health", self.endpoint.trim_end_matches('/'));
        match self.client.get(&url).send().await {
            Ok(res) => res.status().is_success(),
            Err(_) => false,
        }
    }

    pub async fn diagnose(&self, conditions: &[Condition]) -> EsaResult<Diagnosis> {
        let url = format!("{}/diagnose", self.endpoint.trim_end_matches('/'));
        let body = serde_json::json!({ "conditions": conditions });

        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| EsaError::Internal(format!("Fluid reasoner connection failed: {}", e)))?;

        if !res.status().is_success() {
            return Err(EsaError::Internal(format!(
                "Fluid reasoner returned HTTP status {}",
                res.status()
            )));
        }

        let diagnosis: Diagnosis = res.json().await.map_err(|e| {
            EsaError::Internal(format!("Failed to parse fluid reasoner diagnosis: {}", e))
        })?;

        info!(
            "🧠 Fluid reasoner diagnosed: {} (cause: {:?}, confidence: {:.2})",
            diagnosis.hypothesis, diagnosis.root_cause, diagnosis.confidence
        );

        Ok(diagnosis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fluid_reasoner_unavailable_fallback() {
        // Point to an unused port
        let client = FluidReasonerClient::new("http://127.0.0.1:59999");
        assert!(!client.is_available().await);

        let conditions = vec![Condition {
            condition_type: crate::monitor::ConditionType::HighLatency,
            severity: crate::monitor::Severity::Medium,
            workload_id: "test-workload".to_string(),
            description: "Simulated high latency: 350ms".to_string(),
            metrics: serde_json::json!({
                "p95_latency_ms": 350.0,
                "error_rate": 0.01,
                "queue_depth": 50
            }),
        }];

        let result = client.diagnose(&conditions).await;
        assert!(result.is_err(), "Expected connection error to non-existent server");
    }

    #[test]
    fn test_fluid_reasoner_diagnosis_json_deserialization() {
        let json_data = r#"{
            "hypothesis": "Fluid reasoner induced isolated bottleneck in partition for workload 'payment-api-prod'",
            "root_cause": "HOT_PARTITION",
            "confidence": 0.94,
            "evidence_refs": ["queue_depth", "p95_latency_ms"],
            "recommended_action": "CREATE_REPLICA"
        }"#;

        let diagnosis: Diagnosis = serde_json::from_str(json_data).expect("Must parse valid diagnosis");
        assert_eq!(diagnosis.root_cause, crate::diagnosis::RootCause::HotPartition);
        assert_eq!(diagnosis.recommended_action, Some("CREATE_REPLICA".to_string()));
        assert!((diagnosis.confidence - 0.94).abs() < f64::EPSILON);
    }
}

