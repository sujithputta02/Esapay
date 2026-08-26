use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::types::{Region, RiskLevel};

/// Typed action contracts that agents can propose

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action")]
pub enum ActionType {
    #[serde(rename = "CREATE_REPLICA")]
    CreateReplica {
        workload_id: String,
        target_region: Region,
        reason: String,
        expected_effect: ExpectedEffect,
        confidence: f64,
        risk: RiskLevel,
        state_version: u64,
        rollback_enabled: bool,
    },
    #[serde(rename = "SHIFT_ROUTE")]
    ShiftRoute {
        workload_id: String,
        from_region: Region,
        to_region: Region,
        traffic_percentage: f64,
        reason: String,
        expected_effect: ExpectedEffect,
        confidence: f64,
        risk: RiskLevel,
        state_version: u64,
        rollback_enabled: bool,
    },
    #[serde(rename = "MIGRATE_PARTITION")]
    MigratePartition {
        workload_id: String,
        shard_id: String,
        target_region: Region,
        reason: String,
        expected_effect: ExpectedEffect,
        confidence: f64,
        risk: RiskLevel,
        state_version: u64,
        rollback_enabled: bool,
    },
    #[serde(rename = "THROTTLE_WORKLOAD")]
    ThrottleWorkload {
        workload_id: String,
        throttle_percentage: f64,
        reason: String,
        expected_effect: ExpectedEffect,
        confidence: f64,
        risk: RiskLevel,
        state_version: u64,
        rollback_enabled: bool,
    },
    #[serde(rename = "ROLLBACK")]
    Rollback {
        original_action_id: String,
        reason: String,
        target_snapshot: String,
    },
    #[serde(rename = "RESTART_WORKLOAD")]
    RestartWorkload {
        workload_id: String,
        reason: String,
        graceful: bool,
        expected_effect: ExpectedEffect,
        confidence: f64,
        risk: RiskLevel,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExpectedEffect {
    pub latency_delta_ms: Option<f64>,
    pub throughput_delta_pct: Option<f64>,
    pub error_rate_delta: Option<f64>,
    pub queue_delta: Option<i64>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservedEffect {
    pub latency_delta_ms: Option<f64>,
    pub throughput_delta_pct: Option<f64>,
    pub error_rate_delta: Option<f64>,
    pub queue_delta: Option<i64>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectMeasurement {
    pub expected: ExpectedEffect,
    pub observed: ObservedEffect,
    pub effectiveness: f64,
    pub status: EffectStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EffectStatus {
    #[serde(rename = "OBJECTIVE_MET")]
    ObjectiveMet,
    #[serde(rename = "PARTIALLY_MET")]
    PartiallyMet,
    #[serde(rename = "UNDERPERFORMED")]
    Underperformed,
    #[serde(rename = "FAILED")]
    Failed,
}

impl EffectMeasurement {
    pub fn calculate(expected: ExpectedEffect, observed: ObservedEffect) -> Self {
        let mut effectiveness_scores = Vec::new();

        // Calculate effectiveness for each metric
        if let (Some(exp_lat), Some(obs_lat)) =
            (expected.latency_delta_ms, observed.latency_delta_ms)
        {
            if exp_lat != 0.0 {
                let score = (obs_lat / exp_lat).clamp(0.0, 1.0);
                effectiveness_scores.push(score);
            }
        }

        if let (Some(exp_thr), Some(obs_thr)) =
            (expected.throughput_delta_pct, observed.throughput_delta_pct)
        {
            if exp_thr != 0.0 {
                let score = (obs_thr / exp_thr).clamp(0.0, 1.0);
                effectiveness_scores.push(score);
            }
        }

        if let (Some(exp_err), Some(obs_err)) =
            (expected.error_rate_delta, observed.error_rate_delta)
        {
            if exp_err != 0.0 {
                let score = (obs_err / exp_err).clamp(0.0, 1.0);
                effectiveness_scores.push(score);
            }
        }

        if let (Some(exp_queue), Some(obs_queue)) = (expected.queue_delta, observed.queue_delta) {
            if exp_queue != 0 {
                let score = (obs_queue as f64 / exp_queue as f64).clamp(0.0, 1.0);
                effectiveness_scores.push(score);
            }
        }

        // Calculate average effectiveness
        let effectiveness = if effectiveness_scores.is_empty() {
            1.0
        } else {
            effectiveness_scores.iter().sum::<f64>() / effectiveness_scores.len() as f64
        };

        // Determine status
        let status = if effectiveness >= 0.95 {
            EffectStatus::ObjectiveMet
        } else if effectiveness >= 0.75 {
            EffectStatus::PartiallyMet
        } else if effectiveness >= 0.5 {
            EffectStatus::Underperformed
        } else {
            EffectStatus::Failed
        };

        Self {
            expected,
            observed,
            effectiveness,
            status,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct ActionProposal {
    pub proposal_id: String,
    pub action: ActionType,
    pub proposed_by: AgentId,
    pub proposed_at: DateTime<Utc>,
    pub evidence_refs: Vec<String>,
    pub priority: ActionPriority,
}

impl ActionProposal {
    pub fn new(action: ActionType, agent: AgentId, evidence: Vec<String>) -> Self {
        Self {
            proposal_id: Uuid::new_v4().to_string(),
            action,
            proposed_by: agent,
            proposed_at: Utc::now(),
            evidence_refs: evidence,
            priority: ActionPriority::Normal,
        }
    }

    pub fn with_priority(mut self, priority: ActionPriority) -> Self {
        self.priority = priority;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentId {
    #[serde(rename = "monitor")]
    Monitor,
    #[serde(rename = "diagnosis")]
    Diagnosis,
    #[serde(rename = "planning")]
    Planning,
    #[serde(rename = "safety")]
    Safety,
}

impl AgentId {
    pub fn as_str(&self) -> &str {
        match self {
            AgentId::Monitor => "monitor",
            AgentId::Diagnosis => "diagnosis",
            AgentId::Planning => "planning",
            AgentId::Safety => "safety",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionPriority {
    #[serde(rename = "LOW")]
    Low,
    #[serde(rename = "NORMAL")]
    Normal,
    #[serde(rename = "HIGH")]
    High,
    #[serde(rename = "URGENT")]
    Urgent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionOutcome {
    #[serde(rename = "SUCCESS")]
    Success,
    #[serde(rename = "FAILED")]
    Failed,
    #[serde(rename = "ROLLED_BACK")]
    RolledBack,
    #[serde(rename = "PARTIAL")]
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExecution {
    pub execution_id: String,
    pub proposal_id: String,
    pub action: ActionType,
    pub executed_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub outcome: Option<ActionOutcome>,
    pub before_metrics: serde_json::Value,
    pub after_metrics: Option<serde_json::Value>,
    pub effect_measurement: Option<EffectMeasurement>,
    pub error_message: Option<String>,
}

impl ActionExecution {
    pub fn new(proposal: &ActionProposal, before_metrics: serde_json::Value) -> Self {
        Self {
            execution_id: Uuid::new_v4().to_string(),
            proposal_id: proposal.proposal_id.clone(),
            action: proposal.action.clone(),
            executed_at: Utc::now(),
            completed_at: None,
            outcome: None,
            before_metrics,
            after_metrics: None,
            effect_measurement: None,
            error_message: None,
        }
    }

    pub fn complete(mut self, outcome: ActionOutcome, after_metrics: serde_json::Value) -> Self {
        self.completed_at = Some(Utc::now());
        self.outcome = Some(outcome);
        self.after_metrics = Some(after_metrics);
        self
    }

    pub fn complete_with_effect(
        mut self,
        outcome: ActionOutcome,
        after_metrics: serde_json::Value,
        effect_measurement: EffectMeasurement,
    ) -> Self {
        self.completed_at = Some(Utc::now());
        self.outcome = Some(outcome);
        self.after_metrics = Some(after_metrics);
        self.effect_measurement = Some(effect_measurement);
        self
    }

    pub fn fail(mut self, error: String) -> Self {
        self.completed_at = Some(Utc::now());
        self.outcome = Some(ActionOutcome::Failed);
        self.error_message = Some(error);
        self
    }
}
