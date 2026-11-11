use crate::events::PaymentEventType;
use crate::types::{CheckoutSessionData, PaymentGateway, PaymentMethodClass, WorkloadState};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("Provider '{provider}' is not configured: {reason}")]
    NotConfigured { provider: String, reason: String },

    #[error("Authentication failed for provider '{provider}': {reason}")]
    AuthenticationFailed { provider: String, reason: String },

    #[error("Validation failed for provider '{provider}': {reason}")]
    ValidationFailed { provider: String, reason: String },

    #[error("Invalid webhook signature for provider '{provider}': {reason}")]
    InvalidSignature { provider: String, reason: String },

    #[error("Webhook timestamp verification failed for provider '{provider}': {reason}")]
    InvalidWebhookTimestamp { provider: String, reason: String },

    #[error("Webhook payload invalid or malformed for provider '{provider}': {reason}")]
    MalformedWebhook { provider: String, reason: String },

    #[error("Payment not found on provider '{provider}': {payment_id}")]
    PaymentNotFound {
        provider: String,
        payment_id: String,
    },

    #[error("Order not found on provider '{provider}': {order_id}")]
    OrderNotFound { provider: String, order_id: String },

    #[error("Refund not supported or failed on provider '{provider}': {reason}")]
    RefundError { provider: String, reason: String },

    #[error("Upstream gateway error from provider '{provider}' (HTTP {status:?}): {message}")]
    UpstreamError {
        provider: String,
        status: Option<u16>,
        message: String,
    },

    #[error("Network timeout connecting to provider '{provider}': {message}")]
    Timeout { provider: String, message: String },

    #[error("Operation prohibited: {0}")]
    Prohibited(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderParams {
    pub internal_transaction_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub customer_id: String,
    pub customer_email: Option<String>,
    pub customer_phone: Option<String>,
    pub customer_name: Option<String>,
    pub payment_method: Option<PaymentMethodClass>,
    pub return_url: String,
    pub notify_url: Option<String>,
    pub merchant_id: String,
    pub notes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderResult {
    pub provider_order_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub checkout_session: CheckoutSessionData,
    pub raw_response: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDetails {
    pub provider_order_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub status: String,
    pub merchant_id: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub raw_response: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderPaymentStatus {
    Pending,
    Authorized,
    Captured,
    Failed,
    Cancelled,
    Refunded,
    PartiallyRefunded,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentStatusResult {
    pub provider_payment_id: String,
    pub provider_order_id: Option<String>,
    pub status: ProviderPaymentStatus,
    pub amount_minor: u64,
    pub currency: String,
    pub payment_method: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub raw_response: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRefundParams {
    pub provider_payment_id: String,
    pub provider_order_id: Option<String>,
    pub refund_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub reason: Option<String>,
    pub merchant_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundResult {
    pub provider_refund_id: String,
    pub payment_id: String,
    pub amount_minor: u64,
    pub status: String, // "pending", "processed", "failed"
    pub raw_response: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundStatusResult {
    pub provider_refund_id: String,
    pub payment_id: String,
    pub amount_minor: u64,
    pub status: String,
    pub raw_response: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedWebhookEvent {
    pub provider: PaymentGateway,
    pub provider_event_id: String,
    pub event_type: PaymentEventType,
    pub provider_payment_id: String,
    pub provider_order_id: Option<String>,
    pub amount_minor: u64,
    pub currency: String,
    pub refund_id: Option<String>,
    pub refund_amount_minor: Option<u64>,
    pub refund_status: Option<String>,
    pub raw_payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealth {
    pub gateway: PaymentGateway,
    pub name: String,
    pub is_configured: bool,
    pub is_healthy: bool,
    pub is_sandbox: bool,
    pub status: WorkloadState,
    pub p95_latency_ms: f64,
    pub success_rate: f64,
    pub active_traffic_pct: f64,
    pub supported_methods: Vec<PaymentMethodClass>,
    pub message: String,
}

/// Outcome of an atomic refund reservation operation in durable store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefundReservationOutcome {
    /// Newly reserved; returns remaining refundable balance in minor units.
    Reserved { remaining_balance: u64 },
    /// Already reserved under this refund_id with given status and amount (idempotent).
    AlreadyReserved { amount_minor: u64, status: String },
    /// Already settled / processed (idempotent duplicate).
    AlreadyProcessed { amount_minor: u64 },
}

/// Unified, authoritative response contract for payment confirmation across all gateways.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentConfirmationResponse {
    pub success: bool,
    pub payment_state: String,
    pub transaction_id: String,
    pub provider: String,
    pub provider_order_id: Option<String>,
    pub payment_reference: Option<String>,
    pub amount_minor: u64,
    pub currency: String,
    pub idempotent: bool,
    pub message: String,
}

/// Durable record of an initiated payment attempt for merchant ownership and lifecycle verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentAttemptRecord {
    pub transaction_id: String,
    pub merchant_id: String,
    pub provider: String,
    pub provider_order_id: Option<String>,
    pub payment_reference: Option<String>,
    pub amount_minor: u64,
    pub currency: String,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Unified Asynchronous Payment Provider Interface.
/// Authoritative interface for Indian Payment Gateway providers.
#[async_trait]
pub trait PaymentProvider: Send + Sync {
    /// Return the unique gateway provider enum identification.
    fn id(&self) -> PaymentGateway;

    /// User-friendly display name.
    fn name(&self) -> &str;

    /// Whether this provider is configured with credentials in the current environment.
    fn is_configured(&self) -> bool;

    /// Whether this provider is running in test / sandbox mode.
    fn is_sandbox(&self) -> bool;

    /// Payment method classes supported by this provider (UPI, Card, NetBanking, Wallet).
    fn supported_methods(&self) -> &[PaymentMethodClass];

    /// Validate the provider configuration and environment constraints.
    fn validate_config(&self) -> Result<(), ProviderError>;

    /// Create an upstream order / payment session.
    async fn create_order(
        &self,
        params: &CreateOrderParams,
    ) -> Result<CreateOrderResult, ProviderError>;

    /// Fetch order details from the provider.
    async fn get_order(&self, order_id: &str) -> Result<OrderDetails, ProviderError>;

    /// Authoritatively retrieve payment status from the provider.
    async fn get_payment_status(
        &self,
        payment_id: &str,
    ) -> Result<PaymentStatusResult, ProviderError>;

    /// Verify webhook signature and return a normalized webhook event.
    fn verify_webhook(
        &self,
        raw_body: &[u8],
        headers: &HashMap<String, String>,
    ) -> Result<NormalizedWebhookEvent, ProviderError>;

    /// Initiate a refund against a previous transaction.
    async fn create_refund(
        &self,
        params: &CreateRefundParams,
    ) -> Result<RefundResult, ProviderError>;

    /// Retrieve the status of an initiated refund.
    async fn get_refund_status(
        &self,
        refund_id: &str,
        payment_id: Option<&str>,
    ) -> Result<RefundStatusResult, ProviderError>;

    /// Report provider operational health, metrics and configuration state.
    fn health(&self) -> ProviderHealth;
}
