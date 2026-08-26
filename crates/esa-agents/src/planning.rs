use crate::diagnosis::Diagnosis;
use crate::monitor::Condition;
use esa_core::{ActionProposal, ActionType, AgentId, ExpectedEffect, Region, RiskLevel};
use esa_core::{Constraints, Intent, IntentGoal, IntentManager, IntentPriority, TargetMetrics};
use esa_state::StateFabric;
use std::sync::Arc;
use tracing::info;

/// Planning Agent - Generates typed action proposals from diagnosis

pub struct PlanningAgent {
    state_fabric: Arc<StateFabric>,
    intent_manager: Arc<IntentManager>,
}

impl PlanningAgent {
    pub fn new(state_fabric: Arc<StateFabric>, intent_manager: Arc<IntentManager>) -> Self {
        Self {
            state_fabric,
            intent_manager,
        }
    }

    pub fn create_default_intent(&self, workload_id: &str) -> String {
        let goal = IntentGoal {
            objective: "Keep P95 latency below 250ms in primary region".to_string(),
            target_metrics: TargetMetrics {
                max_p95_latency_ms: Some(250.0),
                max_p99_latency_ms: Some(500.0),
                max_error_rate: Some(0.05),
                min_throughput_rpm: Some(100.0),
                max_queue_depth: Some(1000),
            },
            description: "Default performance and reliability intent".to_string(),
        };

        let constraints = Constraints::default();

        let intent = Intent::new(workload_id.to_string(), goal, constraints)
            .with_priority(IntentPriority::Normal);

        let intent_id = self.intent_manager.register_intent(intent);
        info!(
            "📋 Created default intent {} for workload {}",
            intent_id, workload_id
        );
        intent_id
    }

    pub async fn plan(
        &self,
        diagnosis: &Diagnosis,
        conditions: &[Condition],
    ) -> Option<ActionProposal> {
        if diagnosis.recommended_action.is_none() {
            return None;
        }

        let workload_id = if let Some(condition) = conditions.first() {
            condition.workload_id.clone()
        } else {
            return None;
        };

        let workload = self.state_fabric.get_workload(&workload_id)?;
        let current_version = self.state_fabric.current_version();

        // Get active intents for the workload
        let active_intents = self
            .intent_manager
            .get_active_intents_for_workload(&workload_id);

        // If no intents exist, create a default one
        if active_intents.is_empty() {
            self.create_default_intent(&workload_id);
        }

        // Get the highest priority intent for guidance
        let guiding_intent = self
            .intent_manager
            .get_active_intents_for_workload(&workload_id)
            .into_iter()
            .max_by_key(|i| match i.priority {
                IntentPriority::Critical => 4,
                IntentPriority::High => 3,
                IntentPriority::Normal => 2,
                IntentPriority::Low => 1,
            });

        let action = match diagnosis.recommended_action.as_deref() {
            Some("CREATE_REPLICA") => {
                info!(
                    "Planning agent proposing CREATE_REPLICA for {}",
                    workload_id
                );

                // Consider intent constraints
                let target_region = if let Some(intent) = &guiding_intent {
                    // Prefer allowed regions from intent
                    intent
                        .constraints
                        .allowed_regions
                        .first()
                        .cloned()
                        .unwrap_or_else(|| workload.region.clone())
                } else {
                    workload.region.clone()
                };

                ActionType::CreateReplica {
                    workload_id: workload_id.clone(),
                    target_region,
                    reason: format!("{} (Intent-guided)", diagnosis.hypothesis),
                    expected_effect: ExpectedEffect {
                        latency_delta_ms: Some(-80.0),
                        throughput_delta_pct: Some(30.0),
                        error_rate_delta: Some(-0.02),
                        queue_delta: Some(-500),
                        description: "Distribute load across additional replica".to_string(),
                    },
                    confidence: diagnosis.confidence,
                    risk: RiskLevel::Low,
                    state_version: current_version,
                    rollback_enabled: guiding_intent
                        .as_ref()
                        .map(|i| i.constraints.require_rollback_capability)
                        .unwrap_or(true),
                }
            }
            Some("SHIFT_ROUTE") => {
                info!("Planning agent proposing SHIFT_ROUTE for {}", workload_id);

                // Find a fallback region from workload or intent
                let target_region = if let Some(intent) = &guiding_intent {
                    intent
                        .constraints
                        .allowed_regions
                        .iter()
                        .find(|&&ref r| r != &workload.region)
                        .cloned()
                        .or_else(|| workload.locality.fallback_regions.first().cloned())
                        .unwrap_or(Region::IndiaWest)
                } else {
                    workload
                        .locality
                        .fallback_regions
                        .first()
                        .cloned()
                        .unwrap_or(Region::IndiaWest)
                };

                ActionType::ShiftRoute {
                    workload_id: workload_id.clone(),
                    from_region: workload.region.clone(),
                    to_region: target_region,
                    traffic_percentage: 30.0,
                    reason: format!("{} (Intent-guided)", diagnosis.hypothesis),
                    expected_effect: ExpectedEffect {
                        latency_delta_ms: Some(-40.0),
                        throughput_delta_pct: None,
                        error_rate_delta: Some(-0.01),
                        queue_delta: Some(-200),
                        description: "Shift traffic to healthy region".to_string(),
                    },
                    confidence: diagnosis.confidence,
                    risk: RiskLevel::Medium,
                    state_version: current_version,
                    rollback_enabled: guiding_intent
                        .as_ref()
                        .map(|i| i.constraints.require_rollback_capability)
                        .unwrap_or(true),
                }
            }
            _ => return None,
        };

        let proposal =
            ActionProposal::new(action, AgentId::Planning, diagnosis.evidence_refs.clone());

        info!(
            "📝 Planning agent created proposal with intent guidance: {}",
            guiding_intent
                .as_ref()
                .map(|i| i.goal.objective.as_str())
                .unwrap_or("default")
        );

        Some(proposal)
    }
}
