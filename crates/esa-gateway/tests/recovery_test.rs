use esa_core::*;
use esa_gateway::ActionGateway;
use esa_policy::{DecisionVerifier, PolicyEngine};
use esa_state::StateFabric;
use std::sync::Arc;

fn degraded_workload(id: &str) -> WorkloadEntity {
    WorkloadEntity {
        workload_id: id.to_string(),
        shard_id: "shard-001".to_string(),
        state: WorkloadState::Degraded,
        region: Region::IndiaSouth,
        metrics: WorkloadMetrics {
            rate_per_min: 3600.0,
            p50_latency_ms: 120.0,
            p95_latency_ms: 384.0,
            p99_latency_ms: 400.0,
            error_rate: 0.067,
            queue_depth: 750,
            timestamp: chrono::Utc::now(),
        },
        replication: ReplicationPolicy {
            min_replicas: 2,
            max_replicas: 10,
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
    }
}

#[tokio::test]
async fn create_replica_mutates_state_and_restores_health() {
    let fabric = Arc::new(StateFabric::new());
    fabric
        .upsert_workload(degraded_workload("payment-upi-india-south"))
        .unwrap();

    let intent_manager = Arc::new(IntentManager::new());
    let policy_engine = Arc::new(PolicyEngine::new(
        Arc::clone(&fabric),
        Arc::clone(&intent_manager),
    ));
    let decision_verifier = Arc::new(DecisionVerifier::new(Arc::clone(&fabric)));
    let audit_store = Arc::new(AuditStore::new());

    let gateway = ActionGateway::new(
        Arc::clone(&fabric),
        policy_engine,
        decision_verifier,
        audit_store,
    );

    let state_version = fabric.current_version();
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "payment-upi-india-south".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Capacity issue".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-80.0),
                throughput_delta_pct: Some(30.0),
                error_rate_delta: Some(-0.02),
                queue_delta: Some(-500),
                description: "Distribute load across additional replica".to_string(),
            },
            confidence: 0.85,
            risk: RiskLevel::Low,
            state_version,
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["p95_latency_ms".to_string()],
    );

    let result = gateway.execute_with_verdict(&proposal).await.unwrap();
    assert!(result.is_success());

    let workload = fabric.get_workload("payment-upi-india-south").unwrap();
    assert_eq!(workload.state, WorkloadState::Healthy);
    assert!(workload.metrics.p95_latency_ms < 250.0);
    assert!(workload.metrics.queue_depth < 1000);
    assert_eq!(workload.replication.current_replicas, 3);
}
