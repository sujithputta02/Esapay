use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::actions::{ActionExecution, ActionProposal, EffectMeasurement};

/// Comprehensive audit trail for decision lineage and replay

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    // Unique identifiers
    pub audit_id: String,
    pub trace_id: String,
    pub decision_id: String,
    
    // Context
    pub workload_id: String,
    pub state_version: u64,
    pub policy_version: String,
    
    // Decision lineage
    pub proposal: ActionProposal,
    pub policy_result: serde_json::Value,
    pub verification_result: serde_json::Value,
    pub execution: Option<ActionExecution>,
    
    // State snapshots
    pub before_state: serde_json::Value,
    pub after_state: Option<serde_json::Value>,
    
    // Effect tracking
    pub effect_measurement: Option<EffectMeasurement>,
    
    // Outcome
    pub final_outcome: AuditOutcome,
    pub rollback_status: Option<RollbackStatus>,
    
    // Metadata
    pub timestamp: DateTime<Utc>,
    pub agent_reasoning: AgentReasoningTrace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditOutcome {
    #[serde(rename = "SUCCESS")]
    Success,
    #[serde(rename = "DENIED")]
    Denied { reason: String },
    #[serde(rename = "FAILED")]
    Failed { error: String },
    #[serde(rename = "ROLLED_BACK")]
    RolledBack { reason: String },
    #[serde(rename = "REQUIRES_APPROVAL")]
    RequiresApproval { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackStatus {
    pub rollback_executed: bool,
    pub rollback_action_id: Option<String>,
    pub target_snapshot_version: Option<u64>,
    pub rollback_reason: String,
    pub rollback_timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReasoningTrace {
    pub monitor_output: Option<serde_json::Value>,
    pub diagnosis_output: Option<serde_json::Value>,
    pub planning_output: Option<serde_json::Value>,
    pub safety_output: Option<serde_json::Value>,
    pub evidence_refs: Vec<String>,
    pub confidence_scores: Vec<f64>,
}

impl AuditRecord {
    pub fn new(
        trace_id: String,
        decision_id: String,
        workload_id: String,
        state_version: u64,
        proposal: ActionProposal,
        before_state: serde_json::Value,
    ) -> Self {
        Self {
            audit_id: Uuid::new_v4().to_string(),
            trace_id,
            decision_id,
            workload_id,
            state_version,
            policy_version: "v1.0".to_string(),
            proposal,
            policy_result: serde_json::json!({}),
            verification_result: serde_json::json!({}),
            execution: None,
            before_state,
            after_state: None,
            effect_measurement: None,
            final_outcome: AuditOutcome::Success,
            rollback_status: None,
            timestamp: Utc::now(),
            agent_reasoning: AgentReasoningTrace {
                monitor_output: None,
                diagnosis_output: None,
                planning_output: None,
                safety_output: None,
                evidence_refs: Vec::new(),
                confidence_scores: Vec::new(),
            },
        }
    }

    pub fn with_policy_result(mut self, policy_result: serde_json::Value) -> Self {
        self.policy_result = policy_result;
        self
    }

    pub fn with_verification_result(mut self, verification_result: serde_json::Value) -> Self {
        self.verification_result = verification_result;
        self
    }

    pub fn with_execution(mut self, execution: ActionExecution) -> Self {
        self.effect_measurement = execution.effect_measurement.clone();
        self.execution = Some(execution);
        self
    }

    pub fn with_after_state(mut self, after_state: serde_json::Value) -> Self {
        self.after_state = Some(after_state);
        self
    }

    pub fn with_outcome(mut self, outcome: AuditOutcome) -> Self {
        self.final_outcome = outcome;
        self
    }

    pub fn with_agent_reasoning(mut self, reasoning: AgentReasoningTrace) -> Self {
        self.agent_reasoning = reasoning;
        self
    }

    pub fn with_rollback(mut self, status: RollbackStatus) -> Self {
        self.rollback_status = Some(status);
        self
    }
}

/// Audit Store - Append-only audit trail storage
pub struct AuditStore {
    records: std::sync::Arc<dashmap::DashMap<String, AuditRecord>>,
    by_trace: std::sync::Arc<dashmap::DashMap<String, Vec<String>>>,
    by_workload: std::sync::Arc<dashmap::DashMap<String, Vec<String>>>,
}

impl AuditStore {
    pub fn new() -> Self {
        Self {
            records: std::sync::Arc::new(dashmap::DashMap::new()),
            by_trace: std::sync::Arc::new(dashmap::DashMap::new()),
            by_workload: std::sync::Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn append(&self, record: AuditRecord) {
        let audit_id = record.audit_id.clone();
        let trace_id = record.trace_id.clone();
        let workload_id = record.workload_id.clone();

        // Store the record
        self.records.insert(audit_id.clone(), record);

        // Index by trace_id
        self.by_trace
            .entry(trace_id)
            .or_insert_with(Vec::new)
            .push(audit_id.clone());

        // Index by workload_id
        self.by_workload
            .entry(workload_id)
            .or_insert_with(Vec::new)
            .push(audit_id);
    }

    pub fn get(&self, audit_id: &str) -> Option<AuditRecord> {
        self.records.get(audit_id).map(|r| r.clone())
    }

    pub fn get_by_decision_id(&self, decision_id: &str) -> Option<AuditRecord> {
        self.records
            .iter()
            .find(|entry| entry.value().decision_id == decision_id)
            .map(|entry| entry.value().clone())
    }

    pub fn get_by_trace(&self, trace_id: &str) -> Vec<AuditRecord> {
        if let Some(audit_ids) = self.by_trace.get(trace_id) {
            audit_ids
                .iter()
                .filter_map(|id| self.records.get(id).map(|r| r.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_by_workload(&self, workload_id: &str) -> Vec<AuditRecord> {
        if let Some(audit_ids) = self.by_workload.get(workload_id) {
            audit_ids
                .iter()
                .filter_map(|id| self.records.get(id).map(|r| r.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn list_recent(&self, limit: usize) -> Vec<AuditRecord> {
        let mut records: Vec<AuditRecord> = self
            .records
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        
        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        records.truncate(limit);
        records
    }

    pub fn count(&self) -> usize {
        self.records.len()
    }
}

impl Default for AuditStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Decision Replay - Reconstruct decisions without new LLM generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    pub audit_id: String,
    pub trace_id: String,
    pub decision_id: String,
    pub can_replay: bool,
    pub replay_timestamp: DateTime<Utc>,
    
    // Reconstructed decision flow
    pub original_proposal: ActionProposal,
    pub original_state_version: u64,
    pub policy_decision: PolicyReplayDecision,
    pub verification_decision: VerificationReplayDecision,
    pub execution_outcome: Option<String>,
    
    // Deterministic replay checks
    pub state_version_valid: bool,
    pub policy_would_allow: bool,
    pub verification_would_pass: bool,
    
    // Context
    pub evidence: Vec<String>,
    pub reasoning_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyReplayDecision {
    pub verdict: String,
    pub rule_ids: Vec<String>,
    pub risk_score: f64,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReplayDecision {
    pub passed: bool,
    pub failures: Vec<String>,
    pub state_version_check: String,
}

pub struct DecisionReplayer {
    audit_store: std::sync::Arc<AuditStore>,
}

impl DecisionReplayer {
    pub fn new(audit_store: std::sync::Arc<AuditStore>) -> Self {
        Self { audit_store }
    }

    /// Replay a decision from audit record without calling LLM
    pub fn replay(&self, audit_id: &str) -> Option<ReplayResult> {
        let record = self.audit_store.get(audit_id)?;
        
        // Extract policy decision
        let policy_decision = self.reconstruct_policy_decision(&record);
        
        // Extract verification decision
        let verification_decision = self.reconstruct_verification_decision(&record);
        
        // Determine if the decision could be replayed deterministically
        let can_replay = !record.policy_result.is_null() 
            && !record.verification_result.is_null();
        
        // Extract execution outcome
        let execution_outcome = record.execution.as_ref().map(|exec| {
            if let Some(outcome) = &exec.outcome {
                format!("{:?}", outcome)
            } else {
                "PENDING".to_string()
            }
        });
        
        // Build reasoning summary from agent traces
        let reasoning_summary = self.build_reasoning_summary(&record);
        
        Some(ReplayResult {
            audit_id: record.audit_id.clone(),
            trace_id: record.trace_id.clone(),
            decision_id: record.decision_id.clone(),
            can_replay,
            replay_timestamp: Utc::now(),
            original_proposal: record.proposal.clone(),
            original_state_version: record.state_version,
            policy_decision,
            verification_decision,
            execution_outcome,
            state_version_valid: record.state_version > 0,
            policy_would_allow: self.would_policy_allow(&record),
            verification_would_pass: self.would_verification_pass(&record),
            evidence: record.agent_reasoning.evidence_refs.clone(),
            reasoning_summary,
        })
    }

    fn reconstruct_policy_decision(&self, record: &AuditRecord) -> PolicyReplayDecision {
        // Extract policy result from stored JSON
        let verdict = record.policy_result
            .get("verdict")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN")
            .to_string();
        
        let rule_ids = record.policy_result
            .get("rule_ids")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        
        let risk_score = record.policy_result
            .get("risk_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        
        let explanation = record.policy_result
            .get("explanation")
            .and_then(|v| v.as_str())
            .unwrap_or("No explanation available")
            .to_string();
        
        PolicyReplayDecision {
            verdict,
            rule_ids,
            risk_score,
            explanation,
        }
    }

    fn reconstruct_verification_decision(&self, record: &AuditRecord) -> VerificationReplayDecision {
        let passed = record.verification_result
            .get("passed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        
        let failures = record.verification_result
            .get("failures")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        
        let current_version = record.verification_result
            .get("current_state_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        
        let state_version_check = format!(
            "Proposal version: {}, Current version: {}, Drift: {}",
            record.state_version,
            current_version,
            current_version.saturating_sub(record.state_version)
        );
        
        VerificationReplayDecision {
            passed,
            failures,
            state_version_check,
        }
    }

    fn would_policy_allow(&self, record: &AuditRecord) -> bool {
        match &record.final_outcome {
            AuditOutcome::Success => true,
            AuditOutcome::Denied { .. } => false,
            AuditOutcome::RequiresApproval { .. } => false,
            AuditOutcome::Failed { .. } => false,
            AuditOutcome::RolledBack { .. } => false,
        }
    }

    fn would_verification_pass(&self, record: &AuditRecord) -> bool {
        record.verification_result
            .get("passed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    fn build_reasoning_summary(&self, record: &AuditRecord) -> String {
        let mut summary = Vec::new();
        
        if let Some(monitor) = &record.agent_reasoning.monitor_output {
            if let Some(condition) = monitor.get("condition") {
                summary.push(format!("Monitor detected: {}", condition));
            }
        }
        
        if let Some(diagnosis) = &record.agent_reasoning.diagnosis_output {
            if let Some(hypothesis) = diagnosis.get("hypothesis") {
                summary.push(format!("Diagnosis: {}", hypothesis));
            }
        }
        
        if let Some(planning) = &record.agent_reasoning.planning_output {
            if let Some(action) = planning.get("action") {
                summary.push(format!("Planned action: {}", action));
            }
        }
        
        if let Some(safety) = &record.agent_reasoning.safety_output {
            if let Some(assessment) = safety.get("assessment") {
                summary.push(format!("Safety assessment: {}", assessment));
            }
        }
        
        if summary.is_empty() {
            format!("Action: {:?}, Confidence: {:.2}", 
                    record.proposal.action, 
                    record.agent_reasoning.confidence_scores.first().unwrap_or(&0.0))
        } else {
            summary.join(" → ")
        }
    }

    /// Get all replay results for a trace
    pub fn replay_trace(&self, trace_id: &str) -> Vec<ReplayResult> {
        self.audit_store
            .get_by_trace(trace_id)
            .iter()
            .filter_map(|record| self.replay(&record.audit_id))
            .collect()
    }

    /// Get replay statistics
    pub fn replay_stats(&self) -> ReplayStats {
        let total = self.audit_store.count();
        let recent = self.audit_store.list_recent(100);
        
        let replayable = recent.iter()
            .filter(|r| !r.policy_result.is_null() && !r.verification_result.is_null())
            .count();
        
        let successful = recent.iter()
            .filter(|r| matches!(r.final_outcome, AuditOutcome::Success))
            .count();
        
        ReplayStats {
            total_decisions: total,
            replayable_decisions: replayable,
            successful_decisions: successful,
            sample_size: recent.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayStats {
    pub total_decisions: usize,
    pub replayable_decisions: usize,
    pub successful_decisions: usize,
    pub sample_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{ActionType, AgentId};
    use crate::types::{Region, RiskLevel};
    use crate::ExpectedEffect;

    #[test]
    fn test_audit_store() {
        let store = AuditStore::new();
        
        let proposal = ActionProposal::new(
            ActionType::CreateReplica {
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
                confidence: 0.9,
                risk: RiskLevel::Low,
                state_version: 1,
                rollback_enabled: true,
            },
            AgentId::Planning,
            vec!["evidence_1".to_string()],
        );

        let record = AuditRecord::new(
            "trace_001".to_string(),
            "decision_001".to_string(),
            "w_001".to_string(),
            1,
            proposal,
            serde_json::json!({}),
        );

        let audit_id = record.audit_id.clone();
        store.append(record);

        assert!(store.get(&audit_id).is_some());
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn test_decision_replay() {
        let store = std::sync::Arc::new(AuditStore::new());
        let replayer = DecisionReplayer::new(store.clone());
        
        let proposal = ActionProposal::new(
            ActionType::CreateReplica {
                workload_id: "w_001".to_string(),
                target_region: Region::IndiaSouth,
                reason: "Test replay".to_string(),
                expected_effect: ExpectedEffect {
                    latency_delta_ms: Some(-50.0),
                    throughput_delta_pct: None,
                    error_rate_delta: None,
                    queue_delta: None,
                    description: "Test effect".to_string(),
                },
                confidence: 0.9,
                risk: RiskLevel::Low,
                state_version: 1,
                rollback_enabled: true,
            },
            AgentId::Planning,
            vec!["evidence_1".to_string()],
        );

        let mut record = AuditRecord::new(
            "trace_002".to_string(),
            "decision_002".to_string(),
            "w_001".to_string(),
            1,
            proposal,
            serde_json::json!({}),
        );

        record = record.with_policy_result(serde_json::json!({
            "verdict": "ALLOWED",
            "rule_ids": ["RULE_001"],
            "risk_score": 0.3,
            "explanation": "All checks passed"
        }));

        let audit_id = record.audit_id.clone();
        store.append(record);

        let replay_result = replayer.replay(&audit_id);
        assert!(replay_result.is_some());
        
        let result = replay_result.unwrap();
        assert_eq!(result.policy_decision.verdict, "ALLOWED");
        assert!(result.can_replay);
    }
}
