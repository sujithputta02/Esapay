use crate::monitor::Condition;
use crate::ollama::OllamaClient;
use esa_core::EsaResult;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Diagnosis Agent - Analyzes conditions and identifies root causes
pub struct DiagnosisAgent {
    ollama_client: OllamaClient,
}

impl DiagnosisAgent {
    pub fn new(ollama_client: OllamaClient) -> Self {
        Self { ollama_client }
    }

    pub async fn diagnose(&self, conditions: &[Condition]) -> EsaResult<Diagnosis> {
        if conditions.is_empty() {
            return Ok(Diagnosis {
                hypothesis: "No conditions detected".to_string(),
                root_cause: RootCause::None,
                confidence: 1.0,
                evidence_refs: vec![],
                recommended_action: None,
            });
        }

        // Build diagnosis prompt
        let prompt = self.build_diagnosis_prompt(conditions);

        info!("Diagnosis agent analyzing {} conditions", conditions.len());

        // For MVP, use rule-based diagnosis if Ollama is unavailable
        match self
            .ollama_client
            .generate_with_agent("diagnosis", prompt)
            .await
        {
            Ok(response) => {
                // Parse LLM response
                match self.parse_diagnosis_response(&response.response, conditions) {
                    Ok(diagnosis) => Ok(diagnosis),
                    Err(e) => {
                        warn!(
                            "Failed to parse LLM diagnosis, falling back to rules: {}",
                            e
                        );
                        Ok(self.rule_based_diagnosis(conditions))
                    }
                }
            }
            Err(e) => {
                warn!("Ollama unavailable, using rule-based diagnosis: {}", e);
                Ok(self.rule_based_diagnosis(conditions))
            }
        }
    }

    fn build_diagnosis_prompt(&self, conditions: &[Condition]) -> String {
        let conditions_json = serde_json::to_string(conditions).unwrap_or_default();

        format!(
            r#"You are a payment infrastructure diagnosis agent. Analyze the following conditions and identify the root cause.

Conditions:
{}

Respond in JSON format:
{{
  "hypothesis": "brief description of the issue",
  "root_cause": "ONE of: HOT_PARTITION, CAPACITY_ISSUE, NODE_DEGRADATION, TRAFFIC_SPIKE, OTHER",
  "confidence": 0.9,
  "evidence_refs": ["metric names or observations"],
  "recommended_action": "CREATE_REPLICA|SHIFT_ROUTE|null"

Use numeric confidence (e.g. 0.9), not a string.
}}

Use exactly one root_cause value — never combine with | or /.

Be concise and focus on observable metrics."#,
            conditions_json
        )
    }

    fn parse_diagnosis_response(
        &self,
        response: &str,
        _conditions: &[Condition],
    ) -> Result<Diagnosis, String> {
        let json_str = extract_json_object(response);
        let mut value: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("Failed to parse diagnosis JSON: {}", e))?;

        if let Some(obj) = value.as_object_mut() {
            if let Some(rc) = obj.get("root_cause").and_then(|v| v.as_str()) {
                obj.insert(
                    "root_cause".to_string(),
                    serde_json::Value::String(normalize_root_cause_str(rc)),
                );
            }
            coerce_diagnosis_json_fields(obj);
        }

        serde_json::from_value(value).map_err(|e| format!("Failed to parse diagnosis JSON: {}", e))
    }

    fn rule_based_diagnosis(&self, conditions: &[Condition]) -> Diagnosis {
        use crate::monitor::ConditionType;

        let has_high_latency = conditions
            .iter()
            .any(|c| c.condition_type == ConditionType::HighLatency);
        let has_queue_backlog = conditions
            .iter()
            .any(|c| c.condition_type == ConditionType::QueueBacklog);
        let is_degraded = conditions
            .iter()
            .any(|c| c.condition_type == ConditionType::WorkloadDegraded);
        let has_high_error = conditions
            .iter()
            .any(|c| c.condition_type == ConditionType::HighErrorRate);

        if has_high_latency && has_queue_backlog {
            Diagnosis {
                hypothesis: "Hot partition causing latency and queue buildup".to_string(),
                root_cause: RootCause::HotPartition,
                confidence: 0.85,
                evidence_refs: vec!["p95_latency_ms".to_string(), "queue_depth".to_string()],
                recommended_action: Some("CREATE_REPLICA".to_string()),
            }
        } else if has_high_latency {
            Diagnosis {
                hypothesis: "Capacity issue causing increased latency".to_string(),
                root_cause: RootCause::CapacityIssue,
                confidence: 0.75,
                evidence_refs: vec!["p95_latency_ms".to_string()],
                recommended_action: Some("CREATE_REPLICA".to_string()),
            }
        } else if is_degraded || has_high_error || has_queue_backlog {
            Diagnosis {
                hypothesis: "Workload degradation requires capacity recovery".to_string(),
                root_cause: if has_queue_backlog {
                    RootCause::HotPartition
                } else {
                    RootCause::CapacityIssue
                },
                confidence: 0.8,
                evidence_refs: conditions
                    .iter()
                    .map(|c| c.condition_type.to_string())
                    .collect(),
                recommended_action: Some("CREATE_REPLICA".to_string()),
            }
        } else {
            Diagnosis {
                hypothesis: "Workload degradation detected".to_string(),
                root_cause: RootCause::Other,
                confidence: 0.6,
                evidence_refs: conditions
                    .iter()
                    .map(|c| c.condition_type.to_string())
                    .collect(),
                recommended_action: None,
            }
        }
    }
}

fn coerce_diagnosis_json_fields(obj: &mut serde_json::Map<String, serde_json::Value>) {
    if let Some(conf) = obj.get("confidence") {
        if let Some(s) = conf.as_str() {
            if let Ok(v) = s.parse::<f64>() {
                obj.insert("confidence".to_string(), serde_json::json!(v));
            }
        }
    }

    if let Some(action) = obj.get("recommended_action") {
        if action.is_null() || action.as_str() == Some("null") {
            obj.remove("recommended_action");
        }
    }
}

fn extract_json_object(text: &str) -> String {
    let text = text.trim();
    if let Some(start) = text.find("```") {
        let after_fence = &text[start + 3..];
        let content = after_fence
            .strip_prefix("json")
            .unwrap_or(after_fence)
            .trim();
        if let Some(end) = content.find("```") {
            return content[..end].trim().to_string();
        }
    }
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        return text[start..=end].to_string();
    }
    text.to_string()
}

fn normalize_root_cause_str(s: &str) -> String {
    let token = s.split('|').next().unwrap_or(s).trim();
    let upper = token.to_uppercase().replace(' ', "_");
    if upper.contains("HOT_PARTITION") {
        "HOT_PARTITION".to_string()
    } else if upper.contains("CAPACITY") {
        "CAPACITY_ISSUE".to_string()
    } else if upper.contains("NODE") {
        "NODE_DEGRADATION".to_string()
    } else if upper.contains("TRAFFIC") {
        "TRAFFIC_SPIKE".to_string()
    } else if upper == "OTHER" {
        "OTHER".to_string()
    } else if upper == "NONE" {
        "NONE".to_string()
    } else {
        "OTHER".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    pub hypothesis: String,
    pub root_cause: RootCause,
    pub confidence: f64,
    pub evidence_refs: Vec<String>,
    pub recommended_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RootCause {
    #[serde(rename = "HOT_PARTITION")]
    HotPartition,
    #[serde(rename = "CAPACITY_ISSUE")]
    CapacityIssue,
    #[serde(rename = "NODE_DEGRADATION")]
    NodeDegradation,
    #[serde(rename = "TRAFFIC_SPIKE")]
    TrafficSpike,
    #[serde(rename = "OTHER")]
    Other,
    #[serde(rename = "NONE")]
    None,
}

impl std::fmt::Display for crate::monitor::ConditionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            crate::monitor::ConditionType::HighLatency => write!(f, "HIGH_LATENCY"),
            crate::monitor::ConditionType::QueueBacklog => write!(f, "QUEUE_BACKLOG"),
            crate::monitor::ConditionType::HighErrorRate => write!(f, "HIGH_ERROR_RATE"),
            crate::monitor::ConditionType::WorkloadDegraded => write!(f, "WORKLOAD_DEGRADED"),
            crate::monitor::ConditionType::NodeFailure => write!(f, "NODE_FAILURE"),
        }
    }
}
