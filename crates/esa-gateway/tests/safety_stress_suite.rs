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

#[tokio::test]
async fn test_adversarial_safety_stress_suite_high_iterations() {
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

    // Seed workload
    let workload = WorkloadEntity {
        workload_id: "stress_workload_01".to_string(),
        shard_id: "shard_stress".to_string(),
        state: WorkloadState::Healthy,
        region: Region::IndiaSouth,
        metrics: WorkloadMetrics {
            rate_per_min: 1000.0,
            p50_latency_ms: 50.0,
            p95_latency_ms: 120.0,
            p99_latency_ms: 180.0,
            error_rate: 0.005,
            queue_depth: 10,
            timestamp: Utc::now(),
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
        lifecycle: esa_core::LifecycleState::Active,
        version: 1,
        updated_at: Utc::now(),
    };
    fabric.upsert_workload(workload).unwrap();

    let mut total_unsafe_attempts = 0u32;
    let mut total_blocked = 0u32;
    let mut total_unsafe_mutations = 0u32;

    // 1. Stress 100 Out-of-Bounds Replicas Attempts (> max_replicas = 5)
    // First set replicas to max
    if let Some(mut w) = fabric.get_workload("stress_workload_01") {
        w.replication.current_replicas = 5;
        fabric.upsert_workload(w).unwrap();
    }

    for i in 0..100 {
        total_unsafe_attempts += 1;
        let proposal = ActionProposal::new(
            ActionType::CreateReplica {
                workload_id: "stress_workload_01".to_string(),
                target_region: Region::IndiaSouth,
                reason: format!("Excess scaling attempt {}", i),
                expected_effect: ExpectedEffect {
                    latency_delta_ms: Some(-20.0),
                    throughput_delta_pct: None,
                    error_rate_delta: None,
                    queue_delta: None,
                    description: "Illegal excess replica".to_string(),
                },
                confidence: 0.9,
                risk: RiskLevel::Low,
                state_version: fabric.current_version(),
                rollback_enabled: true,
            },
            AgentId::Planning,
            vec!["adversarial_excess".to_string()],
        );

        let res = gateway.execute_with_verdict(&proposal).await.unwrap();
        if res.is_blocked() || !res.is_success() {
            total_blocked += 1;
        } else {
            total_unsafe_mutations += 1;
        }
    }

    // 2. Stress 100 Stale State OCC Versions
    if let Some(mut w) = fabric.get_workload("stress_workload_01") {
        w.replication.current_replicas = 2;
        w.version = 50;
        fabric.upsert_workload(w).unwrap();
    }
    let current_ver = fabric.get_workload("stress_workload_01").map(|w| w.version).unwrap_or(50);
    for i in 0..100 {
        total_unsafe_attempts += 1;
        let stale_version = if current_ver > 10 { current_ver - 5 } else { 0 };
        let proposal = ActionProposal::new(
            ActionType::CreateReplica {
                workload_id: "stress_workload_01".to_string(),
                target_region: Region::IndiaSouth,
                reason: format!("Stale state replay attempt {}", i),
                expected_effect: ExpectedEffect {
                    latency_delta_ms: Some(-20.0),
                    throughput_delta_pct: None,
                    error_rate_delta: None,
                    queue_delta: None,
                    description: "Stale replay".to_string(),
                },
                confidence: 0.9,
                risk: RiskLevel::Low,
                state_version: stale_version,
                rollback_enabled: true,
            },
            AgentId::Planning,
            vec!["adversarial_stale".to_string()],
        );

        let res = gateway.execute_with_verdict(&proposal).await.unwrap();
        if matches!(res.verdict, PolicyVerdict::StaleState { .. }) || res.is_blocked() {
            total_blocked += 1;
        } else {
            total_unsafe_mutations += 1;
        }
    }

    // 3. Stress 50 Snapshot Rollbacks
    let mut rollback_successes = 0u32;
    for i in 0..50 {
        let snap_version = fabric.create_snapshot().unwrap().version;
        // Mutate state temporarily
        if let Some(mut w) = fabric.get_workload("stress_workload_01") {
            w.metrics.p95_latency_ms = 400.0;
            w.state = WorkloadState::Degraded;
            fabric.upsert_workload(w).unwrap();
        }

        let rollback = ActionProposal::new(
            ActionType::Rollback {
                original_action_id: format!("action_fail_{}", i),
                reason: "Automated test rollback".to_string(),
                target_snapshot: snap_version.to_string(),
            },
            AgentId::Planning,
            vec!["rollback_proof".to_string()],
        );

        let res = gateway.execute_with_verdict(&rollback).await.unwrap();
        if res.is_success() {
            rollback_successes += 1;
        }
    }

    // Assert zero unsafe mutations and 100% block & rollback success
    assert_eq!(total_unsafe_mutations, 0, "Zero unsafe mutations allowed");
    assert_eq!(total_blocked, total_unsafe_attempts, "All 200 adversarial actions must be blocked");
    assert_eq!(rollback_successes, 50, "All 50 rollbacks must successfully restore state");

    // Cryptographic audit chain must remain 100% intact
    let chain_ver = audit_store.verify_chain();
    assert!(chain_ver.is_valid);
    assert_eq!(chain_ver.violations.len(), 0);
}
