use crate::diagnosis::{Diagnosis, RootCause};
use crate::monitor::Condition;
use crate::privacy;
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;

/// Bounds and thresholds enforced on model-generated diagnosis outputs.
#[derive(Debug, Clone)]
pub struct ValidationLimits {
    pub max_output_bytes: usize,
    pub max_hypothesis_chars: usize,
    pub max_evidence_refs: usize,
    pub max_evidence_ref_chars: usize,
    pub max_action_chars: usize,
    pub min_confidence_for_action: f64,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 16 * 1024,
            max_hypothesis_chars: 512,
            max_evidence_refs: 16,
            max_evidence_ref_chars: 128,
            max_action_chars: 64,
            min_confidence_for_action: 0.4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    OutputTooLarge {
        actual: usize,
        max: usize,
    },
    MalformedJson(String),
    UnknownField(String),
    InvalidConfidence(String),
    InvalidRootCause(String),
    InvalidAction(String),
    EmptyHypothesis,
    OversizedField {
        field: String,
        actual: usize,
        max: usize,
    },
    TooManyEvidenceRefs {
        actual: usize,
        max: usize,
    },
    InvalidEvidenceRef(String),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::OutputTooLarge { actual, max } => {
                write!(f, "model output too large ({} bytes > {} max)", actual, max)
            }
            ValidationError::MalformedJson(e) => write!(f, "malformed diagnosis JSON: {}", e),
            ValidationError::UnknownField(msg) => write!(f, "unknown field rejected: {}", msg),
            ValidationError::InvalidConfidence(msg) => write!(f, "invalid confidence: {}", msg),
            ValidationError::InvalidRootCause(rc) => write!(f, "invalid root_cause: {}", rc),
            ValidationError::InvalidAction(act) => {
                write!(f, "unsupported recommended_action: {}", act)
            }
            ValidationError::EmptyHypothesis => write!(f, "diagnosis hypothesis cannot be empty"),
            ValidationError::OversizedField { field, actual, max } => {
                write!(
                    f,
                    "field '{}' exceeds max length ({} > {})",
                    field, actual, max
                )
            }
            ValidationError::TooManyEvidenceRefs { actual, max } => {
                write!(f, "too many evidence_refs ({} > {})", actual, max)
            }
            ValidationError::InvalidEvidenceRef(r) => {
                write!(f, "ungrounded or invalid evidence_ref: {}", r)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDiagnosisPayload {
    hypothesis: String,
    root_cause: String,
    confidence: f64,
    evidence_refs: Vec<String>,
    #[serde(default)]
    recommended_action: Option<String>,
}

/// Provider-agnostic validator for structured AI diagnosis responses.
#[derive(Debug, Clone)]
pub struct OutputValidator {
    limits: ValidationLimits,
}

impl OutputValidator {
    pub fn new(limits: ValidationLimits) -> Self {
        Self { limits }
    }

    pub fn with_default_limits() -> Self {
        Self::new(ValidationLimits::default())
    }

    /// Parses and strictly validates raw text from any model provider (Ollama, HF ZeroGPU, etc.).
    pub fn validate_diagnosis_response(
        &self,
        raw_response: &str,
        conditions: &[Condition],
    ) -> Result<Diagnosis, ValidationError> {
        if raw_response.len() > self.limits.max_output_bytes {
            return Err(ValidationError::OutputTooLarge {
                actual: raw_response.len(),
                max: self.limits.max_output_bytes,
            });
        }

        // Strip hidden chain-of-thought before extracting JSON so <think> blocks cannot smuggle JSON.
        let stripped = privacy::strip_chain_of_thought(raw_response);
        let json_str = extract_json_object(&stripped);

        let mut value: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| ValidationError::MalformedJson(e.to_string()))?;

        // Validate field types strictly before deserialization, normalizing only safe null action.
        let obj = value
            .as_object_mut()
            .ok_or_else(|| ValidationError::MalformedJson("expected JSON object".to_string()))?;

        if let Some(action) = obj.get("recommended_action") {
            if action.is_null() || action.as_str() == Some("null") {
                obj.remove("recommended_action");
            }
        }

        let payload: RawDiagnosisPayload = serde_json::from_value(value).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unknown field") {
                ValidationError::UnknownField(msg)
            } else {
                ValidationError::MalformedJson(msg)
            }
        })?;

        let root_cause = parse_strict_root_cause(&payload.root_cause)?;

        let draft = Diagnosis {
            hypothesis: payload.hypothesis,
            root_cause,
            confidence: payload.confidence,
            evidence_refs: payload.evidence_refs,
            recommended_action: payload.recommended_action,
        };

        self.validate_structured_diagnosis(&draft, conditions)
    }

    /// Validates an already-deserialized `Diagnosis` (e.g. from FluidReasonerClient or tests).
    pub fn validate_structured_diagnosis(
        &self,
        diagnosis: &Diagnosis,
        conditions: &[Condition],
    ) -> Result<Diagnosis, ValidationError> {
        if !diagnosis.confidence.is_finite() || !(0.0..=1.0).contains(&diagnosis.confidence) {
            return Err(ValidationError::InvalidConfidence(format!(
                "{} must be finite and within [0.0, 1.0]",
                diagnosis.confidence
            )));
        }

        let clean_hypothesis = privacy::redact_sensitive_text(diagnosis.hypothesis.trim()).redacted;
        if clean_hypothesis.is_empty() {
            return Err(ValidationError::EmptyHypothesis);
        }
        let hyp_chars = clean_hypothesis.chars().count();
        if hyp_chars > self.limits.max_hypothesis_chars {
            return Err(ValidationError::OversizedField {
                field: "hypothesis".to_string(),
                actual: hyp_chars,
                max: self.limits.max_hypothesis_chars,
            });
        }

        if diagnosis.evidence_refs.len() > self.limits.max_evidence_refs {
            return Err(ValidationError::TooManyEvidenceRefs {
                actual: diagnosis.evidence_refs.len(),
                max: self.limits.max_evidence_refs,
            });
        }

        let allowed_refs = allowed_evidence_tokens(conditions);
        let mut validated_refs = Vec::with_capacity(diagnosis.evidence_refs.len());
        for raw_ref in &diagnosis.evidence_refs {
            let trimmed = raw_ref.trim();
            if trimmed.is_empty() {
                return Err(ValidationError::InvalidEvidenceRef(
                    "empty reference".into(),
                ));
            }
            let ref_chars = trimmed.chars().count();
            if ref_chars > self.limits.max_evidence_ref_chars {
                return Err(ValidationError::OversizedField {
                    field: "evidence_refs[]".to_string(),
                    actual: ref_chars,
                    max: self.limits.max_evidence_ref_chars,
                });
            }
            if !is_valid_evidence_ref(trimmed, &allowed_refs) {
                return Err(ValidationError::InvalidEvidenceRef(trimmed.to_string()));
            }
            validated_refs.push(trimmed.to_string());
        }

        let validated_action = match diagnosis.recommended_action.as_deref() {
            None => None,
            Some(raw_act) => {
                let trimmed = raw_act.trim();
                let act_chars = trimmed.chars().count();
                if act_chars > self.limits.max_action_chars {
                    return Err(ValidationError::OversizedField {
                        field: "recommended_action".to_string(),
                        actual: act_chars,
                        max: self.limits.max_action_chars,
                    });
                }
                match trimmed.to_ascii_uppercase().as_str() {
                    "" | "NONE" | "NULL" | "ABSTAIN" => None,
                    "CREATE_REPLICA" => Some("CREATE_REPLICA".to_string()),
                    "SHIFT_ROUTE" => Some("SHIFT_ROUTE".to_string()),
                    other => return Err(ValidationError::InvalidAction(other.to_string())),
                }
            }
        };

        // Explicit abstain / uncertain enforcement:
        // - RootCause::Uncertain or RootCause::None never emits a mutating action.
        // - Low confidence or ungrounded diagnosis (conditions present but zero evidence_refs) abstains.
        let (final_root_cause, final_action) = if diagnosis.root_cause == RootCause::Uncertain
            || diagnosis.root_cause == RootCause::None
        {
            (diagnosis.root_cause.clone(), None)
        } else if (!conditions.is_empty() && validated_refs.is_empty())
            || diagnosis.confidence < self.limits.min_confidence_for_action
        {
            (RootCause::Uncertain, None)
        } else {
            (diagnosis.root_cause.clone(), validated_action)
        };

        Ok(Diagnosis {
            hypothesis: clean_hypothesis,
            root_cause: final_root_cause,
            confidence: diagnosis.confidence,
            evidence_refs: validated_refs,
            recommended_action: final_action,
        })
    }
}

fn parse_strict_root_cause(raw: &str) -> Result<RootCause, ValidationError> {
    let normalized = raw.trim().to_ascii_uppercase().replace(' ', "_");
    match normalized.as_str() {
        "HOT_PARTITION" => Ok(RootCause::HotPartition),
        "CAPACITY_ISSUE" => Ok(RootCause::CapacityIssue),
        "NODE_DEGRADATION" => Ok(RootCause::NodeDegradation),
        "TRAFFIC_SPIKE" => Ok(RootCause::TrafficSpike),
        "OTHER" => Ok(RootCause::Other),
        "NONE" => Ok(RootCause::None),
        "UNCERTAIN" | "ABSTAIN" => Ok(RootCause::Uncertain),
        _ => Err(ValidationError::InvalidRootCause(raw.trim().to_string())),
    }
}

fn allowed_evidence_tokens(conditions: &[Condition]) -> HashSet<String> {
    let mut set: HashSet<String> = [
        "p50_latency_ms",
        "p95_latency_ms",
        "p99_latency_ms",
        "rate_per_min",
        "queue_depth",
        "error_rate",
        "workload_state",
        "current_replicas",
        "HIGH_LATENCY",
        "QUEUE_BACKLOG",
        "HIGH_ERROR_RATE",
        "WORKLOAD_DEGRADED",
        "NODE_FAILURE",
    ]
    .iter()
    .map(|s| s.to_ascii_lowercase())
    .collect();

    for c in conditions {
        set.insert(c.workload_id.to_ascii_lowercase());
        set.insert(c.condition_type.to_string().to_ascii_lowercase());
    }
    set
}

fn is_valid_evidence_ref(reference: &str, allowed: &HashSet<String>) -> bool {
    if reference.contains('\n') || reference.contains('\r') {
        return false;
    }
    let lower = reference.to_ascii_lowercase();
    if allowed.contains(&lower) {
        return true;
    }
    // Allow metric=value or workload:metric forms only if prefix matches a known allowed token
    // and contains no control/script characters.
    for token in allowed {
        if lower.starts_with(&format!("{}=", token))
            || lower.starts_with(&format!("{}:", token))
            || lower.starts_with(&format!("{}.", token))
        {
            return true;
        }
    }
    false
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::{ConditionType, Severity};

    fn sample_condition() -> Condition {
        Condition {
            workload_id: "payment-upi-india-south".to_string(),
            condition_type: ConditionType::HighLatency,
            severity: Severity::Medium,
            description: "P95 latency elevated".to_string(),
            metrics: serde_json::json!({"p95_latency_ms": 280.0}),
        }
    }

    #[test]
    fn test_rejects_unknown_fields() {
        let validator = OutputValidator::with_default_limits();
        let raw = r#"{"hypothesis":"High latency","root_cause":"CAPACITY_ISSUE","confidence":0.85,"evidence_refs":["p95_latency_ms"],"recommended_action":"CREATE_REPLICA","execute_payment":true}"#;
        let err = validator
            .validate_diagnosis_response(raw, &[sample_condition()])
            .unwrap_err();
        assert!(matches!(err, ValidationError::UnknownField(_)));
    }

    #[test]
    fn test_rejects_out_of_bounds_confidence_and_non_finite() {
        let validator = OutputValidator::with_default_limits();
        let high = r#"{"hypothesis":"High latency","root_cause":"CAPACITY_ISSUE","confidence":1.5,"evidence_refs":["p95_latency_ms"],"recommended_action":null}"#;
        assert!(matches!(
            validator.validate_diagnosis_response(high, &[sample_condition()]),
            Err(ValidationError::InvalidConfidence(_))
        ));

        let neg = r#"{"hypothesis":"High latency","root_cause":"CAPACITY_ISSUE","confidence":-0.1,"evidence_refs":["p95_latency_ms"],"recommended_action":null}"#;
        assert!(matches!(
            validator.validate_diagnosis_response(neg, &[sample_condition()]),
            Err(ValidationError::InvalidConfidence(_))
        ));
    }

    #[test]
    fn test_rejects_oversized_strings_and_arrays() {
        let validator = OutputValidator::with_default_limits();
        let long_hyp = "a".repeat(600);
        let raw = format!(
            r#"{{"hypothesis":"{}","root_cause":"CAPACITY_ISSUE","confidence":0.8,"evidence_refs":["p95_latency_ms"]}}"#,
            long_hyp
        );
        assert!(matches!(
            validator.validate_diagnosis_response(&raw, &[sample_condition()]),
            Err(ValidationError::OversizedField { .. })
        ));

        let many_refs: Vec<String> = (0..20).map(|_| "p95_latency_ms".to_string()).collect();
        let raw_arr = serde_json::json!({
            "hypothesis": "High latency",
            "root_cause": "CAPACITY_ISSUE",
            "confidence": 0.8,
            "evidence_refs": many_refs,
        })
        .to_string();
        assert!(matches!(
            validator.validate_diagnosis_response(&raw_arr, &[sample_condition()]),
            Err(ValidationError::TooManyEvidenceRefs { .. })
        ));
    }

    #[test]
    fn test_rejects_ungrounded_evidence_ref_and_invalid_action() {
        let validator = OutputValidator::with_default_limits();
        let bad_ref = r#"{"hypothesis":"High latency","root_cause":"CAPACITY_ISSUE","confidence":0.8,"evidence_refs":["customer_credit_card"]}"#;
        assert!(matches!(
            validator.validate_diagnosis_response(bad_ref, &[sample_condition()]),
            Err(ValidationError::InvalidEvidenceRef(_))
        ));

        let bad_act = r#"{"hypothesis":"High latency","root_cause":"CAPACITY_ISSUE","confidence":0.8,"evidence_refs":["p95_latency_ms"],"recommended_action":"DROP_DATABASE"}"#;
        assert!(matches!(
            validator.validate_diagnosis_response(bad_act, &[sample_condition()]),
            Err(ValidationError::InvalidAction(_))
        ));
    }

    #[test]
    fn test_low_confidence_or_uncertain_abstains_without_action() {
        let validator = OutputValidator::with_default_limits();
        let low_conf = r#"{"hypothesis":"Unclear telemetry","root_cause":"CAPACITY_ISSUE","confidence":0.2,"evidence_refs":["p95_latency_ms"],"recommended_action":"CREATE_REPLICA"}"#;
        let res = validator
            .validate_diagnosis_response(low_conf, &[sample_condition()])
            .unwrap();
        assert_eq!(res.root_cause, RootCause::Uncertain);
        assert_eq!(res.recommended_action, None);

        let explicit_uncertain = r#"{"hypothesis":"Insufficient evidence","root_cause":"UNCERTAIN","confidence":0.5,"evidence_refs":["p95_latency_ms"],"recommended_action":"SHIFT_ROUTE"}"#;
        let res2 = validator
            .validate_diagnosis_response(explicit_uncertain, &[sample_condition()])
            .unwrap();
        assert_eq!(res2.root_cause, RootCause::Uncertain);
        assert_eq!(res2.recommended_action, None);
    }
}
