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
async fn rollback_restores_snapshot_state() {
    let fabric = Arc::new(StateFabric::new());
    fabric
        .upsert_workload(degraded_workload("payment-upi-india-south"))
        .unwrap();

    let snapshot_version = fabric.create_snapshot().unwrap().version;

    let mut workload = fabric.get_workload("payment-upi-india-south").unwrap();
    workload.metrics.p95_latency_ms = 500.0;
    workload.replication.current_replicas = 5;
    workload.state = WorkloadState::Degraded;
    fabric.upsert_workload(workload).unwrap();

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

    let proposal = ActionProposal::new(
        ActionType::Rollback {
            original_action_id: "test-decision".to_string(),
            reason: "Restore healthy baseline".to_string(),
            target_snapshot: snapshot_version.to_string(),
        },
        AgentId::Safety,
        vec!["snapshot_restore".to_string()],
    );

    let result = gateway.execute_with_verdict(&proposal).await.unwrap();
    assert!(result.is_success());

    let restored = fabric.get_workload("payment-upi-india-south").unwrap();
    assert_eq!(restored.replication.current_replicas, 2);
    assert!(restored.metrics.p95_latency_ms < 400.0);
}
