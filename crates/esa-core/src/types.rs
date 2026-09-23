use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Core ESA types and domain models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Region {
    #[serde(rename = "IN-SOUTH")]
    IndiaSouth,
    #[serde(rename = "IN-WEST")]
    IndiaWest,
    #[serde(rename = "IN-NORTH")]
    IndiaNorth,
    #[serde(rename = "US-EAST")]
    UsEast,
    #[serde(rename = "EU-WEST")]
    EuWest,
}

impl Region {
    pub fn as_str(&self) -> &str {
        match self {
            Region::IndiaSouth => "IN-SOUTH",
            Region::IndiaWest => "IN-WEST",
            Region::IndiaNorth => "IN-NORTH",
            Region::UsEast => "US-EAST",
            Region::EuWest => "EU-WEST",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PaymentMethodClass {
    #[serde(rename = "UPI")]
    Upi,
    #[serde(rename = "CARD")]
    Card,
    #[serde(rename = "NETBANKING")]
    NetBanking,
    #[serde(rename = "WALLET")]
    Wallet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub enum PaymentGateway {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "razorpay")]
    Razorpay,
    #[serde(rename = "stripe")]
    Stripe,
    #[serde(rename = "phonepe")]
    PhonePe,
    #[serde(rename = "cashfree")]
    Cashfree,
    #[serde(rename = "paytm")]
    Paytm,
    #[serde(rename = "adyen")]
    Adyen,
}

impl PaymentGateway {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaymentGateway::Auto => "auto",
            PaymentGateway::Razorpay => "razorpay",
            PaymentGateway::Stripe => "stripe",
            PaymentGateway::PhonePe => "phonepe",
            PaymentGateway::Cashfree => "cashfree",
            PaymentGateway::Paytm => "paytm",
            PaymentGateway::Adyen => "adyen",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayHealth {
    pub gateway: PaymentGateway,
    pub name: String,
    pub status: WorkloadState,
    pub p95_latency_ms: f64,
    pub success_rate: f64,
    pub active_traffic_pct: f64,
    pub is_healthy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRouteDecision {
    pub transaction_id: String,
    pub amount: u64,
    pub currency: String,
    pub requested_gateway: PaymentGateway,
    pub routed_gateway: PaymentGateway,
    pub failover_triggered: bool,
    pub routing_reason: String,
    pub checkout_url: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkloadState {
    #[serde(rename = "HEALTHY")]
    Healthy,
    #[serde(rename = "DEGRADED")]
    Degraded,
    #[serde(rename = "OVERLOADED")]
    Overloaded,
    #[serde(rename = "RECOVERING")]
    Recovering,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConsistencyMode {
    #[serde(rename = "STRONG")]
    Strong,
    #[serde(rename = "EVENTUAL")]
    Eventual,
    #[serde(rename = "CAUSAL")]
    Causal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadMetrics {
    pub rate_per_min: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub error_rate: f64,
    pub queue_depth: u64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadEntity {
    pub workload_id: String,
    pub shard_id: String,
    pub state: WorkloadState,
    pub region: Region,
    pub metrics: WorkloadMetrics,
    pub replication: ReplicationPolicy,
    pub locality: LocalityPreference,
    pub lifecycle: LifecycleState,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationPolicy {
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub current_replicas: u32,
    pub consistency_mode: ConsistencyMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalityPreference {
    pub preferred_region: Region,
    pub fallback_regions: Vec<Region>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LifecycleState {
    #[serde(rename = "CREATE")]
    Create,
    #[serde(rename = "WARM")]
    Warm,
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "DRAINING")]
    Draining,
    #[serde(rename = "RETIRED")]
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealth {
    pub node_id: String,
    pub region: Region,
    pub healthy: bool,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub active_workloads: u32,
    pub last_heartbeat: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RiskLevel {
    #[serde(rename = "LOW")]
    Low,
    #[serde(rename = "MEDIUM")]
    Medium,
    #[serde(rename = "HIGH")]
    High,
    #[serde(rename = "CRITICAL")]
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub workload_entities: HashMap<String, WorkloadEntity>,
    pub node_health: HashMap<String, NodeHealth>,
    pub timestamp: DateTime<Utc>,
    pub version: u64,
}
