use esa_core::{
    ActionExecution, ActionOutcome, ActionProposal, ActionType, EsaResult, Region,
    WorkloadMetrics, WorkloadState,
};
use esa_core::{
    AuditRecord, AuditStore, AuditOutcome, ExpectedEffect, ObservedEffect, EffectMeasurement,
};
use esa_policy::{PolicyEngine, PolicyVerdict, DecisionVerifier};
use esa_state::StateFabric;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayResult {
    pub verdict: PolicyVerdict,
    pub execution: Option<ActionExecution>,
    pub decision_id: String,
    pub trace_id: String,
    pub processing_time_ms: u64,
    pub explanation: String,
}

impl GatewayResult {
    pub fn new(
        verdict: PolicyVerdict,
        decision_id: String,
        trace_id: String,
        processing_time_ms: u64,
        explanation: String,
    ) -> Self {
        Self {
            verdict,
            execution: None,
            decision_id,
            trace_id,
            processing_time_ms,
            explanation,
        }
    }

    pub fn with_execution(mut self, execution: ActionExecution) -> Self {
        self.execution = Some(execution);
        self
    }

    pub fn is_success(&self) -> bool {
        matches!(self.verdict, PolicyVerdict::Allowed)
            && self
                .execution
                .as_ref()
                .map(|e| matches!(e.outcome, Some(ActionOutcome::Success)))
                .unwrap_or(false)
    }

    pub fn is_blocked(&self) -> bool {
        matches!(
            self.verdict,
            PolicyVerdict::Denied { .. } | PolicyVerdict::StaleState { .. } | PolicyVerdict::RequiresApproval { .. }
        )
    }
}

pub struct ActionGateway {
    state_fabric: Arc<StateFabric>,
    policy_engine: Arc<PolicyEngine>,
    decision_verifier: Arc<DecisionVerifier>,
    audit_store: Arc<AuditStore>,
}

impl ActionGateway {
    pub fn new(
        state_fabric: Arc<StateFabric>,
        policy_engine: Arc<PolicyEngine>,
        decision_verifier: Arc<DecisionVerifier>,
        audit_store: Arc<AuditStore>,
    ) -> Self {
        Self {
            state_fabric,
            policy_engine,
            decision_verifier,
            audit_store,
        }
    }

    /// Execute action with policy verdict tracking
    pub async fn execute_with_verdict(&self, proposal: &ActionProposal) -> EsaResult<GatewayResult> {
        let start_time = std::time::Instant::now();
        let trace_id = Uuid::new_v4().to_string();
        let decision_id = Uuid::new_v4().to_string();

        // Extract workload from action
        let workload_id = self.extract_workload_id(&proposal.action);
        
        // Capture before state
        let before_state = self.capture_state_snapshot(&workload_id)?;

        // Policy evaluation
        let policy_result = self.policy_engine.evaluate(proposal)?;
        let mut audit = AuditRecord::new(
            trace_id.clone(),
            decision_id.clone(),
            workload_id.clone(),
            self.state_fabric.current_version(),
            proposal.clone(),
            before_state.clone(),
        )
        .with_policy_result(serde_json::to_value(&policy_result).unwrap_or_default());

        match &policy_result.verdict {
            PolicyVerdict::Denied { reason } => {
                warn!("❌ Action denied: {}", reason);
                audit = audit.with_outcome(AuditOutcome::Denied {
                    reason: reason.clone(),
                });
                self.audit_store.append(audit);

                let execution = ActionExecution::new(proposal, serde_json::json!({}));
                return Ok(GatewayResult {
                    verdict: policy_result.verdict.clone(),
                    execution: Some(execution),
                    decision_id,
                    trace_id,
                    processing_time_ms: start_time.elapsed().as_millis() as u64,
                    explanation: reason.clone(),
                });
            }
            PolicyVerdict::StaleState { current_version, proposed_version, drift } => {
                let reason = format!(
                    "State version mismatch: proposed={}, current={}, drift={}",
                    proposed_version, current_version, drift
                );
                warn!("🔄 {}", reason);
                audit = audit.with_outcome(AuditOutcome::Denied {
                    reason: reason.clone(),
                });
                self.audit_store.append(audit);

                let execution = ActionExecution::new(proposal, serde_json::json!({}));
                return Ok(GatewayResult {
                    verdict: policy_result.verdict.clone(),
                    execution: Some(execution),
                    decision_id,
                    trace_id,
                    processing_time_ms: start_time.elapsed().as_millis() as u64,
                    explanation: reason,
                });
            }
            PolicyVerdict::RequiresApproval { reason } => {
                warn!("⏸️  Approval required: {}", reason);
                audit = audit.with_outcome(AuditOutcome::RequiresApproval {
                    reason: reason.clone(),
                });
                self.audit_store.append(audit);

                let execution = ActionExecution::new(proposal, serde_json::json!({}));
                return Ok(GatewayResult {
                    verdict: policy_result.verdict.clone(),
                    execution: Some(execution),
                    decision_id,
                    trace_id,
                    processing_time_ms: start_time.elapsed().as_millis() as u64,
                    explanation: reason.clone(),
                });
            }
            _ => {}
        }

        // Verification
        let verification = self.decision_verifier.verify(proposal)?;
        audit = audit.with_verification_result(serde_json::json!({
            "passed": verification.passed,
            "failures": verification.failures,
            "current_state_version": verification.current_state_version,
        }));
        if !verification.passed {
            audit = audit.with_outcome(AuditOutcome::Failed {
                error: format!("Verification failed"),
            });
            self.audit_store.append(audit);

            let execution = ActionExecution::new(proposal, serde_json::json!({}));
            return Ok(GatewayResult {
                verdict: PolicyVerdict::Denied {
                    reason: "Verification failed".to_string(),
                },
                execution: Some(execution),
                decision_id,
                trace_id,
                processing_time_ms: start_time.elapsed().as_millis() as u64,
                explanation: "Action failed verification".to_string(),
            });
        }

        // Snapshot for rollback (PRD §24)
        let snapshot_version = self
            .state_fabric
            .create_snapshot()
            .map(|s| s.version)
            .unwrap_or(0);

        // Execute — mutate runtime state and metrics (PRD AC-11, AC-13)
        info!("✅ Executing action");
        let before_metrics = before_state.clone();
        let apply_result = self.apply_action(proposal);

        let execution = match apply_result {
            Ok((after_metrics, effect_measurement)) => {
                info!(
                    "📉 Recovery applied for {}: P95 {:.0}ms → {:.0}ms, queue {} → {}, effectiveness {:.0}%",
                    workload_id,
                    before_metrics
                        .get("metrics")
                        .and_then(|m| m.get("p95_latency_ms"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    after_metrics
                        .get("p95_latency_ms")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    before_metrics
                        .get("metrics")
                        .and_then(|m| m.get("queue_depth"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    after_metrics
                        .get("queue_depth")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    effect_measurement.effectiveness * 100.0
                );

                let execution = ActionExecution::new(proposal, before_metrics)
                    .complete_with_effect(ActionOutcome::Success, after_metrics.clone(), effect_measurement);

                audit = audit
                    .with_outcome(AuditOutcome::Success)
                    .with_execution(execution.clone())
                    .with_after_state(after_metrics.clone());
                self.audit_store.append(audit);

                execution
            }
            Err(e) => {
                if snapshot_version > 0 {
                    let _ = self.state_fabric.restore_snapshot(snapshot_version);
                }
                warn!("❌ Action execution failed, snapshot restored: {}", e);

                audit = audit.with_outcome(AuditOutcome::Failed {
                    error: e.to_string(),
                });
                self.audit_store.append(audit);

                ActionExecution::new(proposal, before_metrics).fail(e.to_string())
            }
        };

        let explanation = if execution.outcome == Some(ActionOutcome::Success) {
            "Action executed successfully".to_string()
        } else {
            execution
                .error_message
                .clone()
                .unwrap_or_else(|| "Action execution failed".to_string())
        };

        Ok(GatewayResult {
            verdict: policy_result.verdict.clone(),
            execution: Some(execution),
            decision_id,
            trace_id,
            processing_time_ms: start_time.elapsed().as_millis() as u64,
            explanation,
        })
    }

    /// Apply typed action to the state fabric — actual runtime mutation (PRD §4, AC-11).
    fn apply_action(
        &self,
        proposal: &ActionProposal,
    ) -> EsaResult<(serde_json::Value, EffectMeasurement)> {
        match &proposal.action {
            ActionType::CreateReplica {
                workload_id,
                expected_effect,
                ..
            } => self.apply_create_replica(workload_id, expected_effect),
            ActionType::ShiftRoute {
                workload_id,
                to_region,
                expected_effect,
                ..
            } => self.apply_shift_route(workload_id, to_region.clone(), expected_effect),
            ActionType::ThrottleWorkload {
                workload_id,
                expected_effect,
                ..
            } => self.apply_metric_recovery(workload_id, expected_effect, false),
            ActionType::RestartWorkload {
                workload_id,
                expected_effect,
                ..
            } => self.apply_metric_recovery(workload_id, expected_effect, true),
            ActionType::MigratePartition {
                workload_id,
                target_region,
                expected_effect,
                ..
            } => {
                self.apply_shift_route(workload_id, target_region.clone(), expected_effect)
            }
            ActionType::Rollback {
                original_action_id,
                reason,
                target_snapshot,
            } => self.apply_rollback(original_action_id, target_snapshot, reason),
        }
    }

    fn apply_create_replica(
        &self,
        workload_id: &str,
        expected: &ExpectedEffect,
    ) -> EsaResult<(serde_json::Value, EffectMeasurement)> {
        let workload = self
            .state_fabric
            .get_workload(workload_id)
            .ok_or_else(|| esa_core::EsaError::ResourceNotFound {
                resource: format!("workload {}", workload_id),
            })?;

        let before = workload.metrics.clone();
        let mut workload = workload;

        if workload.replication.current_replicas < workload.replication.max_replicas {
            let old_replicas = workload.replication.current_replicas;
            workload.replication.current_replicas += 1;
            let load_factor = old_replicas as f64 / workload.replication.current_replicas as f64;

            self.apply_expected_effect(&mut workload.metrics, expected);

            workload.metrics.rate_per_min *= load_factor;
            workload.metrics.p95_latency_ms *= load_factor;
            workload.metrics.p50_latency_ms *= load_factor;
            workload.metrics.p99_latency_ms *= load_factor;
            workload.metrics.queue_depth =
                ((workload.metrics.queue_depth as f64) * load_factor).round() as u64;
        } else {
            self.apply_expected_effect(&mut workload.metrics, expected);
        }

        self.finalize_workload_recovery(&mut workload);
        self.commit_workload_change(workload, &before, expected)
    }

    fn apply_rollback(
        &self,
        _original_action_id: &str,
        target_snapshot: &str,
        reason: &str,
    ) -> EsaResult<(serde_json::Value, EffectMeasurement)> {
        let version = target_snapshot.parse::<u64>().map_err(|_| {
            esa_core::EsaError::InvalidAction {
                reason: "target_snapshot must be a numeric snapshot version".to_string(),
            }
        })?;

        let _before_summary = self.summarize_workloads();
        self.state_fabric.restore_snapshot(version)?;
        let after_summary = self.summarize_workloads();

        let expected = ExpectedEffect {
            latency_delta_ms: None,
            throughput_delta_pct: None,
            error_rate_delta: None,
            queue_delta: None,
            description: format!("Rollback to snapshot {}: {}", version, reason),
        };
        let observed = ObservedEffect {
            latency_delta_ms: None,
            throughput_delta_pct: None,
            error_rate_delta: None,
            queue_delta: None,
            description: "Workload state restored from snapshot".to_string(),
        };

        let effect = EffectMeasurement {
            expected,
            observed,
            effectiveness: 1.0,
            status: esa_core::EffectStatus::ObjectiveMet,
        };

        Ok((after_summary, effect))
    }

    fn summarize_workloads(&self) -> serde_json::Value {
        let workloads = self.state_fabric.list_workloads();
        let avg_p95 = if workloads.is_empty() {
            0.0
        } else {
            workloads.iter().map(|w| w.metrics.p95_latency_ms).sum::<f64>()
                / workloads.len() as f64
        };
        let total_queue = workloads.iter().map(|w| w.metrics.queue_depth).sum::<u64>();
        let healthy = workloads
            .iter()
            .filter(|w| w.state == WorkloadState::Healthy)
            .count();

        serde_json::json!({
            "workload_count": workloads.len(),
            "healthy_count": healthy,
            "avg_p95_ms": avg_p95,
            "total_queue": total_queue,
            "snapshot_version": self.state_fabric.current_version(),
        })
    }

    fn apply_shift_route(
        &self,
        workload_id: &str,
        to_region: Region,
        expected: &ExpectedEffect,
    ) -> EsaResult<(serde_json::Value, EffectMeasurement)> {
        let workload = self
            .state_fabric
            .get_workload(workload_id)
            .ok_or_else(|| esa_core::EsaError::ResourceNotFound {
                resource: format!("workload {}", workload_id),
            })?;

        let before = workload.metrics.clone();
        let mut workload = workload;
        workload.region = to_region;
        self.apply_expected_effect(&mut workload.metrics, expected);
        self.finalize_workload_recovery(&mut workload);
        self.commit_workload_change(workload, &before, expected)
    }

    fn apply_metric_recovery(
        &self,
        workload_id: &str,
        expected: &ExpectedEffect,
        full_reset: bool,
    ) -> EsaResult<(serde_json::Value, EffectMeasurement)> {
        let workload = self
            .state_fabric
            .get_workload(workload_id)
            .ok_or_else(|| esa_core::EsaError::ResourceNotFound {
                resource: format!("workload {}", workload_id),
            })?;

        let before = workload.metrics.clone();
        let mut workload = workload;

        if full_reset {
            workload.metrics.error_rate = 0.01;
            workload.metrics.queue_depth = workload.metrics.queue_depth.min(200);
        }

        self.apply_expected_effect(&mut workload.metrics, expected);
        self.finalize_workload_recovery(&mut workload);
        self.commit_workload_change(workload, &before, expected)
    }

    fn apply_expected_effect(&self, metrics: &mut WorkloadMetrics, expected: &ExpectedEffect) {
        if let Some(delta) = expected.latency_delta_ms {
            metrics.p95_latency_ms = (metrics.p95_latency_ms + delta).max(20.0);
            metrics.p50_latency_ms = (metrics.p50_latency_ms + delta * 0.6).max(10.0);
            metrics.p99_latency_ms = (metrics.p99_latency_ms + delta * 0.8).max(30.0);
        }
        if let Some(delta) = expected.queue_delta {
            metrics.queue_depth = (metrics.queue_depth as i64 + delta).max(0) as u64;
        }
        if let Some(delta) = expected.error_rate_delta {
            metrics.error_rate = (metrics.error_rate + delta).clamp(0.001, 0.99);
        }
        if let Some(delta_pct) = expected.throughput_delta_pct {
            metrics.rate_per_min *= 1.0 + delta_pct / 100.0;
        }
        metrics.timestamp = Utc::now();
    }

    fn finalize_workload_recovery(&self, workload: &mut esa_core::WorkloadEntity) {
        workload.metrics.timestamp = Utc::now();
        workload.state = Self::compute_workload_state(&workload.metrics);
        workload.version += 1;
        workload.updated_at = Utc::now();
    }

    fn commit_workload_change(
        &self,
        workload: esa_core::WorkloadEntity,
        before: &WorkloadMetrics,
        expected: &ExpectedEffect,
    ) -> EsaResult<(serde_json::Value, EffectMeasurement)> {
        let workload_id = workload.workload_id.clone();
        let state = workload.state.clone();
        let replicas = workload.replication.current_replicas;
        let after = workload.metrics.clone();
        self.state_fabric.upsert_workload(workload)?;

        let observed = Self::build_observed_effect(before, &after);
        let effect = EffectMeasurement::calculate(expected.clone(), observed);

        let after_json = serde_json::json!({
            "workload_id": workload_id,
            "state": state,
            "replicas": replicas,
            "p95_latency_ms": after.p95_latency_ms,
            "queue_depth": after.queue_depth,
            "error_rate": after.error_rate,
            "rate_per_min": after.rate_per_min,
        });

        Ok((after_json, effect))
    }

    fn extract_workload_id(&self, action: &ActionType) -> String {
        match action {
            ActionType::CreateReplica { workload_id, .. } => workload_id.clone(),
            ActionType::ShiftRoute { workload_id, .. } => workload_id.clone(),
            ActionType::MigratePartition { workload_id, .. } => workload_id.clone(),
            ActionType::ThrottleWorkload { workload_id, .. } => workload_id.clone(),
            ActionType::RestartWorkload { workload_id, .. } => workload_id.clone(),
            ActionType::Rollback { original_action_id, .. } => {
                if let Some(record) = self.audit_store.get_by_decision_id(original_action_id) {
                    record.workload_id
                } else if let Some(record) = self.audit_store.get(original_action_id) {
                    record.workload_id
                } else {
                    self.state_fabric
                        .list_workloads()
                        .first()
                        .map(|w| w.workload_id.clone())
                        .unwrap_or_else(|| "unknown".to_string())
                }
            }
        }
    }

    fn compute_workload_state(metrics: &WorkloadMetrics) -> WorkloadState {
        if metrics.p95_latency_ms > 250.0
            || metrics.queue_depth > 1000
            || metrics.error_rate > 0.05
        {
            if metrics.p95_latency_ms > 300.0
                || metrics.queue_depth > 1500
                || metrics.error_rate > 0.08
            {
                WorkloadState::Degraded
            } else {
                WorkloadState::Overloaded
            }
        } else {
            WorkloadState::Healthy
        }
    }

    fn build_observed_effect(before: &WorkloadMetrics, after: &WorkloadMetrics) -> ObservedEffect {
        ObservedEffect {
            latency_delta_ms: Some(after.p95_latency_ms - before.p95_latency_ms),
            throughput_delta_pct: if before.rate_per_min > 0.0 {
                Some(((after.rate_per_min / before.rate_per_min) - 1.0) * 100.0)
            } else {
                None
            },
            error_rate_delta: Some(after.error_rate - before.error_rate),
            queue_delta: Some(after.queue_depth as i64 - before.queue_depth as i64),
            description: "Observed metrics after gateway execution".to_string(),
        }
    }

    fn capture_state_snapshot(&self, workload_id: &str) -> EsaResult<serde_json::Value> {
        if let Some(workload) = self.state_fabric.get_workload(workload_id) {
            Ok(serde_json::json!({
                "workload_id": workload.workload_id,
                "state": workload.state,
                "metrics": {
                    "rate_per_min": workload.metrics.rate_per_min,
                    "p95_latency_ms": workload.metrics.p95_latency_ms,
                    "queue_depth": workload.metrics.queue_depth,
                },
            }))
        } else {
            Ok(serde_json::json!({}))
        }
    }

    pub fn demonstrate_verdict_types(&self) -> Vec<(String, GatewayResult)> {
        vec![
            (
                "ALLOW".to_string(),
                GatewayResult::new(
                    PolicyVerdict::Allowed,
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    45,
                    "All policy checks passed".to_string(),
                ),
            ),
            (
                "DENY".to_string(),
                GatewayResult::new(
                    PolicyVerdict::Denied {
                        reason: "Replica count exceeds limit".to_string(),
                    },
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    32,
                    "Action violates constraints".to_string(),
                ),
            ),
            (
                "STALE_STATE".to_string(),
                GatewayResult::new(
                    PolicyVerdict::StaleState {
                        current_version: 2,
                        proposed_version: 1,
                        drift: 1,
                    },
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    28,
                    "State version mismatch".to_string(),
                ),
            ),
            (
                "REQUIRES_APPROVAL".to_string(),
                GatewayResult::new(
                    PolicyVerdict::RequiresApproval {
                        reason: "High risk action".to_string(),
                    },
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    51,
                    "Manual approval needed".to_string(),
                ),
            ),
        ]
    }
}
