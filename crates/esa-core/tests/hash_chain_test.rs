use esa_core::{
    ActionOutcome, ActionProposal, ActionType, AgentId, AuditOutcome, AuditRecord, AuditStore,
    ExpectedEffect, Region, RiskLevel, GENESIS_HASH,
};
use std::sync::Arc;

#[test]
fn test_tamper_evident_sha256_hash_chain() {
    let store = Arc::new(AuditStore::new());

    // Initially empty chain is valid
    let v0 = store.verify_chain();
    assert!(v0.is_valid);
    assert_eq!(v0.total_records, 0);
    assert_eq!(v0.latest_hash, GENESIS_HASH);

    // Append 5 sequential audit records
    let mut previous_hash = GENESIS_HASH.to_string();
    for i in 1..=5 {
        let proposal = ActionProposal::new(
            ActionType::CreateReplica {
                workload_id: format!("w_{:03}", i),
                target_region: Region::IndiaSouth,
                reason: format!("Scale test {}", i),
                expected_effect: ExpectedEffect {
                    latency_delta_ms: Some(-40.0),
                    throughput_delta_pct: Some(20.0),
                    error_rate_delta: Some(-0.01),
                    queue_delta: Some(-200),
                    description: "Scaling effect".to_string(),
                },
                confidence: 0.95,
                risk: RiskLevel::Low,
                state_version: i as u64,
                rollback_enabled: true,
            },
            AgentId::Planning,
            vec![format!("metric_evidence_{}", i)],
        );

        let record = AuditRecord::new(
            format!("trace_{:03}", i),
            format!("decision_{:03}", i),
            format!("w_{:03}", i),
            i as u64,
            proposal,
            serde_json::json!({ "p95": 250.0 }),
        )
        .with_outcome(AuditOutcome::Success);

        let appended = store.append(record);
        assert_eq!(appended.previous_hash, previous_hash);
        assert_ne!(appended.current_hash, previous_hash);
        assert_eq!(appended.current_hash.len(), 64); // SHA-256 hex string

        previous_hash = appended.current_hash;
    }

    // Verify intact chain
    let verification = store.verify_chain();
    assert!(verification.is_valid);
    assert_eq!(verification.total_records, 5);
    assert_eq!(verification.latest_hash, previous_hash);
    assert!(verification.violations.is_empty());
}
