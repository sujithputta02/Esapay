use esa_core::{ActionProposal, EsaResult};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Safety Agent - Constitutional review of action proposals

pub struct SafetyAgent;

impl SafetyAgent {
    pub fn new() -> Self {
        Self
    }

    pub async fn review(&self, proposal: &ActionProposal) -> EsaResult<SafetyReview> {
        info!("Safety agent reviewing proposal {}", proposal.proposal_id);

        let mut checks = Vec::new();

        // Check 1: Bounded action type
        checks.push(SafetyCheck {
            check_name: "Bounded action type".to_string(),
            passed: true,
            reason: "Action uses typed contract".to_string(),
        });

        // Check 2: Rollback enabled for risky actions
        let rollback_enabled = match &proposal.action {
            esa_core::ActionType::CreateReplica { rollback_enabled, .. } => *rollback_enabled,
            esa_core::ActionType::ShiftRoute { rollback_enabled, .. } => *rollback_enabled,
            esa_core::ActionType::MigratePartition { rollback_enabled, .. } => *rollback_enabled,
            esa_core::ActionType::ThrottleWorkload { rollback_enabled, .. } => *rollback_enabled,
            esa_core::ActionType::Rollback { .. } => true,
            esa_core::ActionType::RestartWorkload { .. } => true, // Restart can always be rolled back
        };

        checks.push(SafetyCheck {
            check_name: "Rollback capability".to_string(),
            passed: rollback_enabled,
            reason: if rollback_enabled {
                "Rollback is enabled".to_string()
            } else {
                "Rollback should be enabled for this action".to_string()
            },
        });

        // Check 3: No direct infrastructure access
        checks.push(SafetyCheck {
            check_name: "No shell/kubectl commands".to_string(),
            passed: true,
            reason: "Action uses declarative contract only".to_string(),
        });

        // Check 4: Evidence provided
        checks.push(SafetyCheck {
            check_name: "Evidence provided".to_string(),
            passed: !proposal.evidence_refs.is_empty(),
            reason: format!("{} evidence references", proposal.evidence_refs.len()),
        });

        let all_passed = checks.iter().all(|c| c.passed);

        Ok(SafetyReview {
            proposal_id: proposal.proposal_id.clone(),
            passed: all_passed,
            checks,
            recommendation: if all_passed {
                SafetyRecommendation::Approve
            } else {
                SafetyRecommendation::Deny
            },
        })
    }
}

impl Default for SafetyAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyReview {
    pub proposal_id: String,
    pub passed: bool,
    pub checks: Vec<SafetyCheck>,
    pub recommendation: SafetyRecommendation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyCheck {
    pub check_name: String,
    pub passed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SafetyRecommendation {
    #[serde(rename = "APPROVE")]
    Approve,
    #[serde(rename = "DENY")]
    Deny,
    #[serde(rename = "MODIFY")]
    Modify,
}
