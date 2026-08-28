use chrono::Utc;
use esa_core::{
    ActionProposal, ActionType, AgentId, ConsistencyMode, ExpectedEffect, IntentManager,
    LocalityPreference, Region, ReplicationPolicy, RiskLevel, WorkloadEntity, WorkloadMetrics,
    WorkloadState,
};
use esa_gateway::ActionGateway;
use esa_policy::{DecisionVerifier, PolicyEngine};
use esa_state::StateFabric;
use std::sync::Arc;

#[tokio::test]
async fn test_live_kubernetes_mutation_and_rollback() {
    // Check if kubectl is accessible
    let kubectl_check = std::process::Command::new("kubectl")
        .args(["get", "namespace", "esa-workloads"])
        .output();

    if kubectl_check.is_err() || !kubectl_check.unwrap().status.success() {
        println!("Skipping Kubernetes test: cluster or namespace not reachable");
        return;
    }

    std::env::set_var("KUBERNETES_ENABLED", "true");

    let fabric = Arc::new(StateFabric::new());
    let intent_mgr = Arc::new(IntentManager::new());
    let policy = Arc::new(PolicyEngine::new(fabric.clone(), intent_mgr.clone()));
    let verifier = Arc::new(DecisionVerifier::new(fabric.clone()));
    let audit_store = Arc::new(esa_core::AuditStore::new());

    let gateway = Arc::new(ActionGateway::new(
        fabric.clone(),
        policy,
        verifier,
        audit_store.clone(),
    ));

    // Seed payment-processor matching the k8s deployment
    let workload = WorkloadEntity {
        workload_id: "payment-processor".to_string(),
        shard_id: "shard_k8s".to_string(),
        state: WorkloadState::Degraded,
        region: Region::IndiaSouth,
        metrics: WorkloadMetrics {
            rate_per_min: 3500.0,
            p50_latency_ms: 80.0,
            p95_latency_ms: 320.0,
            p99_latency_ms: 450.0,
            error_rate: 0.04,
            queue_depth: 1200,
            timestamp: Utc::now(),
        },
        replication: ReplicationPolicy {
            min_replicas: 2,
            max_replicas: 6,
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

    // 1. Propose scaling from 2 -> 3 replicas
    let proposal = ActionProposal::new(
        ActionType::CreateReplica {
            workload_id: "payment-processor".to_string(),
            target_region: Region::IndiaSouth,
            reason: "Scale Kubernetes deployment under load".to_string(),
            expected_effect: ExpectedEffect {
                latency_delta_ms: Some(-80.0),
                throughput_delta_pct: Some(30.0),
                error_rate_delta: Some(-0.02),
                queue_delta: Some(-500),
                description: "Live k8s pod scaling".to_string(),
            },
            confidence: 0.95,
            risk: RiskLevel::Low,
            state_version: fabric.current_version(),
            rollback_enabled: true,
        },
        AgentId::Planning,
        vec!["k8s_load_condition".to_string()],
    );

    let result = gateway.execute_with_verdict(&proposal).await.unwrap();
    assert!(result.is_success(), "Gateway must execute k8s scaling");

    // 2. Verify state fabric was updated to 3 replicas
    let updated = fabric.get_workload("payment-processor").unwrap();
    assert_eq!(updated.replication.current_replicas, 3);

    // 3. Verify that kubectl deployment was actually scaled in the cluster
    let k8s_out = std::process::Command::new("kubectl")
        .args([
            "get",
            "deployment",
            "payment-processor",
            "-n",
            "esa-workloads",
            "-o",
            "jsonpath={.spec.replicas}",
        ])
        .output()
        .unwrap();
    let replicas_str = String::from_utf8_lossy(&k8s_out.stdout);
    println!("Live Kubernetes deployment spec.replicas = {}", replicas_str);
    assert_eq!(replicas_str.trim(), "3", "Kubernetes cluster must reflect 3 replicas");

    // 4. Test rollback scaling back to 2 replicas
    let snapshot_version = 1;
    let rollback_proposal = ActionProposal::new(
        ActionType::Rollback {
            original_action_id: result.decision_id.clone(),
            reason: "Live k8s rollback verification".to_string(),
            target_snapshot: snapshot_version.to_string(),
        },
        AgentId::Planning,
        vec!["rollback_proof".to_string()],
    );

    let rollback_result = gateway.execute_with_verdict(&rollback_proposal).await.unwrap();
    assert!(rollback_result.is_success());

    let rolled_back_out = std::process::Command::new("kubectl")
        .args([
            "get",
            "deployment",
            "payment-processor",
            "-n",
            "esa-workloads",
            "-o",
            "jsonpath={.spec.replicas}",
        ])
        .output()
        .unwrap();
    let rolled_back_str = String::from_utf8_lossy(&rolled_back_out.stdout);
    println!("Live Kubernetes deployment after rollback = {}", rolled_back_str);
    assert_eq!(rolled_back_str.trim(), "2", "Kubernetes cluster must rollback to 2 replicas");
}
