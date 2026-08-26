use esa_core::*;
use esa_policy::{PolicyEngine, PolicyVerdict};
use esa_state::StateFabric;
use std::sync::Arc;

fn sample_create_replica(state_version: u64, risk: RiskLevel, confidence: f64) -> ActionType {
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
        confidence,
        risk,
        state_version,
        rollback_enabled: true,
    }
}

#[test]
fn test_policy_allows_low_risk_action() {
    let fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let engine = PolicyEngine::new(fabric, intent_manager);

    let proposal = ActionProposal::new(
        sample_create_replica(0, RiskLevel::Low, 0.9),
        AgentId::Planning,
        vec![],
    );
    let result = engine.evaluate(&proposal).unwrap();

    assert!(matches!(result.verdict, PolicyVerdict::Allowed));
}

#[test]
fn test_policy_requires_approval_for_high_risk() {
    let fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let engine = PolicyEngine::new(fabric, intent_manager);

    let proposal = ActionProposal::new(
        sample_create_replica(0, RiskLevel::High, 0.9),
        AgentId::Planning,
        vec![],
    );
    let result = engine.evaluate(&proposal).unwrap();

    assert!(matches!(result.verdict, PolicyVerdict::RequiresApproval { .. }));
}

#[test]
fn test_policy_denies_stale_state_version() {
    let fabric = Arc::new(StateFabric::new());

    for _ in 0..10 {
        fabric.increment_version();
    }

    let intent_manager = Arc::new(IntentManager::new());
    let engine = PolicyEngine::new(Arc::clone(&fabric), intent_manager);

    let proposal = ActionProposal::new(
        sample_create_replica(0, RiskLevel::Low, 0.9),
        AgentId::Planning,
        vec![],
    );
    let result = engine.evaluate(&proposal).unwrap();

    assert!(matches!(result.verdict, PolicyVerdict::StaleState { .. }));
}

#[test]
fn test_policy_requires_approval_for_low_confidence() {
    let fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let engine = PolicyEngine::new(fabric, intent_manager);

    let proposal = ActionProposal::new(
        sample_create_replica(0, RiskLevel::Low, 0.5),
        AgentId::Planning,
        vec![],
    );
    let result = engine.evaluate(&proposal).unwrap();

    assert!(matches!(result.verdict, PolicyVerdict::RequiresApproval { .. }));
}

#[test]
fn test_risk_score_calculation() {
    let fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let engine = PolicyEngine::new(fabric, intent_manager);

    let proposal = ActionProposal::new(
        sample_create_replica(0, RiskLevel::Low, 0.95),
        AgentId::Planning,
        vec![],
    );
    let result = engine.evaluate(&proposal).unwrap();

    assert!(result.risk_score < 0.3);
}
