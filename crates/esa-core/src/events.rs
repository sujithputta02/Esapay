use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{PaymentMethodClass, Region};

/// Payment event types from Razorpay Test Mode or synthetic generator

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PaymentEventType {
    #[serde(rename = "payment.authorized")]
    PaymentAuthorized,
    #[serde(rename = "payment.captured")]
    PaymentCaptured,
    #[serde(rename = "payment.failed")]
    PaymentFailed,
    #[serde(rename = "order.paid")]
    OrderPaid,
    #[serde(rename = "refund.created")]
    RefundCreated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentEvent {
    pub event_id: String,
    pub event_type: PaymentEventType,
    pub timestamp: DateTime<Utc>,
    pub region: Region,
    pub payment_method_class: PaymentMethodClass,
    /// Pseudonymous reference - never contains real card data
    pub pseudonymous_reference: String,
    pub amount_cents: Option<u64>,
    pub processing_latency_ms: Option<f64>,
    pub success: bool,
}

impl PaymentEvent {
    pub fn new_synthetic(
        event_type: PaymentEventType,
        region: Region,
        method: PaymentMethodClass,
        success: bool,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            event_type,
            timestamp: Utc::now(),
            region,
            payment_method_class: method,
            pseudonymous_reference: format!("PSEUDO-{}", Uuid::new_v4()),
            amount_cents: Some(10000),
            processing_latency_ms: None,
            success,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub event_id: String,
    pub timestamp: DateTime<Utc>,
    pub event_type: TelemetryEventType,
    pub source: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TelemetryEventType {
    #[serde(rename = "metrics.updated")]
    MetricsUpdated,
    #[serde(rename = "node.health")]
    NodeHealth,
    #[serde(rename = "workload.condition")]
    WorkloadCondition,
    #[serde(rename = "queue.depth")]
    QueueDepth,
}
