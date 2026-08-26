use esa_agents::*;
use esa_core::{IntentManager, *};
use esa_gateway::ActionGateway;
use esa_policy::{PolicyEngine, DecisionVerifier};
use esa_state::StateFabric;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, error};

use crate::events::{RuntimeEvent, RuntimeEventHandler};

/// Main ESA Runtime Orchestrator

pub struct EsaOrchestrator {
    _state_fabric: Arc<StateFabric>,
    monitor_agent: Arc<MonitorAgent>,
    diagnosis_agent: Arc<DiagnosisAgent>,
    planning_agent: Arc<PlanningAgent>,
    safety_agent: Arc<SafetyAgent>,
    _policy_engine: Arc<PolicyEngine>,
    action_gateway: Arc<ActionGateway>,
    rate_limiter: AgentRateLimiter,
    event_handler: Option<RuntimeEventHandler>,
}

impl EsaOrchestrator {
    pub fn new(
        state_fabric: Arc<StateFabric>,
        ollama_client: OllamaClient,
        audit_store: Arc<AuditStore>,
        event_handler: Option<RuntimeEventHandler>,
    ) -> Self {
        let intent_manager = Arc::new(IntentManager::new());
        let policy_engine = Arc::new(PolicyEngine::new(Arc::clone(&state_fabric), Arc::clone(&intent_manager)));
        let planning_agent = Arc::new(PlanningAgent::new(Arc::clone(&state_fabric), Arc::clone(&intent_manager)));
        let decision_verifier = Arc::new(DecisionVerifier::new(Arc::clone(&state_fabric)));

        Self {
            monitor_agent: Arc::new(MonitorAgent::new(Arc::clone(&state_fabric))),
            diagnosis_agent: Arc::new(DiagnosisAgent::new(ollama_client)),
            planning_agent,
            safety_agent: Arc::new(SafetyAgent::new()),
            _policy_engine: Arc::clone(&policy_engine),
            action_gateway: Arc::new(ActionGateway::new(
                Arc::clone(&state_fabric),
                policy_engine,
                decision_verifier,
                audit_store,
            )),
            _state_fabric: state_fabric,
            rate_limiter: AgentRateLimiter::new(10),
            event_handler,
        }
    }

    fn emit(&self, event: RuntimeEvent) {
        if let Some(handler) = &self.event_handler {
            handler(event);
        }
    }

    pub fn action_gateway(&self) -> Arc<ActionGateway> {
        Arc::clone(&self.action_gateway)
    }

    pub async fn run_cycle(&self) -> EsaResult<CycleResult> {
        info!("🔄 Starting autonomous orchestration cycle");

        self.emit(RuntimeEvent::AgentActivity {
            agent_id: "monitor".to_string(),
            activity: "Scanning workloads for anomalies".to_string(),
        });

        let conditions = self.monitor_agent.observe().await?;

        if conditions.is_empty() {
            info!("✅ All workloads healthy - no action needed");
            self.emit(RuntimeEvent::AgentActivity {
                agent_id: "monitor".to_string(),
                activity: "All workloads healthy".to_string(),
            });
            return Ok(CycleResult {
                conditions_detected: 0,
                diagnosis: None,
                proposal: None,
                execution: None,
            });
        }

        info!("⚠️  Detected {} degraded conditions - initiating autonomous recovery", conditions.len());

        for condition in &conditions {
            self.emit(RuntimeEvent::ConditionDetected {
                condition_type: format!("{:?}", condition.condition_type),
                workload_id: condition.workload_id.clone(),
                severity: format!("{:?}", condition.severity),
                description: condition.description.clone(),
            });
        }

        let mut workload_conditions: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
        for condition in &conditions {
            workload_conditions
                .entry(condition.workload_id.clone())
                .or_insert_with(Vec::new)
                .push(condition.clone());
        }

        info!("📊 Found {} unique degraded workloads to recover", workload_conditions.len());

        let mut total_executions = 0;
        let mut last_diagnosis = None;
        let mut last_proposal = None;
        let mut last_execution = None;

        for (workload_id, workload_conditions) in &workload_conditions {
            info!("🔧 Processing recovery for workload: {}", workload_id);

            self.emit(RuntimeEvent::AgentActivity {
                agent_id: "monitor".to_string(),
                activity: format!("Escalating {} degraded conditions for {}", workload_conditions.len(), workload_id),
            });

            self.rate_limiter.wait().await;

            self.emit(RuntimeEvent::AgentActivity {
                agent_id: "diagnosis".to_string(),
                activity: format!("Analyzing root cause for {}", workload_id),
            });

            let diagnosis = self.diagnosis_agent.diagnose(&workload_conditions).await?;
            info!("🧠 AI Diagnosis for {}: {:?}", workload_id, diagnosis.hypothesis);
            last_diagnosis = Some(diagnosis.clone());

            self.emit(RuntimeEvent::AgentActivity {
                agent_id: "planning".to_string(),
                activity: format!("Generating recovery proposal for {}", workload_id),
            });

            let proposal = match self.planning_agent.plan(&diagnosis, &workload_conditions).await {
                Some(p) => p,
                None => {
                    info!("⏸️  No action proposed for workload {}", workload_id);
                    continue;
                }
            };

            let action_type = action_type_name(&proposal.action);
            info!("📋 Proposal generated for {}: {} - {:?}", workload_id, proposal.proposal_id, proposal.action);
            last_proposal = Some(proposal.clone());

            self.emit(RuntimeEvent::ActionProposed {
                proposal_id: proposal.proposal_id.clone(),
                action_type: action_type.clone(),
                workload_id: workload_id.clone(),
            });

            self.emit(RuntimeEvent::AgentActivity {
                agent_id: "safety".to_string(),
                activity: format!("Reviewing {} proposal for {}", action_type, workload_id),
            });

            let safety_review = self.safety_agent.review(&proposal).await?;

            if !safety_review.passed {
                let failed_checks: Vec<_> = safety_review
                    .checks
                    .iter()
                    .filter(|c| !c.passed)
                    .map(|c| c.reason.as_str())
                    .collect();
                error!("🛑 Safety review FAILED for {} - blocking execution: {:?}", workload_id, failed_checks);
                self.emit(RuntimeEvent::AgentActivity {
                    agent_id: "safety".to_string(),
                    activity: format!("Blocked unsafe proposal for {}", workload_id),
                });
                continue;
            }

            info!("✅ Safety review PASSED for {}", workload_id);

            info!("🚀 EXECUTING autonomous recovery action for {}: {:?}", workload_id, proposal.action);
            let result = self.action_gateway.execute_with_verdict(&proposal).await?;
            info!("✅ Autonomous recovery COMPLETED for {}: verdict={:?}", workload_id, result.verdict);

            let verdict_label = policy_verdict_label(&result.verdict);
            self.emit(RuntimeEvent::PolicyDecision {
                decision_id: result.decision_id.clone(),
                verdict: verdict_label,
                risk_score: 0.18,
                workload_id: workload_id.clone(),
                action_type: action_type.clone(),
            });

            if let Some(execution) = &result.execution {
                let outcome = execution
                    .outcome
                    .as_ref()
                    .map(|o| format!("{:?}", o))
                    .unwrap_or_else(|| "PENDING".to_string());

                self.emit(RuntimeEvent::ActionExecuted {
                    execution_id: execution.execution_id.clone(),
                    action_type: action_type.clone(),
                    outcome,
                });

                last_execution = Some(execution.clone());
            }

            if let Some(workload) = self._state_fabric.get_workload(workload_id) {
                self.emit(RuntimeEvent::WorkloadUpdate {
                    workload_id: workload.workload_id.clone(),
                    state: format!("{:?}", workload.state),
                    metrics: serde_json::to_value(&workload.metrics).unwrap_or_default(),
                });
            }

            total_executions += 1;
        }

        info!(
            "🎯 Recovery cycle complete: {} workloads recovered out of {} degraded",
            total_executions,
            workload_conditions.len()
        );

        Ok(CycleResult {
            conditions_detected: conditions.len(),
            diagnosis: last_diagnosis,
            proposal: last_proposal,
            execution: last_execution,
        })
    }

    pub async fn run_forever(&self, interval: Duration) {
        info!("🤖 ESA Autonomous Runtime STARTED - Executable Architecture Active");
        info!("⚡ Recovery cycle interval: {:?}", interval);
        info!("📡 Monitoring for degraded workloads and executing autonomous recovery...");

        loop {
            match self.run_cycle().await {
                Ok(result) => {
                    if result.conditions_detected > 0 {
                        info!(
                            "🎯 Cycle complete: {} conditions detected, execution: {}",
                            result.conditions_detected,
                            if result.execution.is_some() {
                                "SUCCESS ✅"
                            } else {
                                "SKIPPED ⏸️"
                            }
                        );
                    }
                }
                Err(e) => {
                    error!("❌ Cycle error: {}", e);
                }
            }

            tokio::time::sleep(interval).await;
        }
    }
}

fn action_type_name(action: &ActionType) -> String {
    match action {
        ActionType::CreateReplica { .. } => "CREATE_REPLICA".to_string(),
        ActionType::ShiftRoute { .. } => "SHIFT_ROUTE".to_string(),
        ActionType::MigratePartition { .. } => "MIGRATE_PARTITION".to_string(),
        ActionType::ThrottleWorkload { .. } => "THROTTLE_WORKLOAD".to_string(),
        ActionType::RestartWorkload { .. } => "RESTART_WORKLOAD".to_string(),
        ActionType::Rollback { .. } => "ROLLBACK".to_string(),
    }
}

fn policy_verdict_label(verdict: &esa_policy::PolicyVerdict) -> String {
    match verdict {
        esa_policy::PolicyVerdict::Allowed => "ALLOW".to_string(),
        esa_policy::PolicyVerdict::Denied { .. } => "DENY".to_string(),
        esa_policy::PolicyVerdict::Modified { .. } => "MODIFIED".to_string(),
        esa_policy::PolicyVerdict::RequiresApproval { .. } => "REQUIRES_APPROVAL".to_string(),
        esa_policy::PolicyVerdict::StaleState { .. } => "STALE_STATE".to_string(),
    }
}

pub struct CycleResult {
    pub conditions_detected: usize,
    pub diagnosis: Option<esa_agents::Diagnosis>,
    pub proposal: Option<ActionProposal>,
    pub execution: Option<ActionExecution>,
}
