use esa_core::{ActionProposal, EsaResult};
use esa_state::StateFabric;
use std::sync::Arc;

/// Decision Verifier - Validates proposals against current runtime state
pub struct DecisionVerifier {
    state_fabric: Arc<StateFabric>,
}

impl DecisionVerifier {
    pub fn new(state_fabric: Arc<StateFabric>) -> Self {
        Self { state_fabric }
    }

    pub fn verify(&self, proposal: &ActionProposal) -> EsaResult<VerificationResult> {
        let current_version = self.state_fabric.current_version();

        let checks = vec![
            self.verify_state_version(proposal, current_version),
            self.verify_workload_exists(proposal),
            self.verify_resource_limits(proposal),
        ];

        let mut passed = true;
        let mut failures = Vec::new();

        for check in checks {
            if let Err(e) = check {
                passed = false;
                failures.push(e.to_string());
            }
        }

        Ok(VerificationResult {
            passed,
            failures,
            current_state_version: current_version,
        })
    }

    fn verify_state_version(&self, proposal: &ActionProposal, current: u64) -> EsaResult<()> {
        use esa_core::ActionType;

        let proposed_version = match &proposal.action {
            ActionType::CreateReplica { state_version, .. } => *state_version,
            ActionType::ShiftRoute { state_version, .. } => *state_version,
            ActionType::MigratePartition { state_version, .. } => *state_version,
            ActionType::ThrottleWorkload { state_version, .. } => *state_version,
            ActionType::RestartWorkload { .. } => return Ok(()),
            ActionType::Rollback { .. } => return Ok(()),
        };

        if current.saturating_sub(proposed_version) > 5 {
            return Err(esa_core::EsaError::StateVersionMismatch {
                expected: current,
                actual: proposed_version,
            });
        }

        Ok(())
    }

    fn verify_workload_exists(&self, proposal: &ActionProposal) -> EsaResult<()> {
        use esa_core::ActionType;

        let workload_id = match &proposal.action {
            ActionType::CreateReplica { workload_id, .. } => workload_id,
            ActionType::ShiftRoute { workload_id, .. } => workload_id,
            ActionType::MigratePartition { workload_id, .. } => workload_id,
            ActionType::ThrottleWorkload { workload_id, .. } => workload_id,
            ActionType::RestartWorkload { workload_id, .. } => workload_id,
            ActionType::Rollback { .. } => return Ok(()),
        };

        if self.state_fabric.get_workload(workload_id).is_none() {
            return Err(esa_core::EsaError::ResourceNotFound {
                resource: format!("workload {}", workload_id),
            });
        }

        Ok(())
    }

    fn verify_resource_limits(&self, _proposal: &ActionProposal) -> EsaResult<()> {
        // Check resource quotas, budget limits, etc.
        // For MVP, this is a placeholder
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub passed: bool,
    pub failures: Vec<String>,
    pub current_state_version: u64,
}
