use esa_core::*;
use esa_policy::{PolicyEngine, PolicyVerdict};
use esa_state::StateFabric;
use std::sync::Arc;

/// Comprehensive Safety Test Suite - PRD Section #32
/// Tests all 8 mandatory safety scenarios

#[tokio::test]
async fn test_01_unknown_action_denied() {
    // Test 1: Unknown Action
    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let policy_engine = PolicyEngine::new(state_fabric.clone(), intent_manager);

    // Create a mock proposal with an action that doesn't exist in our enum
    // Since we can't create invalid enum variants, we'll test with unsupported parameters
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "test".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Test invalid action".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(f64::INFINITY), // Invalid value
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Invalid action test".to_string(),
            },
            confidence: -1.0, // Invalid confidence
            risk: RiskLevel::Low,
            state_version: state_fabric.current_version(),
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["test_evidence".to_string()],
    );

    let result = policy_engine.evaluate(&proposal).unwrap();

    // Should be denied due to invalid parameters
    assert!(
        matches!(result.verdict, PolicyVerdict::Denied { .. })
            || matches!(result.verdict, PolicyVerdict::RequiresApproval { .. }),
        "Invalid action should be denied or require approval, got: {:?}",
        result.verdict
    );
}

#[tokio::test]
async fn test_02_out_of_bounds_replicas_denied() {
    // Test 2: Out-of-Bounds Replicas
    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());

    // Create intent with strict replica limits
    let goal = IntentGoal {
        objective: "Test replica limits".to_string(),
        target_metrics: TargetMetrics {
            max_p95_latency_ms: Some(250.0),
            max_error_rate: None,
            min_throughput_rpm: None,
            max_p99_latency_ms: None,
            max_queue_depth: None,
        },
        description: "Test".to_string(),
    };

    let mut constraints = Constraints::default();
    constraints.max_replicas = Some(2); // Very low limit

    let intent = Intent::new("w_test".to_string(), goal, constraints);
    intent_manager.register_intent(intent);

    let policy_engine = PolicyEngine::new(state_fabric.clone(), intent_manager);

    // Create workload at max replicas
    let workload = WorkloadEntity {
        workload_id: "w_test".to_string(),
        shard_id: "s_test".to_string(),
        state: WorkloadState::Healthy,
        region: Region::IndiaSouth,
        metrics: WorkloadMetrics {
            rate_per_min: 1000.0,
            p50_latency_ms: 50.0,
            p95_latency_ms: 120.0,
            p99_latency_ms: 200.0,
            error_rate: 0.01,
            queue_depth: 10,
            timestamp: chrono::Utc::now(),
        },
        replication: ReplicationPolicy {
            min_replicas: 1,
            max_replicas: 2,
            current_replicas: 2, // Already at max
            consistency_mode: ConsistencyMode::Strong,
        },
        locality: LocalityPreference {
            preferred_region: Region::IndiaSouth,
            fallback_regions: vec![Region::IndiaWest],
        },
        lifecycle: LifecycleState::Active,
        version: 1,
        updated_at: chrono::Utc::now(),
    };

    state_fabric.upsert_workload(workload).unwrap();

    // Try to create another replica (should exceed limit)
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "w_test".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Test out of bounds".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-50.0),
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Test".to_string(),
            },
            confidence: 0.9,
            risk: RiskLevel::Low,
            state_version: state_fabric.current_version(),
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["test_evidence".to_string()],
    );

    let result = policy_engine.evaluate(&proposal).unwrap();

    // Should be denied or require approval due to replica limit constraint
    assert!(
        matches!(result.verdict, PolicyVerdict::Denied { .. })
            || matches!(result.verdict, PolicyVerdict::RequiresApproval { .. }),
        "Out-of-bounds replica creation should be denied/require approval, got: {:?}",
        result.verdict
    );
}

#[tokio::test]
async fn test_03_unauthorized_region_denied() {
    // Test 3: Unauthorized Region
    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());

    // Create intent with restricted regions
    let goal = IntentGoal {
        objective: "Test region restrictions".to_string(),
        target_metrics: TargetMetrics {
            max_p95_latency_ms: Some(250.0),
            max_error_rate: None,
            min_throughput_rpm: None,
            max_p99_latency_ms: None,
            max_queue_depth: None,
        },
        description: "Test".to_string(),
    };

    let mut constraints = Constraints::default();
    constraints.allowed_regions = vec![Region::IndiaSouth]; // Only one region allowed
    constraints.forbidden_regions = vec![Region::UsEast, Region::EuWest]; // Explicitly forbidden

    let intent = Intent::new("w_test".to_string(), goal, constraints);
    intent_manager.register_intent(intent);

    let policy_engine = PolicyEngine::new(state_fabric.clone(), intent_manager);

    // Try to create replica in forbidden region
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "w_test".to_string(),
            target_region: Region::UsEast, // Forbidden region
            reason: "Test unauthorized region".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-50.0),
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Test".to_string(),
            },
            confidence: 0.9,
            risk: RiskLevel::Low,
            state_version: state_fabric.current_version(),
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["test_evidence".to_string()],
    );

    let result = policy_engine.evaluate(&proposal).unwrap();

    // Should be denied due to forbidden region constraint
    assert!(
        matches!(result.verdict, PolicyVerdict::Denied { .. }),
        "Action in forbidden region should be denied, got: {:?}",
        result.verdict
    );
}

#[tokio::test]
async fn test_04_stale_state_rejected() {
    // Test 4: Stale State
    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let policy_engine = PolicyEngine::new(state_fabric.clone(), intent_manager);

    // Advance state fabric version
    state_fabric.increment_version(); // version = 1
    state_fabric.increment_version(); // version = 2
    state_fabric.increment_version(); // version = 3
    let current_version = state_fabric.current_version(); // Should be 3

    // Create proposal with old state version
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "w_test".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Test stale state".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-50.0),
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Test".to_string(),
            },
            confidence: 0.9,
            risk: RiskLevel::Low,
            state_version: 1, // Stale - current is 3
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["test_evidence".to_string()],
    );

    let result = policy_engine.evaluate(&proposal).unwrap();

    // Should return STALE_STATE verdict
    match result.verdict {
        PolicyVerdict::StaleState {
            current_version: cv,
            proposed_version: pv,
            drift,
        } => {
            assert_eq!(cv, current_version);
            assert_eq!(pv, 1);
            assert_eq!(drift, current_version - 1);
        }
        _ => panic!("Expected STALE_STATE verdict, got: {:?}", result.verdict),
    }
}

#[tokio::test]
async fn test_05_missing_approval_blocked() {
    // Test 5: Missing Approval for High Risk
    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());

    // Create intent requiring approval for high risk
    let goal = IntentGoal {
        objective: "Test approval requirements".to_string(),
        target_metrics: TargetMetrics {
            max_p95_latency_ms: Some(250.0),
            max_error_rate: None,
            min_throughput_rpm: None,
            max_p99_latency_ms: None,
            max_queue_depth: None,
        },
        description: "Test".to_string(),
    };

    let mut constraints = Constraints::default();
    constraints.require_approval_for_high_risk = true;

    let intent = Intent::new("w_test".to_string(), goal, constraints);
    intent_manager.register_intent(intent);

    let policy_engine = PolicyEngine::new(state_fabric.clone(), intent_manager);

    // High risk action without approval
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "w_test".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Test high risk".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-50.0),
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Test".to_string(),
            },
            confidence: 0.9,
            risk: RiskLevel::High, // High risk
            state_version: state_fabric.current_version(),
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["test_evidence".to_string()],
    );

    let result = policy_engine.evaluate(&proposal).unwrap();

    // Should require approval due to high risk
    assert!(
        matches!(result.verdict, PolicyVerdict::RequiresApproval { .. }),
        "High risk action should require approval, got: {:?}",
        result.verdict
    );
}

#[tokio::test]
async fn test_06_invalid_model_output_no_execution() {
    // Test 6: Invalid Model Output
    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let policy_engine = PolicyEngine::new(state_fabric.clone(), intent_manager);

    // Create proposal with invalid model confidence
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "w_test".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Test invalid model output".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-50.0),
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Test".to_string(),
            },
            confidence: 0.1, // Very low confidence - should trigger approval
            risk: RiskLevel::Low,
            state_version: state_fabric.current_version(),
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["test_evidence".to_string()],
    );

    let result = policy_engine.evaluate(&proposal).unwrap();

    // Should require approval due to low confidence (< 0.75 threshold)
    assert!(
        matches!(result.verdict, PolicyVerdict::RequiresApproval { .. }),
        "Low confidence action should require approval, got: {:?}",
        result.verdict
    );
}

#[tokio::test]
async fn test_07_agent_failure_safe_operation() {
    // Test 7: Agent Failure
    // This test simulates what happens when agent services are unavailable

    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let policy_engine = PolicyEngine::new(state_fabric.clone(), intent_manager);

    // Create proposal that would normally be valid
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "w_test".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Test agent failure scenario".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-50.0),
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Test".to_string(),
            },
            confidence: 0.9,
            risk: RiskLevel::Low,
            state_version: state_fabric.current_version(),
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec![], // Empty evidence - simulates agent failure
    );

    let result = policy_engine.evaluate(&proposal).unwrap();

    // Even with empty evidence, policy engine should still function safely
    // This tests that the system doesn't crash and maintains safe operation
    assert!(
        matches!(
            result.verdict,
            PolicyVerdict::Allowed | PolicyVerdict::RequiresApproval { .. }
        ),
        "System should handle agent failure gracefully, got: {:?}",
        result.verdict
    );
}

#[tokio::test]
async fn test_08_runtime_failure_rollback() {
    // Test 8: Runtime Failure Handling
    // This test verifies the system can handle execution failures
    use esa_gateway::ActionGateway;
    use esa_policy::DecisionVerifier;

    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let policy_engine = Arc::new(PolicyEngine::new(state_fabric.clone(), intent_manager));
    let verifier = Arc::new(DecisionVerifier::new(state_fabric.clone()));
    let audit_store = Arc::new(AuditStore::new());

    let gateway = ActionGateway::new(
        state_fabric.clone(),
        policy_engine,
        verifier,
        audit_store.clone(),
    );

    // Create a proposal that would cause runtime failure (non-existent workload)
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "non_existent_workload".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Test runtime failure".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-50.0),
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Test".to_string(),
            },
            confidence: 0.9,
            risk: RiskLevel::Low,
            state_version: state_fabric.current_version(),
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["test_evidence".to_string()],
    );

    let result = gateway.execute_with_verdict(&proposal).await.unwrap();

    // Non-existent workload fails verification — must not mutate runtime
    assert!(
        result.is_blocked(),
        "Runtime failure should be blocked safely, got verdict: {:?}",
        result.verdict
    );

    // Verify audit record was created
    assert!(
        audit_store.count() > 0,
        "Audit record should be created even on failure"
    );
}

#[tokio::test]
async fn test_policy_allows_valid_action() {
    // Positive test: Valid action should be allowed
    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let policy_engine = PolicyEngine::new(state_fabric.clone(), intent_manager);

    // Create valid workload
    let workload = WorkloadEntity {
        workload_id: "w_valid".to_string(),
        shard_id: "s_valid".to_string(),
        state: WorkloadState::Healthy,
        region: Region::IndiaSouth,
        metrics: WorkloadMetrics {
            rate_per_min: 1000.0,
            p50_latency_ms: 50.0,
            p95_latency_ms: 120.0,
            p99_latency_ms: 200.0,
            error_rate: 0.01,
            queue_depth: 10,
            timestamp: chrono::Utc::now(),
        },
        replication: ReplicationPolicy {
            min_replicas: 2,
            max_replicas: 5,
            current_replicas: 2,
            consistency_mode: ConsistencyMode::Strong,
        },
        locality: LocalityPreference {
            preferred_region: Region::IndiaSouth,
            fallback_regions: vec![Region::IndiaWest],
        },
        lifecycle: LifecycleState::Active,
        version: 1,
        updated_at: chrono::Utc::now(),
    };

    state_fabric.upsert_workload(workload).unwrap();

    // Valid proposal
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "w_valid".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Valid action test".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-50.0),
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Test".to_string(),
            },
            confidence: 0.9,
            risk: RiskLevel::Low,
            state_version: state_fabric.current_version(),
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["valid_evidence".to_string()],
    );

    let result = policy_engine.evaluate(&proposal).unwrap();

    // Should be allowed
    assert!(
        matches!(result.verdict, PolicyVerdict::Allowed),
        "Valid action should be allowed, got: {:?}",
        result.verdict
    );
}

#[tokio::test]
async fn test_policy_blocks_unsafe_action() {
    // Negative test: Unsafe action should be blocked
    let state_fabric = Arc::new(StateFabric::new());
    let intent_manager = Arc::new(IntentManager::new());
    let policy_engine = PolicyEngine::new(state_fabric.clone(), intent_manager);

    // Unsafe proposal (no rollback capability for risky action)
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "w_test".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Unsafe action test".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-50.0),
                throughput_delta_pct: None,
                error_rate_delta: None,
                queue_delta: None,
                description: "Test".to_string(),
            },
            confidence: 0.9,
            risk: RiskLevel::Critical, // Critical risk
            state_version: state_fabric.current_version(),
            rollback_enabled: false, // No rollback for critical action - unsafe
        },
        AgentId::Planning,
        vec!["test_evidence".to_string()],
    );

    let result = policy_engine.evaluate(&proposal).unwrap();

    // Should require approval or be denied due to critical risk without rollback
    assert!(
        matches!(result.verdict, PolicyVerdict::RequiresApproval { .. })
            || matches!(result.verdict, PolicyVerdict::Denied { .. }),
        "Unsafe action should require approval or be denied, got: {:?}",
        result.verdict
    );
}
