//! Razorpay Payment Gateway Domain Adapter
//!
//! Provides translation between domain-specific payment events (Checkout transactions,
//! UPI Intent flow, Card 3DS authentication, Webhooks, Ledger settlement) and
//! generic ESA StateFabric workload entities and typed action IRs.
//!
//! Architectural Pipeline:
//! ```text
//! ┌────────────────────────────────────────────────────────┐
//! │  Razorpay Payment Workloads (Checkout, UPI, Ledger)    │
//! └───────────────────────────┬────────────────────────────┘
//!                             │
//!                             ▼
//! ┌────────────────────────────────────────────────────────┐
//! │       Payment Domain Adapter (crates/esa-core)         │
//! │  - Maps Payment SLA (P95 < 250ms, SR > 99.5%)          │
//! │  - Enforces Data Sovereignty (RBI India Regions)       │
//! └───────────────────────────┬────────────────────────────┘
//!                             │
//!                             ▼
//! ┌────────────────────────────────────────────────────────┐
//! │                     ESA CORE                           │
//! │  - Streaming Monitor -> LLM Diagnosis -> Planning      │
//! │  - Invariant Safety -> Atomic OCC Gate -> Effect Check │
//! └───────────────────────────┬────────────────────────────┘
//!                             │
//!                             ▼
//! ┌────────────────────────────────────────────────────────┐
//! │       Governed Runtime (Kubernetes / Cloud Infra)      │
//! └────────────────────────────────────────────────────────┘
//! ```

use crate::types::{
    LocalityPreference, PaymentMethodClass, Region, ReplicationPolicy, WorkloadEntity,
    WorkloadMetrics, WorkloadState,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentTransactionTelemetry {
    pub payment_id: String,
    pub merchant_id: String,
    pub method: PaymentMethodClass,
    pub amount_inr: u64,
    pub latency_ms: f64,
    pub status: PaymentStatus,
    pub target_gateway: String,
    pub region: Region,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PaymentStatus {
    Authorized,
    Captured,
    Failed { error_code: String },
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentWorkloadDescriptor {
    pub workload_id: String,
    pub tier: String,
    pub primary_method: PaymentMethodClass,
    pub target_sla_p95_ms: f64,
    pub target_success_rate: f64,
}

pub struct PaymentAdapter;

impl PaymentAdapter {
    /// Translates Razorpay payment workload descriptor into a generic ESA WorkloadEntity
    pub fn to_workload_entity(
        desc: PaymentWorkloadDescriptor,
        region: Region,
        replicas: u32,
    ) -> WorkloadEntity {
        WorkloadEntity {
            workload_id: desc.workload_id.clone(),
            shard_id: format!("shard_{}", desc.workload_id),
            state: WorkloadState::Healthy,
            region: region.clone(),
            metrics: WorkloadMetrics {
                rate_per_min: 2500.0,
                p50_latency_ms: 45.0,
                p95_latency_ms: 120.0,
                p99_latency_ms: 180.0,
                error_rate: 0.005,
                queue_depth: 50,
                timestamp: Utc::now(),
            },
            replication: ReplicationPolicy {
                min_replicas: 2,
                max_replicas: 6,
                current_replicas: replicas,
                consistency_mode: crate::types::ConsistencyMode::Strong,
            },
            locality: LocalityPreference {
                preferred_region: region,
                fallback_regions: vec![Region::IndiaWest, Region::IndiaNorth],
            },
            lifecycle: crate::types::LifecycleState::Active,
            version: 1,
            updated_at: Utc::now(),
        }
    }
}
