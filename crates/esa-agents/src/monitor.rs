use esa_core::{EsaResult, WorkloadState};
use esa_state::StateFabric;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

/// Monitor Agent - Detects workload conditions and anomalies

pub struct MonitorAgent {
    state_fabric: Arc<StateFabric>,
}

impl MonitorAgent {
    pub fn new(state_fabric: Arc<StateFabric>) -> Self {
        Self { state_fabric }
    }

    pub async fn observe(&self) -> EsaResult<Vec<Condition>> {
        let workloads = self.state_fabric.list_workloads();
        let mut conditions = Vec::new();

        for workload in workloads {
            // Check for high latency
            if workload.metrics.p95_latency_ms > 250.0 {
                conditions.push(Condition {
                    condition_type: ConditionType::HighLatency,
                    workload_id: workload.workload_id.clone(),
                    severity: Severity::Medium,
                    description: format!(
                        "P95 latency is {}ms (threshold: 250ms)",
                        workload.metrics.p95_latency_ms
                    ),
                    metrics: serde_json::to_value(&workload.metrics).unwrap(),
                });
            }

            // Check for high queue depth
            if workload.metrics.queue_depth > 1000 {
                conditions.push(Condition {
                    condition_type: ConditionType::QueueBacklog,
                    workload_id: workload.workload_id.clone(),
                    severity: Severity::High,
                    description: format!(
                        "Queue depth is {} (threshold: 1000)",
                        workload.metrics.queue_depth
                    ),
                    metrics: serde_json::to_value(&workload.metrics).unwrap(),
                });
            }

            // Check for high error rate
            if workload.metrics.error_rate > 0.05 {
                conditions.push(Condition {
                    condition_type: ConditionType::HighErrorRate,
                    workload_id: workload.workload_id.clone(),
                    severity: Severity::High,
                    description: format!(
                        "Error rate is {:.2}% (threshold: 5%)",
                        workload.metrics.error_rate * 100.0
                    ),
                    metrics: serde_json::to_value(&workload.metrics).unwrap(),
                });
            }

            // Check workload state
            if workload.state == WorkloadState::Degraded
                || workload.state == WorkloadState::Overloaded
            {
                conditions.push(Condition {
                    condition_type: ConditionType::WorkloadDegraded,
                    workload_id: workload.workload_id.clone(),
                    severity: Severity::High,
                    description: format!("Workload state is {:?}", workload.state),
                    metrics: serde_json::to_value(&workload.metrics).unwrap(),
                });
            }
        }

        if !conditions.is_empty() {
            info!("Monitor agent detected {} conditions", conditions.len());
        }

        Ok(conditions)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub condition_type: ConditionType,
    pub workload_id: String,
    pub severity: Severity,
    pub description: String,
    pub metrics: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConditionType {
    #[serde(rename = "HIGH_LATENCY")]
    HighLatency,
    #[serde(rename = "QUEUE_BACKLOG")]
    QueueBacklog,
    #[serde(rename = "HIGH_ERROR_RATE")]
    HighErrorRate,
    #[serde(rename = "WORKLOAD_DEGRADED")]
    WorkloadDegraded,
    #[serde(rename = "NODE_FAILURE")]
    NodeFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    #[serde(rename = "LOW")]
    Low,
    #[serde(rename = "MEDIUM")]
    Medium,
    #[serde(rename = "HIGH")]
    High,
    #[serde(rename = "CRITICAL")]
    Critical,
}
