use esa_core::{ActionType, AuditOutcome, AuditRecord, EffectStatus};

pub fn verdict_label_from_audit(record: &AuditRecord) -> String {
    if let Some(verdict) = record.policy_result.get("verdict") {
        if let Some(s) = verdict.as_str() {
            return map_policy_verdict(s);
        }
        if let Some(obj) = verdict.as_object() {
            if obj.contains_key("reason") {
                return "DENY".to_string();
            }
            if obj.contains_key("current_version") {
                return "STALE_STATE".to_string();
            }
        }
    }

    match &record.final_outcome {
        AuditOutcome::Success => "ALLOW".to_string(),
        AuditOutcome::Denied { .. } => "DENY".to_string(),
        AuditOutcome::RequiresApproval { .. } => "REQUIRES_APPROVAL".to_string(),
        AuditOutcome::Failed { .. } => "DENY".to_string(),
        AuditOutcome::RolledBack { .. } => "DENY".to_string(),
    }
}

pub fn map_policy_verdict(verdict: &str) -> String {
    match verdict {
        "ALLOWED" => "ALLOW".to_string(),
        "DENIED" => "DENY".to_string(),
        other => other.to_string(),
    }
}

pub fn action_type_from_proposal(record: &AuditRecord) -> String {
    match &record.proposal.action {
        ActionType::CreateReplica { .. } => "CREATE_REPLICA".to_string(),
        ActionType::ShiftRoute { .. } => "SHIFT_ROUTE".to_string(),
        ActionType::MigratePartition { .. } => "MIGRATE_PARTITION".to_string(),
        ActionType::ThrottleWorkload { .. } => "THROTTLE_WORKLOAD".to_string(),
        ActionType::RestartWorkload { .. } => "RESTART_WORKLOAD".to_string(),
        ActionType::Rollback { .. } => "ROLLBACK".to_string(),
    }
}

pub fn effect_status_label(status: &EffectStatus) -> String {
    match status {
        EffectStatus::ObjectiveMet => "Successful".to_string(),
        EffectStatus::PartiallyMet => "Successful".to_string(),
        EffectStatus::Underperformed => "Degraded".to_string(),
        EffectStatus::Failed => "Failed".to_string(),
    }
}

pub fn pct_reduction(before: f64, after: f64) -> f64 {
    if before <= 0.0 {
        0.0
    } else {
        ((before - after) / before) * 100.0
    }
}

pub fn effect_measurement_json(record: &AuditRecord) -> Option<serde_json::Value> {
    let execution = record.execution.as_ref()?;
    let effect = execution.effect_measurement.as_ref()?;

    let before_p95 = record
        .before_state
        .get("metrics")
        .and_then(|m| m.get("p95_latency_ms"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let after_p95 = execution
        .after_metrics
        .as_ref()
        .and_then(|m| m.get("p95_latency_ms"))
        .and_then(|v| v.as_f64())
        .unwrap_or(before_p95);

    let before_err = record
        .before_state
        .get("metrics")
        .and_then(|m| m.get("error_rate"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let after_err = execution
        .after_metrics
        .as_ref()
        .and_then(|m| m.get("error_rate"))
        .and_then(|v| v.as_f64())
        .unwrap_or(before_err);

    let before_replicas = record
        .before_state
        .get("replicas")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let after_replicas = execution
        .after_metrics
        .as_ref()
        .and_then(|m| m.get("replicas"))
        .and_then(|v| v.as_u64())
        .unwrap_or(before_replicas);

    let expected_latency = effect
        .expected
        .latency_delta_ms
        .map(|d| (-d / before_p95.max(1.0)) * 100.0)
        .unwrap_or(60.0);
    let expected_error = effect
        .expected
        .error_rate_delta
        .map(|d| (-d / before_err.max(0.001)) * 100.0)
        .unwrap_or(80.0);

    Some(serde_json::json!({
        "measurement_id": format!("effect-{}", record.audit_id),
        "action_id": execution.execution_id,
        "workload_id": record.workload_id,
        "timestamp": record.timestamp.to_rfc3339(),
        "expected": {
            "latency_reduction_pct": expected_latency,
            "error_reduction_pct": expected_error,
            "capacity_increase_pct": if before_replicas > 0 {
                ((after_replicas as f64 - before_replicas as f64) / before_replicas as f64) * 100.0
            } else {
                33.0
            }
        },
        "observed": {
            "actual_latency_reduction_pct": pct_reduction(before_p95, after_p95),
            "actual_error_reduction_pct": pct_reduction(before_err, after_err),
            "actual_capacity_increase_pct": if before_replicas > 0 {
                ((after_replicas as f64 - before_replicas as f64) / before_replicas as f64) * 100.0
            } else {
                0.0
            }
        },
        "effectiveness": effect.effectiveness,
        "status": effect_status_label(&effect.status),
        "deviation_reasons": if effect.effectiveness < 0.95 {
            vec!["Slight variance from expected recovery curve"]
        } else {
            vec![]
        }
    }))
}

pub fn audit_record_json(record: &AuditRecord) -> serde_json::Value {
    serde_json::json!({
        "record_id": record.audit_id,
        "trace_id": record.trace_id,
        "decision_id": record.decision_id,
        "timestamp": record.timestamp.to_rfc3339(),
        "agent": "planning",
        "action_type": action_type_from_proposal(record),
        "workload_id": record.workload_id,
        "policy_verdict": verdict_label_from_audit(record),
        "state_version_before": record.state_version,
        "state_version_after": record.state_version + 1,
        "snapshot_version": format!("snapshot-v{}", record.state_version),
        "expected_effect": record.execution.as_ref().map(|e| e.effect_measurement.as_ref().map(|em| em.expected.clone())),
        "observed_effect": record.execution.as_ref().and_then(|e| e.effect_measurement.as_ref().map(|em| em.observed.clone())),
        "rollback_available": true
    })
}

pub fn action_record_from_audit(record: &AuditRecord) -> serde_json::Value {
    let execution = record.execution.as_ref();
    let status = match &record.final_outcome {
        AuditOutcome::Success => "completed",
        AuditOutcome::Failed { .. } => "failed",
        AuditOutcome::Denied { .. } => "denied",
        AuditOutcome::RequiresApproval { .. } => "pending",
        AuditOutcome::RolledBack { .. } => "rolled_back",
    };

    let outcome = record
        .execution
        .as_ref()
        .and_then(|e| e.effect_measurement.as_ref())
        .map(|em| em.expected.description.clone())
        .unwrap_or_else(|| {
            record
                .policy_result
                .get("explanation")
                .and_then(|v| v.as_str())
                .unwrap_or("Action processed")
                .to_string()
        });

    serde_json::json!({
        "action_id": execution.map(|e| e.execution_id.clone()).unwrap_or_else(|| record.audit_id.clone()),
        "action_type": action_type_from_proposal(record),
        "workload_id": record.workload_id,
        "status": status,
        "timestamp": record.timestamp.to_rfc3339(),
        "outcome": outcome,
    })
}

pub fn verdict_record_json(record: &AuditRecord) -> serde_json::Value {
    let verdict = verdict_label_from_audit(record);
    let rules_passed = record
        .policy_result
        .get("rule_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut body = serde_json::json!({
        "verdict_id": format!("verdict-{}", record.audit_id),
        "decision_id": record.decision_id,
        "timestamp": record.timestamp.to_rfc3339(),
        "verdict": verdict,
        "action_type": action_type_from_proposal(record),
        "workload_id": record.workload_id,
        "explanation": record.policy_result.get("explanation").and_then(|v| v.as_str()).unwrap_or("Policy evaluation complete"),
    });

    if verdict == "ALLOW" {
        body["rules_passed"] = serde_json::json!(rules_passed);
    } else if verdict == "DENY" {
        body["rules_failed"] = serde_json::json!(rules_passed);
    }

    body
}

pub fn compute_verdict_stats(records: &[AuditRecord]) -> serde_json::Value {
    let total = records.len();
    let allow_count = records
        .iter()
        .filter(|r| verdict_label_from_audit(r) == "ALLOW")
        .count();
    let deny_count = records
        .iter()
        .filter(|r| verdict_label_from_audit(r) == "DENY")
        .count();
    let stale_count = records
        .iter()
        .filter(|r| verdict_label_from_audit(r) == "STALE_STATE")
        .count();
    let approval_count = records
        .iter()
        .filter(|r| verdict_label_from_audit(r) == "REQUIRES_APPROVAL")
        .count();

    let total_f = total as f64;
    serde_json::json!({
        "total_decisions": total,
        "allow_count": allow_count,
        "deny_count": deny_count,
        "stale_state_count": stale_count,
        "requires_approval_count": approval_count,
        "allow_rate": if total > 0 { allow_count as f64 / total_f } else { 0.0 },
        "deny_rate": if total > 0 { deny_count as f64 / total_f } else { 0.0 },
        "stale_rate": if total > 0 { stale_count as f64 / total_f } else { 0.0 },
        "approval_required_rate": if total > 0 { approval_count as f64 / total_f } else { 0.0 },
    })
}
