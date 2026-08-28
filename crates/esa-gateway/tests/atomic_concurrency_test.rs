use chrono::Utc;
use esa_core::{
    ActionProposal, ActionType, AgentId, ConsistencyMode, ExpectedEffect, IntentManager,
    LocalityPreference, Region, ReplicationPolicy, RiskLevel, WorkloadEntity, WorkloadMetrics,
    WorkloadState,
};
use esa_gateway::ActionGateway;
use esa_policy::{DecisionVerifier, PolicyEngine, PolicyVerdict};
use esa_state::StateFabric;
use std::sync::Arc;
use tokio::task::JoinSet;

#[tokio::test]
async fn test_atomic_occ_concurrent_state_version_rejection() {
    let fabric = Arc::new(StateFabric::new());
    let intent_mgr = Arc::new(IntentManager::new());
    let policy = Arc::new(PolicyEngine::new(fabric.clone(), intent_mgr.clone()));
    let verifier = Arc::new(DecisionVerifier::new(fabric.clone()));
    let audit_store = Arc::new(esa_core::AuditStore::new());

    let gateway = Arc::new(ActionGateway::new(
        fabric.clone(),
        policy,
        verifier,
        audit_store,
    ));

    // Seed healthy workload at state version 0 -> after upsert it becomes version 1
    let workload = WorkloadEntity {
        workload_id: "pay_workload_01".to_string(),
        shard_id: "shard_01".to_string(),
        state: WorkloadState::Healthy,
        region: Region::IndiaSouth,
        metrics: WorkloadMetrics {
            rate_per_min: 1200.0,
            p50_latency_ms: 60.0,
            p95_latency_ms: 180.0,
            p99_latency_ms: 240.0,
            error_rate: 0.005,
            queue_depth: 20,
            timestamp: Utc::now(),
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
        lifecycle: esa_core::LifecycleState::Active,
        version: 1,
        updated_at: Utc::now(),
    };

    fabric.upsert_workload(workload).unwrap();
    let initial_version = fabric.current_version();
    assert_eq!(initial_version, 1);

    // Spawn 10 concurrent tasks all attempting to execute an action based on reading initial_version (1)
    let num_tasks = 10;
    let mut set = JoinSet::new();

    for i in 0..num_tasks {
        let gw = gateway.clone();
        let prop = ActionProposal::new(
            ActionType::CreateReplica {
                workload_id: "pay_workload_01".to_string(),
                target_region: Region::IndiaSouth,
                reason: format!("Concurrent scale request {}", i),
                expected_effect: ExpectedEffect {
                    latency_delta_ms: Some(-30.0),
                    throughput_delta_pct: Some(15.0),
                    error_rate_delta: None,
                    queue_delta: Some(-10),
                    description: "Concurrent replica".to_string(),
                },
                confidence: 0.9,
                risk: RiskLevel::Low,
                state_version: initial_version,
                rollback_enabled: true,
            },
            AgentId::Planning,
            vec![format!("evidence_concurrent_{}", i)],
        );

        set.spawn(async move { gw.execute_with_verdict(&prop).await });
    }

    let mut allowed_count = 0;
    let mut stale_rejected_count = 0;

    while let Some(res) = set.join_next().await {
        let result = res.unwrap().unwrap();
        match result.verdict {
            PolicyVerdict::Allowed => {
                allowed_count += 1;
            }
            PolicyVerdict::StaleState { .. } => {
                stale_rejected_count += 1;
            }
            other => {
                panic!("Unexpected policy verdict: {:?}", other);
            }
        }
    }

    // Exactly one concurrent worker should succeed on version 1, and the remaining 9 MUST be rejected as stale state!
    assert_eq!(allowed_count, 1, "Exactly one concurrent worker should execute on version 1");
    assert_eq!(stale_rejected_count, num_tasks - 1, "All other concurrent workers must be rejected as stale state");
}
