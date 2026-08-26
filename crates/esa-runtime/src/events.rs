use std::sync::Arc;

/// Runtime events emitted during autonomous orchestration cycles.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    AgentActivity {
        agent_id: String,
        activity: String,
    },
    ConditionDetected {
        condition_type: String,
        workload_id: String,
        severity: String,
        description: String,
    },
    ActionProposed {
        proposal_id: String,
        action_type: String,
        workload_id: String,
    },
    ActionExecuted {
        execution_id: String,
        action_type: String,
        outcome: String,
    },
    PolicyDecision {
        decision_id: String,
        verdict: String,
        risk_score: f64,
        workload_id: String,
        action_type: String,
    },
    WorkloadUpdate {
        workload_id: String,
        state: String,
        metrics: serde_json::Value,
    },
}

pub type RuntimeEventHandler = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;
