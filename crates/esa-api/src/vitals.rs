use chrono::Utc;
use esa_core::WorkloadEntity;
use parking_lot::RwLock;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;

const MAX_SNAPSHOTS: usize = 120;

#[derive(Debug, Clone, Serialize)]
pub struct VitalsSnapshot {
    pub timestamp: String,
    pub total_tps: f64,
    pub avg_p95_ms: f64,
    pub avg_error_rate: f64,
    pub total_queue: u64,
    pub healthy_count: u32,
    pub degraded_count: u32,
    pub total_workloads: u32,
}

#[derive(Clone)]
pub struct VitalsStore {
    snapshots: Arc<RwLock<VecDeque<VitalsSnapshot>>>,
}

impl VitalsStore {
    pub fn new() -> Self {
        Self {
            snapshots: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    pub fn record_from_workloads(&self, workloads: &[WorkloadEntity]) {
        if workloads.is_empty() {
            return;
        }

        let total_tps = workloads
            .iter()
            .map(|w| w.metrics.rate_per_min / 60.0)
            .sum();
        let avg_p95 = workloads
            .iter()
            .map(|w| w.metrics.p95_latency_ms)
            .sum::<f64>()
            / workloads.len() as f64;
        let avg_error =
            workloads.iter().map(|w| w.metrics.error_rate).sum::<f64>() / workloads.len() as f64;
        let total_queue = workloads.iter().map(|w| w.metrics.queue_depth).sum();
        let healthy_count = workloads
            .iter()
            .filter(|w| matches!(w.state, esa_core::WorkloadState::Healthy))
            .count() as u32;
        let degraded_count = workloads.len() as u32 - healthy_count;

        let snapshot = VitalsSnapshot {
            timestamp: Utc::now().to_rfc3339(),
            total_tps,
            avg_p95_ms: avg_p95,
            avg_error_rate: avg_error,
            total_queue,
            healthy_count,
            degraded_count,
            total_workloads: workloads.len() as u32,
        };

        let mut buf = self.snapshots.write();
        if buf.len() >= MAX_SNAPSHOTS {
            buf.pop_front();
        }
        buf.push_back(snapshot);
    }

    pub fn latest(&self) -> Option<VitalsSnapshot> {
        self.snapshots.read().back().cloned()
    }

    pub fn history(&self) -> Vec<VitalsSnapshot> {
        self.snapshots.read().iter().cloned().collect()
    }
}
