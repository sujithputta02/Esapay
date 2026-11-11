use crate::client::CashfreeClient;
use crate::config::CashfreeConfig;
use crate::webhook::parse_webhook;
use async_trait::async_trait;
use esa_core::provider::{
    CreateOrderParams, CreateOrderResult, CreateRefundParams, NormalizedWebhookEvent, OrderDetails,
    PaymentProvider, PaymentStatusResult, ProviderError, ProviderHealth, ProviderPaymentStatus,
    RefundResult, RefundStatusResult,
};
use esa_core::types::{CheckoutSessionData, PaymentGateway, PaymentMethodClass, WorkloadState};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Default)]
struct CircuitStateInternal {
    consecutive_failures: u32,
    open_until: Option<Instant>,
    is_half_open: bool,
}

pub struct CashfreeProvider {
    client: CashfreeClient,
    latency_sum_ms: AtomicU64,
    latency_count: AtomicU64,
    success_count: AtomicU64,
    failure_count: AtomicU64,
    circuit: RwLock<CircuitStateInternal>,
    failure_threshold: u32,
    cooldown_duration: std::time::Duration,
}

impl CashfreeProvider {
    pub fn new(config: CashfreeConfig) -> Self {
        let client = CashfreeClient::new(config);
        Self {
            client,
            latency_sum_ms: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            failure_count: AtomicU64::new(0),
            circuit: RwLock::new(CircuitStateInternal::default()),
            failure_threshold: 3,
            cooldown_duration: std::time::Duration::from_secs(30),
        }
    }

    pub fn client(&self) -> &CashfreeClient {
        &self.client
    }

    pub fn config(&self) -> &CashfreeConfig {
        self.client.config()
    }

    pub fn circuit_state(&self) -> CircuitBreakerState {
        let guard = self.circuit.read();
        match guard.open_until {
            None => CircuitBreakerState::Closed,
            Some(until) => {
                if Instant::now() < until {
                    CircuitBreakerState::Open
                } else {
                    CircuitBreakerState::HalfOpen
                }
            }
        }
    }

    pub fn check_circuit(&self) -> Result<(), ProviderError> {
        let state = self.circuit_state();
        match state {
            CircuitBreakerState::Open => Err(ProviderError::UpstreamError {
                provider: "cashfree".into(),
                status: Some(503),
                message: "Circuit breaker is open due to consecutive upstream failures".into(),
            }),
            CircuitBreakerState::HalfOpen => {
                let mut guard = self.circuit.write();
                guard.is_half_open = true;
                Ok(())
            }
            CircuitBreakerState::Closed => Ok(()),
        }
    }

    pub fn record_success(&self, latency_ms: u64) {
        self.latency_sum_ms.fetch_add(latency_ms, Ordering::Relaxed);
        self.latency_count.fetch_add(1, Ordering::Relaxed);
        self.success_count.fetch_add(1, Ordering::Relaxed);

        let mut guard = self.circuit.write();
        guard.consecutive_failures = 0;
        guard.open_until = None;
        guard.is_half_open = false;
    }

    pub fn record_failure(&self, latency_ms: u64) {
        self.latency_sum_ms.fetch_add(latency_ms, Ordering::Relaxed);
        self.latency_count.fetch_add(1, Ordering::Relaxed);
        self.failure_count.fetch_add(1, Ordering::Relaxed);

        let mut guard = self.circuit.write();
        guard.consecutive_failures += 1;
        if guard.is_half_open || guard.consecutive_failures >= self.failure_threshold {
            guard.open_until = Some(Instant::now() + self.cooldown_duration);
            guard.is_half_open = false;
        }
    }

    pub fn trip_circuit(&self, duration: std::time::Duration) {
        let mut guard = self.circuit.write();
        guard.open_until = Some(Instant::now() + duration);
        guard.is_half_open = false;
    }

    pub fn reset_circuit(&self) {
        let mut guard = self.circuit.write();
        guard.consecutive_failures = 0;
        guard.open_until = None;
        guard.is_half_open = false;
    }
}

const CASHFREE_SUPPORTED_METHODS: &[PaymentMethodClass] = &[
    PaymentMethodClass::Upi,
    PaymentMethodClass::Card,
    PaymentMethodClass::NetBanking,
    PaymentMethodClass::Wallet,
];

#[async_trait]
impl PaymentProvider for CashfreeProvider {
    fn id(&self) -> PaymentGateway {
        PaymentGateway::Cashfree
    }

    fn name(&self) -> &str {
        "Cashfree Payments"
    }

    fn is_configured(&self) -> bool {
        !self.config().client_id.is_empty() && !self.config().client_secret.is_empty()
    }

    fn is_sandbox(&self) -> bool {
        self.config().is_sandbox()
    }

    fn supported_methods(&self) -> &[PaymentMethodClass] {
        CASHFREE_SUPPORTED_METHODS
    }

    fn validate_config(&self) -> Result<(), ProviderError> {
        if self.config().client_id.trim().is_empty() {
            return Err(ProviderError::NotConfigured {
                provider: "cashfree".into(),
                reason: "CASHFREE_CLIENT_ID is missing or empty".into(),
            });
        }
        if self.config().client_secret.trim().is_empty() {
            return Err(ProviderError::NotConfigured {
                provider: "cashfree".into(),
                reason: "CASHFREE_CLIENT_SECRET is missing or empty".into(),
            });
        }
        Ok(())
    }

    async fn create_order(
        &self,
        params: &CreateOrderParams,
    ) -> Result<CreateOrderResult, ProviderError> {
        self.check_circuit()?;
        let start = Instant::now();

        // Cashfree order ID must be alphanumeric and up to 45 chars
        let order_id = format!("cf_{}", params.internal_transaction_id.replace('-', ""));
        let order_id = if order_id.len() > 45 {
            order_id[..45].to_string()
        } else {
            order_id
        };

        let note = format!("ESAPay order for merchant: {}", params.merchant_id);

        let res = self
            .client
            .create_order(
                &order_id,
                params.amount_minor,
                &params.currency,
                &params.customer_id,
                params.customer_email.as_deref(),
                params.customer_phone.as_deref(),
                params.customer_name.as_deref(),
                &params.return_url,
                params.notify_url.as_deref(),
                Some(&note),
            )
            .await;

        let elapsed = start.elapsed().as_millis() as u64;

        match res {
            Ok(body) => {
                self.record_success(elapsed);
                let payment_session_id = body
                    .get("payment_session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ProviderError::UpstreamError {
                        provider: "cashfree".into(),
                        status: None,
                        message: "Cashfree response missing payment_session_id".into(),
                    })?;

                let env_str = if self.is_sandbox() {
                    "sandbox"
                } else {
                    "production"
                };

                Ok(CreateOrderResult {
                    provider_order_id: order_id.clone(),
                    amount_minor: params.amount_minor,
                    currency: params.currency.clone(),
                    checkout_session: CheckoutSessionData::Cashfree {
                        order_id,
                        payment_session_id: payment_session_id.to_string(),
                        environment: env_str.to_string(),
                    },
                    raw_response: body,
                })
            }
            Err(e) => {
                self.record_failure(elapsed);
                self.trip_circuit(std::time::Duration::from_secs(10));
                Err(ProviderError::UpstreamError {
                    provider: "cashfree".into(),
                    status: None,
                    message: e.to_string(),
                })
            }
        }
    }

    async fn get_order(&self, order_id: &str) -> Result<OrderDetails, ProviderError> {
        self.check_circuit()?;
        let start = Instant::now();

        let res = self.client.get_order(order_id).await;
        let elapsed = start.elapsed().as_millis() as u64;

        match res {
            Ok(body) => {
                self.record_success(elapsed);
                let amount_f64 = body
                    .get("order_amount")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let amount_minor = (amount_f64 * 100.0).round() as u64;
                let currency = body
                    .get("order_currency")
                    .and_then(|v| v.as_str())
                    .unwrap_or("INR")
                    .to_string();
                let status = body
                    .get("order_status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNKNOWN")
                    .to_string();

                Ok(OrderDetails {
                    provider_order_id: order_id.to_string(),
                    amount_minor,
                    currency,
                    status,
                    merchant_id: None,
                    created_at: None,
                    raw_response: body,
                })
            }
            Err(e) => {
                self.record_failure(elapsed);
                Err(ProviderError::UpstreamError {
                    provider: "cashfree".into(),
                    status: None,
                    message: e.to_string(),
                })
            }
        }
    }

    async fn get_payment_status(
        &self,
        payment_id: &str,
    ) -> Result<PaymentStatusResult, ProviderError> {
        self.check_circuit()?;
        let start = Instant::now();

        // If payment_id contains order prefix or is order_id, check payments for order
        let payments = self.client.get_order_payments(payment_id).await;
        let elapsed = start.elapsed().as_millis() as u64;

        match payments {
            Ok(list) => {
                self.record_success(elapsed);
                if let Some(payment) = list.first() {
                    let p_status_str = payment
                        .get("payment_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let status = match p_status_str.to_uppercase().as_str() {
                        "SUCCESS" => ProviderPaymentStatus::Captured,
                        "FAILED" => ProviderPaymentStatus::Failed,
                        "USER_DROPPED" | "CANCELLED" => ProviderPaymentStatus::Cancelled,
                        "PENDING" => ProviderPaymentStatus::Pending,
                        _ => ProviderPaymentStatus::Unknown,
                    };

                    let amt_f64 = payment
                        .get("payment_amount")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let amount_minor = (amt_f64 * 100.0).round() as u64;
                    let currency = payment
                        .get("payment_currency")
                        .and_then(|v| v.as_str())
                        .unwrap_or("INR")
                        .to_string();

                    let cf_payment_id = payment
                        .get("cf_payment_id")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| payment_id.to_string());

                    Ok(PaymentStatusResult {
                        provider_payment_id: cf_payment_id,
                        provider_order_id: Some(payment_id.to_string()),
                        status,
                        amount_minor,
                        currency,
                        payment_method: payment.get("payment_method").map(|v| v.to_string()),
                        error_code: payment
                            .get("payment_completion_time")
                            .map(|_| "COMPLETED".into()),
                        error_message: payment
                            .get("payment_message")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        raw_response: payment.clone(),
                    })
                } else {
                    Err(ProviderError::PaymentNotFound {
                        provider: "cashfree".into(),
                        payment_id: payment_id.to_string(),
                    })
                }
            }
            Err(e) => {
                self.record_failure(elapsed);
                Err(ProviderError::UpstreamError {
                    provider: "cashfree".into(),
                    status: None,
                    message: e.to_string(),
                })
            }
        }
    }

    fn verify_webhook(
        &self,
        raw_body: &[u8],
        headers: &HashMap<String, String>,
    ) -> Result<NormalizedWebhookEvent, ProviderError> {
        let signature = headers
            .get("x-webhook-signature")
            .or_else(|| headers.get("X-Webhook-Signature"))
            .ok_or_else(|| ProviderError::InvalidSignature {
                provider: "cashfree".into(),
                reason: "Missing 'x-webhook-signature' header".into(),
            })?;

        let timestamp = headers
            .get("x-webhook-timestamp")
            .or_else(|| headers.get("X-Webhook-Timestamp"))
            .ok_or_else(|| ProviderError::InvalidWebhookTimestamp {
                provider: "cashfree".into(),
                reason: "Missing 'x-webhook-timestamp' header".into(),
            })?;

        parse_webhook(raw_body, timestamp, signature, &self.config().client_secret).map_err(|e| {
            match e {
                crate::webhook::CashfreeWebhookError::InvalidSignature => {
                    ProviderError::InvalidSignature {
                        provider: "cashfree".into(),
                        reason: "Signature mismatch".into(),
                    }
                }
                crate::webhook::CashfreeWebhookError::MissingSignature => {
                    ProviderError::InvalidSignature {
                        provider: "cashfree".into(),
                        reason: "Missing signature".into(),
                    }
                }
                crate::webhook::CashfreeWebhookError::MissingTimestamp => {
                    ProviderError::InvalidWebhookTimestamp {
                        provider: "cashfree".into(),
                        reason: "Missing timestamp".into(),
                    }
                }
                other => ProviderError::MalformedWebhook {
                    provider: "cashfree".into(),
                    reason: other.to_string(),
                },
            }
        })
    }

    async fn create_refund(
        &self,
        params: &CreateRefundParams,
    ) -> Result<RefundResult, ProviderError> {
        self.check_circuit()?;
        let start = Instant::now();

        let order_id = params
            .provider_order_id
            .as_deref()
            .unwrap_or(&params.provider_payment_id);

        let res = self
            .client
            .create_refund(
                order_id,
                &params.refund_id,
                params.amount_minor,
                params.reason.as_deref(),
            )
            .await;

        let elapsed = start.elapsed().as_millis() as u64;

        match res {
            Ok(body) => {
                self.record_success(elapsed);
                let cf_refund_id = body
                    .get("cf_refund_id")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| params.refund_id.clone());
                let status = body
                    .get("refund_status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("PENDING")
                    .to_lowercase();

                Ok(RefundResult {
                    provider_refund_id: cf_refund_id,
                    payment_id: params.provider_payment_id.clone(),
                    amount_minor: params.amount_minor,
                    status,
                    raw_response: body,
                })
            }
            Err(e) => {
                self.record_failure(elapsed);
                Err(ProviderError::RefundError {
                    provider: "cashfree".into(),
                    reason: e.to_string(),
                })
            }
        }
    }

    async fn get_refund_status(
        &self,
        refund_id: &str,
        payment_id: Option<&str>,
    ) -> Result<RefundStatusResult, ProviderError> {
        self.check_circuit()?;
        let start = Instant::now();

        let order_id = payment_id.unwrap_or("");
        let res = self.client.get_refund(order_id, refund_id).await;
        let elapsed = start.elapsed().as_millis() as u64;

        match res {
            Ok(body) => {
                self.record_success(elapsed);
                let amt_f64 = body
                    .get("refund_amount")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let amount_minor = (amt_f64 * 100.0).round() as u64;
                let status = body
                    .get("refund_status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("PENDING")
                    .to_lowercase();

                Ok(RefundStatusResult {
                    provider_refund_id: refund_id.to_string(),
                    payment_id: order_id.to_string(),
                    amount_minor,
                    status,
                    raw_response: body,
                })
            }
            Err(e) => {
                self.record_failure(elapsed);
                Err(ProviderError::RefundError {
                    provider: "cashfree".into(),
                    reason: e.to_string(),
                })
            }
        }
    }

    fn health(&self) -> ProviderHealth {
        let is_cfg = self.is_configured();
        let cb_state = self.circuit_state();
        let is_healthy = is_cfg && cb_state != CircuitBreakerState::Open;

        let count = self.latency_count.load(Ordering::Relaxed);
        let p95 = if count > 0 {
            (self.latency_sum_ms.load(Ordering::Relaxed) as f64) / (count as f64)
        } else {
            0.0
        };

        let successes = self.success_count.load(Ordering::Relaxed);
        let failures = self.failure_count.load(Ordering::Relaxed);
        let total = successes + failures;
        let success_rate = if total > 0 {
            (successes as f64) / (total as f64)
        } else if is_cfg {
            1.0
        } else {
            0.0
        };

        let status = if !is_cfg || cb_state == CircuitBreakerState::Open {
            WorkloadState::Degraded
        } else {
            WorkloadState::Healthy
        };

        let message = if !is_cfg {
            "Cashfree credentials unconfigured".to_string()
        } else if cb_state == CircuitBreakerState::Open {
            "Cashfree circuit open due to upstream errors".to_string()
        } else if cb_state == CircuitBreakerState::HalfOpen {
            "Cashfree circuit half-open (probing recovery)".to_string()
        } else {
            format!(
                "Operational ({})",
                if self.is_sandbox() {
                    "Sandbox"
                } else {
                    "Production"
                }
            )
        };

        ProviderHealth {
            gateway: PaymentGateway::Cashfree,
            name: self.name().to_string(),
            is_configured: is_cfg,
            is_healthy,
            is_sandbox: self.is_sandbox(),
            status,
            p95_latency_ms: p95,
            success_rate,
            active_traffic_pct: 0.0,
            supported_methods: CASHFREE_SUPPORTED_METHODS.to_vec(),
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_provider() -> CashfreeProvider {
        let cfg = CashfreeConfig {
            client_id: "test_client_id".to_string(),
            client_secret: "test_client_secret".to_string(),
            api_version: "2023-08-01".to_string(),
            api_base: "https://sandbox.cashfree.com/pg".to_string(),
            env: crate::config::CashfreeEnv::Sandbox,
        };
        CashfreeProvider::new(cfg)
    }

    #[test]
    fn test_circuit_breaker_lifecycle() {
        let provider = create_test_provider();

        // 1. Initial closed state
        assert_eq!(provider.circuit_state(), CircuitBreakerState::Closed);
        assert!(provider.check_circuit().is_ok());
        assert!(provider.health().is_healthy);

        // 2. Failure threshold trips to Open
        provider.record_failure(100);
        provider.record_failure(100);
        assert_eq!(provider.circuit_state(), CircuitBreakerState::Closed);
        provider.record_failure(100); // 3rd failure trips
        assert_eq!(provider.circuit_state(), CircuitBreakerState::Open);
        assert!(provider.check_circuit().is_err());
        assert!(!provider.health().is_healthy);

        // 3. Cooldown with short duration for test
        provider.trip_circuit(Duration::from_millis(50));
        assert_eq!(provider.circuit_state(), CircuitBreakerState::Open);
        std::thread::sleep(Duration::from_millis(60));

        // 4. Half-Open after cooldown expires
        assert_eq!(provider.circuit_state(), CircuitBreakerState::HalfOpen);
        assert!(provider.health().is_healthy);
        assert!(provider.check_circuit().is_ok());

        // 5. Successful probe restores to Closed
        provider.record_success(50);
        assert_eq!(provider.circuit_state(), CircuitBreakerState::Closed);
        assert!(provider.health().is_healthy);

        // 6. Repeated failure in Half-Open immediately trips back to Open
        provider.trip_circuit(Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(15));
        assert_eq!(provider.circuit_state(), CircuitBreakerState::HalfOpen);
        assert!(provider.check_circuit().is_ok());
        provider.record_failure(100);
        assert_eq!(provider.circuit_state(), CircuitBreakerState::Open);
        assert!(!provider.health().is_healthy);
    }
}
