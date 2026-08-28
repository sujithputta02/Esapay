use esa_core::{
    ActionProposal, ActionType, AgentId, AuditOutcome, AuditRecord, AuditStore, ExpectedEffect,
    Region, RiskLevel,
};
use std::sync::Arc;

#[test]
fn test_tamper_detection_and_integrity_verification() {
    let store = Arc::new(AuditStore::new());

    // 1. Create a chain of 4 decisions
    for i in 1..=4 {
        let proposal = ActionProposal::new(
            ActionType::CreateReplica {
                workload_id: format!("w_{}", i),
                target_region: Region::IndiaSouth,
                reason: format!("Legitimate decision {}", i),
                expected_effect: ExpectedEffect {
                    latency_delta_ms: Some(-50.0),
                    throughput_delta_pct: None,
                    error_rate_delta: None,
                    queue_delta: None,
                    description: "Scaling".to_string(),
                },
                confidence: 0.9,
                risk: RiskLevel::Low,
                state_version: i,
                rollback_enabled: true,
            },
            AgentId::Planning,
            vec!["telemetry_data".to_string()],
        );

        let record = AuditRecord::new(
            format!("trace_{}", i),
            format!("decision_{}", i),
            format!("w_{}", i),
            i,
            proposal,
            serde_json::json!({ "p95": 200.0 }),
        )
        .with_outcome(AuditOutcome::Success);

        store.append(record);
    }

    // 2. Untampered chain verifies 100% valid
    let initial_verification = store.verify_chain();
    assert!(initial_verification.is_valid);
    assert_eq!(initial_verification.total_records, 4);
    assert!(initial_verification.violations.is_empty());

    // 3. Attempt adversary modification: get record #2 and tamper with its payload
    let recent = store.list_recent(4);
    let record_2 = recent.iter().find(|r| r.state_version == 2).unwrap();
    let audit_id_2 = record_2.audit_id.clone();

    // Directly alter the stored record in DashMap to simulate malicious database edit
    let mut tampered_record = store.get(&audit_id_2).unwrap();
    tampered_record.final_outcome = AuditOutcome::Denied {
        reason: "Adversary maliciously altered outcome".to_string(),
    };

    // Insert tampered record directly into store without re-hashing
    store.update_raw_for_testing(tampered_record);

    // The store should detect the payload modification because current_hash != calculate_hash()
    let tampered_verification = store.verify_chain();
    assert!(
        !tampered_verification.is_valid,
        "Tampered chain must fail cryptographic verification"
    );
    assert!(!tampered_verification.violations.is_empty());
    assert!(tampered_verification
        .violations
        .iter()
        .any(|v| v.contains("Payload integrity violation")));
}
