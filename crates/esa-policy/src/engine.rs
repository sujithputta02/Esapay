use esa_core::{
    ActionProposal, ActionType, ConstraintValidator, EsaResult, IntentManager, RiskLevel,
};
use esa_state::StateFabric;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Policy Engine - Deterministic action validation and authorization

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PolicyVerdict {
    #[serde(rename = "ALLOWED")]
    Allowed,
    #[serde(rename = "DENIED")]
    Denied { reason: String },
    #[serde(rename = "MODIFIED")]
    Modified { modifications: Vec<String> },
    #[serde(rename = "REQUIRES_APPROVAL")]
    RequiresApproval { reason: String },
    #[serde(rename = "STALE_STATE")]
    StaleState {
        current_version: u64,
        proposed_version: u64,
        drift: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyResult {
    pub decision_id: String,
    pub proposal_id: String,
    pub verdict: PolicyVerdict,
    pub rule_ids: Vec<String>,
    pub risk_score: f64,
    pub modifications: Vec<String>,
    pub explanation: String,
}

pub struct PolicyEngine {
    state_fabric: Arc<StateFabric>,
    _intent_manager: Arc<IntentManager>,
    constraint_validator: ConstraintValidator,
    rules: Vec<PolicyRule>,
}

impl PolicyEngine {
    pub fn new(state_fabric: Arc<StateFabric>, intent_manager: Arc<IntentManager>) -> Self {
        let constraint_validator = ConstraintValidator::new(intent_manager.clone());

        Self {
            state_fabric,
            _intent_manager: intent_manager,
            constraint_validator,
            rules: Self::default_rules(),
        }
    }

    fn default_rules() -> Vec<PolicyRule> {
        vec![
            PolicyRule {
                id: "RULE_001".to_string(),
                check: Box::new(|proposal, state| {
                    if let ActionType::CreateReplica { workload_id, .. } = &proposal.action {
                        if let Some(workload) = state.get_workload(workload_id) {
                            if workload.replication.current_replicas
                                >= workload.replication.max_replicas
                            {
                                return Ok(PolicyVerdict::Denied {
                                    reason: format!(
                                        "Replica count {} already at max {}",
                                        workload.replication.current_replicas,
                                        workload.replication.max_replicas
                                    ),
                                });
                            }
                        }
                    }
                    Ok(PolicyVerdict::Allowed)
                }),
            },
            PolicyRule {
                id: "RULE_002".to_string(),
                check: Box::new(|proposal, _state| {
                    if matches!(&proposal.action, ActionType::Rollback { .. }) {
                        return Ok(PolicyVerdict::Allowed);
                    }

                    let risk = match &proposal.action {
                        ActionType::CreateReplica { risk, .. } => risk,
                        ActionType::ShiftRoute { risk, .. } => risk,
                        ActionType::MigratePartition { risk, .. } => risk,
                        ActionType::ThrottleWorkload { risk, .. } => risk,
                        ActionType::RestartWorkload { risk, .. } => risk,
                        ActionType::Rollback { .. } => &RiskLevel::Low,
                    };

                    if matches!(risk, RiskLevel::High | RiskLevel::Critical) {
                        Ok(PolicyVerdict::RequiresApproval {
                            reason: format!("Action has {:?} risk level", risk),
                        })
                    } else {
                        Ok(PolicyVerdict::Allowed)
                    }
                }),
            },
            PolicyRule {
                id: "RULE_003_STALE_STATE".to_string(),
                check: Box::new(|proposal, state| {
                    let state_version = match &proposal.action {
                        ActionType::CreateReplica { state_version, .. } => *state_version,
                        ActionType::ShiftRoute { state_version, .. } => *state_version,
                        ActionType::MigratePartition { state_version, .. } => *state_version,
                        ActionType::ThrottleWorkload { state_version, .. } => *state_version,
                        ActionType::RestartWorkload { .. } => return Ok(PolicyVerdict::Allowed),
                        ActionType::Rollback { .. } => return Ok(PolicyVerdict::Allowed),
                    };

                    let current_version = state.current_version();
                    let drift = current_version.saturating_sub(state_version);

                    // STRICT: Any state version mismatch is considered stale
                    if state_version != current_version {
                        Ok(PolicyVerdict::StaleState {
                            current_version,
                            proposed_version: state_version,
                            drift,
                        })
                    } else {
                        Ok(PolicyVerdict::Allowed)
                    }
                }),
            },
            PolicyRule {
                id: "RULE_004".to_string(),
                check: Box::new(|proposal, _state| {
                    let confidence = match &proposal.action {
                        ActionType::CreateReplica { confidence, .. } => *confidence,
                        ActionType::ShiftRoute { confidence, .. } => *confidence,
                        ActionType::MigratePartition { confidence, .. } => *confidence,
                        ActionType::ThrottleWorkload { confidence, .. } => *confidence,
                        ActionType::RestartWorkload { confidence, .. } => *confidence,
                        ActionType::Rollback { .. } => return Ok(PolicyVerdict::Allowed),
                    };

                    if confidence < 0.75 {
                        Ok(PolicyVerdict::RequiresApproval {
                            reason: format!("Confidence too low: {:.2}", confidence),
                        })
                    } else {
                        Ok(PolicyVerdict::Allowed)
                    }
                }),
            },
        ]
    }

    pub fn evaluate(&self, proposal: &ActionProposal) -> EsaResult<PolicyResult> {
        let mut applied_rules = Vec::new();
        let mut final_verdict = PolicyVerdict::Allowed;
        let mut modifications = Vec::new();
        let mut explanations = Vec::new();

        // Step 1: Validate against intent constraints
        match self.constraint_validator.validate_action(proposal) {
            Ok(violations) => {
                for violation in violations {
                    match violation.severity {
                        esa_core::ViolationSeverity::Critical => {
                            return Ok(PolicyResult {
                                decision_id: uuid::Uuid::new_v4().to_string(),
                                proposal_id: proposal.proposal_id.clone(),
                                verdict: PolicyVerdict::Denied {
                                    reason: format!(
                                        "Intent constraint violation: {}",
                                        violation.description
                                    ),
                                },
                                rule_ids: vec!["INTENT_CONSTRAINT".to_string()],
                                risk_score: 1.0,
                                modifications: Vec::new(),
                                explanation: violation.description,
                            });
                        }
                        esa_core::ViolationSeverity::Violation => {
                            explanations
                                .push(format!("Constraint violation: {}", violation.description));
                            final_verdict = PolicyVerdict::RequiresApproval {
                                reason: violation.description,
                            };
                        }
                        esa_core::ViolationSeverity::Warning => {
                            explanations
                                .push(format!("Constraint warning: {}", violation.description));
                        }
                    }
                }
            }
            Err(e) => {
                return Err(e);
            }
        }

        // Step 2: Apply standard policy rules
        for rule in &self.rules {
            let verdict = (rule.check)(proposal, &self.state_fabric)?;
            applied_rules.push(rule.id.clone());

            match verdict {
                PolicyVerdict::Denied { reason } => {
                    return Ok(PolicyResult {
                        decision_id: uuid::Uuid::new_v4().to_string(),
                        proposal_id: proposal.proposal_id.clone(),
                        verdict: PolicyVerdict::Denied {
                            reason: reason.clone(),
                        },
                        rule_ids: applied_rules,
                        risk_score: self.calculate_risk_score(proposal),
                        modifications: Vec::new(),
                        explanation: reason,
                    });
                }
                PolicyVerdict::StaleState {
                    current_version,
                    proposed_version,
                    drift,
                } => {
                    let reason = format!(
                        "STALE_STATE: Action planned against version {}, current version is {} (drift: {})",
                        proposed_version, current_version, drift
                    );
                    return Ok(PolicyResult {
                        decision_id: uuid::Uuid::new_v4().to_string(),
                        proposal_id: proposal.proposal_id.clone(),
                        verdict: PolicyVerdict::StaleState {
                            current_version,
                            proposed_version,
                            drift,
                        },
                        rule_ids: applied_rules,
                        risk_score: 0.0, // Stale state is not about risk
                        modifications: Vec::new(),
                        explanation: reason,
                    });
                }
                PolicyVerdict::RequiresApproval { reason } => {
                    final_verdict = PolicyVerdict::RequiresApproval {
                        reason: reason.clone(),
                    };
                    explanations.push(reason);
                }
                PolicyVerdict::Modified {
                    modifications: mods,
                } => {
                    modifications.extend(mods.clone());
                    final_verdict = PolicyVerdict::Modified {
                        modifications: modifications.clone(),
                    };
                }
                PolicyVerdict::Allowed => {}
            }
        }

        let explanation = if explanations.is_empty() {
            "All policy checks passed".to_string()
        } else {
            explanations.join("; ")
        };

        Ok(PolicyResult {
            decision_id: uuid::Uuid::new_v4().to_string(),
            proposal_id: proposal.proposal_id.clone(),
            verdict: final_verdict,
            rule_ids: applied_rules,
            risk_score: self.calculate_risk_score(proposal),
            modifications,
            explanation,
        })
    }

    fn calculate_risk_score(&self, proposal: &ActionProposal) -> f64 {
        let base_risk = match &proposal.action {
            ActionType::CreateReplica {
                risk, confidence, ..
            } => {
                let risk_val = match risk {
                    RiskLevel::Low => 0.2,
                    RiskLevel::Medium => 0.5,
                    RiskLevel::High => 0.8,
                    RiskLevel::Critical => 1.0,
                };
                risk_val * (1.0 - confidence * 0.5)
            }
            ActionType::ShiftRoute {
                risk, confidence, ..
            } => {
                let risk_val = match risk {
                    RiskLevel::Low => 0.3,
                    RiskLevel::Medium => 0.6,
                    RiskLevel::High => 0.85,
                    RiskLevel::Critical => 1.0,
                };
                risk_val * (1.0 - confidence * 0.5)
            }
            ActionType::MigratePartition { risk, .. } => match risk {
                RiskLevel::Low => 0.4,
                RiskLevel::Medium => 0.7,
                RiskLevel::High => 0.9,
                RiskLevel::Critical => 1.0,
            },
            ActionType::ThrottleWorkload { .. } => 0.85,
            ActionType::RestartWorkload { graceful, risk, .. } => {
                let base = if *graceful { 0.3 } else { 0.6 };
                match risk {
                    RiskLevel::Low => base,
                    RiskLevel::Medium => base + 0.2,
                    RiskLevel::High => base + 0.3,
                    RiskLevel::Critical => 1.0,
                }
            }
            ActionType::Rollback { .. } => 0.75,
        };

        base_risk.clamp(0.0, 1.0)
    }
}

struct PolicyRule {
    id: String,
    #[allow(clippy::type_complexity)]
    check: Box<dyn Fn(&ActionProposal, &StateFabric) -> EsaResult<PolicyVerdict> + Send + Sync>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use esa_core::*;

    #[test]
    fn test_policy_evaluation() {
        let fabric = Arc::new(StateFabric::new());
        let intent_manager = Arc::new(IntentManager::new());
        let engine = PolicyEngine::new(fabric.clone(), intent_manager);

        let proposal = ActionProposal::new(
            ActionType::CreateReplica {
                workload_id: "w_001".to_string(),
                target_region: Region::IndiaSouth,
                reason: "High latency".to_string(),
                expected_effect: ExpectedEffect {
                    latency_delta_ms: Some(-50.0),
                    throughput_delta_pct: None,
                    error_rate_delta: None,
                    queue_delta: None,
                    description: "Reduce latency".to_string(),
                },
                confidence: 0.9,
                risk: RiskLevel::Low,
                state_version: fabric.current_version(),
                rollback_enabled: true,
            },
            AgentId::Planning,
            vec!["evidence_1".to_string()],
        );

        let result = engine.evaluate(&proposal).unwrap();
        assert!(matches!(result.verdict, PolicyVerdict::Allowed));
    }
}
