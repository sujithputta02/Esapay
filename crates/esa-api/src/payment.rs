use esa_core::*;
use esa_state::StateFabric;
use std::sync::Arc;
use tracing::{error, info};

pub struct PaymentApplyResult {
    pub event_id: String,
    pub workload_updated: bool,
    pub workload_id: Option<String>,
    pub workload: Option<WorkloadEntity>,
}

/// Apply a normalized payment event to matching workloads (synthetic or Razorpay).
pub fn apply_payment_event(
    state_fabric: &Arc<StateFabric>,
    event: &PaymentEvent,
) -> PaymentApplyResult {
    info!(
        "Ingesting payment event: {} - Amount: {} cents",
        event.event_id,
        event.amount_cents.unwrap_or(0)
    );

    let region = &event.region;
    let payment_method = &event.payment_method_class;

    let all_workloads = state_fabric.list_workloads();
    let matching_workload = all_workloads.iter().find(|w| {
        format!("{:?}", w.region)
            .to_lowercase()
            .contains(&format!("{:?}", region).to_lowercase())
    });

    let mut workload_updated = false;
    let mut workload_id = None;

    if let Some(mut workload) = matching_workload.cloned() {
        let base_load_increase = match payment_method {
            PaymentMethodClass::Upi => 1.5,
            PaymentMethodClass::Card => 2.0,
            PaymentMethodClass::NetBanking => 2.5,
            PaymentMethodClass::Wallet => 1.2,
        };

        let amount_cents = event.amount_cents.unwrap_or(5000);
        let amount_multiplier = if amount_cents > 1_000_000 {
            1.3
        } else if amount_cents > 500_000 {
            1.15
        } else {
            1.0
        };

        let total_impact = base_load_increase * amount_multiplier;

        workload.metrics.rate_per_min += 100.0 * total_impact;
        workload.metrics.p95_latency_ms += 15.0 * total_impact;
        workload.metrics.p99_latency_ms += 20.0 * total_impact;
        workload.metrics.queue_depth += (50.0 * total_impact) as u64;
        workload.metrics.error_rate =
            (workload.metrics.error_rate + 0.002 * total_impact).min(0.99);

        if workload.metrics.p95_latency_ms > 250.0 || workload.metrics.error_rate > 0.05 {
            workload.state = WorkloadState::Degraded;
            info!(
                "Workload {} now DEGRADED due to transaction load! P95: {:.0}ms, Errors: {:.2}%",
                workload.workload_id,
                workload.metrics.p95_latency_ms,
                workload.metrics.error_rate * 100.0
            );
        } else if workload.metrics.p95_latency_ms > 180.0 || workload.metrics.queue_depth > 400 {
            workload.state = WorkloadState::Overloaded;
            info!("Workload {} OVERLOADED from transactions", workload.workload_id);
        }

        workload_id = Some(workload.workload_id.clone());

        if let Err(e) = state_fabric.upsert_workload(workload.clone()) {
            error!("Failed to update workload after payment: {}", e);
        } else {
            workload_updated = true;
            info!(
                "Workload {} metrics updated: +{:.0} req/min, P95: {:.0}ms, Queue: {}",
                workload.workload_id,
                100.0 * total_impact,
                workload.metrics.p95_latency_ms,
                workload.metrics.queue_depth
            );
        }
    }

    PaymentApplyResult {
        event_id: event.event_id.clone(),
        workload_updated,
        workload_id: workload_id.clone(),
        workload: if workload_updated {
            workload_id.as_ref().and_then(|id| state_fabric.get_workload(id))
        } else {
            None
        },
    }
}
