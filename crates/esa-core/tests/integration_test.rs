use esa_core::*;

#[test]
fn test_workload_entity_creation() {
    let workload = WorkloadEntity {
        workload_id: "w_test_001".to_string(),
        shard_id: "s_test_001".to_string(),
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

    assert_eq!(workload.workload_id, "w_test_001");
    assert_eq!(workload.state, WorkloadState::Healthy);
    assert_eq!(workload.replication.current_replicas, 2);
}

#[test]
fn test_action_proposal_creation() {
    let action = ActionType::CreateReplica {
        workload_id: "w_001".to_string(),
        target_region: Region::IndiaSouth,
        reason: "High latency detected".to_string(),
        expected_effect: ExpectedEffect {
            latency_delta_ms: Some(-80.0),
            throughput_delta_pct: Some(30.0),
            error_rate_delta: None,
            queue_delta: None,
            description: "Reduce latency by adding replica".to_string(),
        },
        confidence: 0.9,
        risk: RiskLevel::Low,
        state_version: 1,
        rollback_enabled: true,
    };

    let proposal = ActionProposal::new(action, AgentId::Planning, vec!["evidence_1".to_string()]);

    assert_eq!(proposal.proposed_by, AgentId::Planning);
    assert_eq!(proposal.evidence_refs.len(), 1);
}

#[test]
fn test_payment_event_creation() {
    let event = PaymentEvent::new_synthetic(
        PaymentEventType::PaymentAuthorized,
        Region::IndiaSouth,
        PaymentMethodClass::Upi,
        true,
    );

    assert_eq!(event.event_type, PaymentEventType::PaymentAuthorized);
    assert_eq!(event.region, Region::IndiaSouth);
    assert!(event.success);
}

#[test]
fn test_region_serialization() {
    let region = Region::IndiaSouth;
    assert_eq!(region.as_str(), "IN-SOUTH");
}

#[test]
fn test_action_execution_lifecycle() {
    let action = ActionType::CreateReplica {
        workload_id: "w_001".to_string(),
        target_region: Region::IndiaSouth,
        reason: "Test".to_string(),
        expected_effect: ExpectedEffect {
            latency_delta_ms: Some(-50.0),
            throughput_delta_pct: None,
            error_rate_delta: None,
            queue_delta: None,
            description: "Test effect".to_string(),
        },
        confidence: 0.85,
        risk: RiskLevel::Low,
        state_version: 1,
        rollback_enabled: true,
    };

    let proposal = ActionProposal::new(action, AgentId::Planning, vec![]);
    let before_metrics = serde_json::json!({ "p95_latency_ms": 250.0 });

    let mut execution = ActionExecution::new(&proposal, before_metrics);
    assert!(execution.outcome.is_none());

    let after_metrics = serde_json::json!({ "p95_latency_ms": 120.0 });
    execution = execution.complete(ActionOutcome::Success, after_metrics);

    assert_eq!(execution.outcome, Some(ActionOutcome::Success));
    assert!(execution.completed_at.is_some());
}
