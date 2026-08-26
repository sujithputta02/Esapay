use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{Region, ConsistencyMode};

/// Intent and Constraints system for ESA - PRD sections #6.1 and #8

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub intent_id: String,
    pub workload_id: String,
    pub goal: IntentGoal,
    pub constraints: Constraints,
    pub priority: IntentPriority,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentGoal {
    pub objective: String,
    pub target_metrics: TargetMetrics,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetMetrics {
    pub max_p95_latency_ms: Option<f64>,
    pub max_p99_latency_ms: Option<f64>,
    pub max_error_rate: Option<f64>,
    pub min_throughput_rpm: Option<f64>,
    pub max_queue_depth: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraints {
    // Resource constraints
    pub max_replicas: Option<u32>,
    pub min_replicas: Option<u32>,
    pub allowed_regions: Vec<Region>,
    pub forbidden_regions: Vec<Region>,
    
    // Quality constraints
    pub consistency_requirement: Option<ConsistencyMode>,
    pub availability_requirement: Option<f64>, // 0.0 to 1.0
    
    // Cost constraints
    pub max_cost_increase_percent: Option<f64>,
    pub budget_limit_usd: Option<f64>,
    
    // Safety constraints
    pub require_approval_for_high_risk: bool,
    pub require_rollback_capability: bool,
    pub max_concurrent_actions: Option<u32>,
    
    // Time constraints
    pub max_action_duration_seconds: Option<u64>,
    pub maintenance_windows: Vec<MaintenanceWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub description: String,
    pub allowed_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IntentPriority {
    #[serde(rename = "LOW")]
    Low,
    #[serde(rename = "NORMAL")]
    Normal,
    #[serde(rename = "HIGH")]
    High,
    #[serde(rename = "CRITICAL")]
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentViolation {
    pub constraint_type: String,
    pub current_value: serde_json::Value,
    pub constraint_value: serde_json::Value,
    pub severity: ViolationSeverity,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ViolationSeverity {
    #[serde(rename = "WARNING")]
    Warning,
    #[serde(rename = "VIOLATION")]
    Violation,
    #[serde(rename = "CRITICAL")]
    Critical,
}

impl Intent {
    pub fn new(workload_id: String, goal: IntentGoal, constraints: Constraints) -> Self {
        Self {
            intent_id: uuid::Uuid::new_v4().to_string(),
            workload_id,
            goal,
            constraints,
            priority: IntentPriority::Normal,
            created_at: Utc::now(),
            expires_at: None,
            active: true,
        }
    }

    pub fn with_priority(mut self, priority: IntentPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_expiry(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    pub fn is_active(&self) -> bool {
        self.active && !self.is_expired()
    }
}

impl Default for Constraints {
    fn default() -> Self {
        Self {
            max_replicas: Some(10),
            min_replicas: Some(2),
            allowed_regions: vec![
                Region::IndiaSouth,
                Region::IndiaWest,
                Region::IndiaNorth,
            ],
            forbidden_regions: Vec::new(),
            consistency_requirement: Some(ConsistencyMode::Strong),
            availability_requirement: Some(0.99),
            max_cost_increase_percent: Some(50.0),
            budget_limit_usd: None,
            require_approval_for_high_risk: true,
            require_rollback_capability: true,
            max_concurrent_actions: Some(3),
            max_action_duration_seconds: Some(300), // 5 minutes
            maintenance_windows: Vec::new(),
        }
    }
}

/// Intent Manager - Manages active intents and constraint validation
pub struct IntentManager {
    intents: std::sync::Arc<dashmap::DashMap<String, Intent>>,
    by_workload: std::sync::Arc<dashmap::DashMap<String, Vec<String>>>,
}

impl IntentManager {
    pub fn new() -> Self {
        Self {
            intents: std::sync::Arc::new(dashmap::DashMap::new()),
            by_workload: std::sync::Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn register_intent(&self, intent: Intent) -> String {
        let intent_id = intent.intent_id.clone();
        let workload_id = intent.workload_id.clone();

        self.intents.insert(intent_id.clone(), intent);

        // Index by workload
        self.by_workload
            .entry(workload_id)
            .or_insert_with(Vec::new)
            .push(intent_id.clone());

        intent_id
    }

    pub fn get_intent(&self, intent_id: &str) -> Option<Intent> {
        self.intents.get(intent_id).map(|entry| entry.clone())
    }

    pub fn get_active_intents_for_workload(&self, workload_id: &str) -> Vec<Intent> {
        if let Some(intent_ids) = self.by_workload.get(workload_id) {
            intent_ids
                .iter()
                .filter_map(|id| self.intents.get(id))
                .map(|entry| entry.clone())
                .filter(|intent| intent.is_active())
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn deactivate_intent(&self, intent_id: &str) -> bool {
        if let Some(mut intent) = self.intents.get_mut(intent_id) {
            intent.active = false;
            true
        } else {
            false
        }
    }

    pub fn list_active_intents(&self) -> Vec<Intent> {
        self.intents
            .iter()
            .map(|entry| entry.value().clone())
            .filter(|intent| intent.is_active())
            .collect()
    }

    pub fn cleanup_expired(&self) -> usize {
        let expired_ids: Vec<String> = self.intents
            .iter()
            .filter(|entry| entry.value().is_expired())
            .map(|entry| entry.key().clone())
            .collect();

        let count = expired_ids.len();
        for id in expired_ids {
            self.intents.remove(&id);
        }

        count
    }
}

impl Default for IntentManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Constraint Validator - Validates actions against intent constraints
pub struct ConstraintValidator {
    intent_manager: std::sync::Arc<IntentManager>,
}

impl ConstraintValidator {
    pub fn new(intent_manager: std::sync::Arc<IntentManager>) -> Self {
        Self { intent_manager }
    }

    pub fn validate_action(
        &self,
        action: &crate::actions::ActionProposal,
    ) -> Result<Vec<IntentViolation>, crate::errors::EsaError> {
        let workload_id = self.extract_workload_id(&action.action);
        let intents = self.intent_manager.get_active_intents_for_workload(&workload_id);

        let mut violations = Vec::new();

        for intent in intents {
            let intent_violations = self.validate_against_intent(action, &intent)?;
            violations.extend(intent_violations);
        }

        Ok(violations)
    }

    fn extract_workload_id(&self, action: &crate::actions::ActionType) -> String {
        match action {
            crate::actions::ActionType::CreateReplica { workload_id, .. } => workload_id.clone(),
            crate::actions::ActionType::ShiftRoute { workload_id, .. } => workload_id.clone(),
            crate::actions::ActionType::MigratePartition { workload_id, .. } => workload_id.clone(),
            crate::actions::ActionType::ThrottleWorkload { workload_id, .. } => workload_id.clone(),
            crate::actions::ActionType::RestartWorkload { workload_id, .. } => workload_id.clone(),
            crate::actions::ActionType::Rollback { original_action_id, .. } => original_action_id.clone(),
        }
    }

    fn validate_against_intent(
        &self,
        action: &crate::actions::ActionProposal,
        intent: &Intent,
    ) -> Result<Vec<IntentViolation>, crate::errors::EsaError> {
        let mut violations = Vec::new();
        let constraints = &intent.constraints;

        // Validate region constraints
        if let Some(target_region) = self.extract_target_region(&action.action) {
            if !constraints.allowed_regions.is_empty() 
                && !constraints.allowed_regions.contains(&target_region) {
                violations.push(IntentViolation {
                    constraint_type: "allowed_regions".to_string(),
                    current_value: serde_json::to_value(&target_region)?,
                    constraint_value: serde_json::to_value(&constraints.allowed_regions)?,
                    severity: ViolationSeverity::Violation,
                    description: format!("Target region {:?} not in allowed regions", target_region),
                });
            }

            if constraints.forbidden_regions.contains(&target_region) {
                violations.push(IntentViolation {
                    constraint_type: "forbidden_regions".to_string(),
                    current_value: serde_json::to_value(&target_region)?,
                    constraint_value: serde_json::to_value(&constraints.forbidden_regions)?,
                    severity: ViolationSeverity::Critical,
                    description: format!("Target region {:?} is forbidden", target_region),
                });
            }
        }

        // Validate risk constraints
        let risk_level = self.extract_risk_level(&action.action);
        if constraints.require_approval_for_high_risk 
            && matches!(risk_level, Some(crate::types::RiskLevel::High | crate::types::RiskLevel::Critical)) {
            violations.push(IntentViolation {
                constraint_type: "require_approval_for_high_risk".to_string(),
                current_value: serde_json::to_value(&risk_level)?,
                constraint_value: serde_json::Value::Bool(true),
                severity: ViolationSeverity::Warning,
                description: "High risk action requires approval".to_string(),
            });
        }

        // Validate rollback requirement
        if constraints.require_rollback_capability {
            let rollback_enabled = self.extract_rollback_enabled(&action.action);
            if !rollback_enabled {
                violations.push(IntentViolation {
                    constraint_type: "require_rollback_capability".to_string(),
                    current_value: serde_json::Value::Bool(rollback_enabled),
                    constraint_value: serde_json::Value::Bool(true),
                    severity: ViolationSeverity::Violation,
                    description: "Action must have rollback capability enabled".to_string(),
                });
            }
        }

        Ok(violations)
    }

    fn extract_target_region(&self, action: &crate::actions::ActionType) -> Option<Region> {
        match action {
            crate::actions::ActionType::CreateReplica { target_region, .. } => Some(target_region.clone()),
            crate::actions::ActionType::ShiftRoute { to_region, .. } => Some(to_region.clone()),
            crate::actions::ActionType::MigratePartition { target_region, .. } => Some(target_region.clone()),
            _ => None,
        }
    }

    fn extract_risk_level(&self, action: &crate::actions::ActionType) -> Option<crate::types::RiskLevel> {
        match action {
            crate::actions::ActionType::CreateReplica { risk, .. } => Some(risk.clone()),
            crate::actions::ActionType::ShiftRoute { risk, .. } => Some(risk.clone()),
            crate::actions::ActionType::MigratePartition { risk, .. } => Some(risk.clone()),
            crate::actions::ActionType::ThrottleWorkload { risk, .. } => Some(risk.clone()),
            crate::actions::ActionType::RestartWorkload { risk, .. } => Some(risk.clone()),
            crate::actions::ActionType::Rollback { .. } => Some(crate::types::RiskLevel::High),
        }
    }

    fn extract_rollback_enabled(&self, action: &crate::actions::ActionType) -> bool {
        match action {
            crate::actions::ActionType::CreateReplica { rollback_enabled, .. } => *rollback_enabled,
            crate::actions::ActionType::ShiftRoute { rollback_enabled, .. } => *rollback_enabled,
            crate::actions::ActionType::MigratePartition { rollback_enabled, .. } => *rollback_enabled,
            crate::actions::ActionType::ThrottleWorkload { rollback_enabled, .. } => *rollback_enabled,
            crate::actions::ActionType::RestartWorkload { .. } => true,
            crate::actions::ActionType::Rollback { .. } => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{ActionProposal, ActionType, AgentId, ExpectedEffect};

    #[test]
    fn test_intent_creation() {
        let goal = IntentGoal {
            objective: "Keep P95 latency below 250ms".to_string(),
            target_metrics: TargetMetrics {
                max_p95_latency_ms: Some(250.0),
                max_error_rate: Some(0.01),
                min_throughput_rpm: Some(1000.0),
                max_p99_latency_ms: None,
                max_queue_depth: None,
            },
            description: "Primary performance goal".to_string(),
        };

        let constraints = Constraints::default();
        let intent = Intent::new("w_001".to_string(), goal, constraints);

        assert!(intent.is_active());
        assert!(!intent.is_expired());
    }

    #[test]
    fn test_constraint_validation() {
        let manager = std::sync::Arc::new(IntentManager::new());
        let validator = ConstraintValidator::new(manager.clone());

        let goal = IntentGoal {
            objective: "Test goal".to_string(),
            target_metrics: TargetMetrics {
                max_p95_latency_ms: Some(250.0),
                max_error_rate: None,
                min_throughput_rpm: None,
                max_p99_latency_ms: None,
                max_queue_depth: None,
            },
            description: "Test".to_string(),
        };

        let mut constraints = Constraints::default();
        constraints.allowed_regions = vec![Region::IndiaSouth];

        let intent = Intent::new("w_001".to_string(), goal, constraints);
        manager.register_intent(intent);

        let proposal = ActionProposal::new(
            ActionType::CreateReplica {
                workload_id: "w_001".to_string(),
                target_region: Region::UsEast, // Not in allowed regions
                reason: "Test".to_string(),
                expected_effect: ExpectedEffect {
                    latency_delta_ms: Some(-50.0),
                    throughput_delta_pct: None,
                    error_rate_delta: None,
                    queue_delta: None,
                    description: "Test".to_string(),
                },
                confidence: 0.9,
                risk: crate::types::RiskLevel::Low,
                state_version: 1,
                rollback_enabled: true,
            },
            AgentId::Planning,
            vec!["test_evidence".to_string()],
        );

        let violations = validator.validate_action(&proposal).unwrap();
        assert!(!violations.is_empty());
        
        // Should have region violation
        let region_violation = violations.iter().find(|v| v.constraint_type == "allowed_regions");
        assert!(region_violation.is_some());
    }
}