use crate::client::PayUClient;
use crate::config::PayUConfig;
use crate::webhook::parse_and_verify_callback;
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

pub struct PayUProvider {
    client: PayUClient,
    latency_sum_ms: AtomicU64,
    latency_count: AtomicU64,
    success_count: AtomicU64,
    failure_count: AtomicU64,
    circuit: RwLock<CircuitStateInternal>,
    failure_threshold: u32,
    cooldown_duration: std::time::Duration,
}

impl PayUProvider {
    pub fn new(config: PayUConfig) -> Self {
        let client = PayUClient::new(config);
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

    /// Build a provider with system proxy discovery disabled (useful in sandboxed test environments).
    pub fn new_without_proxy(config: PayUConfig) -> Self {
        let client = PayUClient::new_without_proxy(config);
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

    pub fn client(&self) -> &PayUClient {
        &self.client
    }

    pub fn config(&self) -> &PayUConfig {
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
                provider: "payu".into(),
                status: Some(503),
                message: "PayU circuit breaker is open due to consecutive upstream failures".into(),
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

const PAYU_SUPPORTED_METHODS: &[PaymentMethodClass] = &[
    PaymentMethodClass::Upi,
    PaymentMethodClass::Card,
    PaymentMethodClass::NetBanking,
    PaymentMethodClass::Wallet,
];

#[async_trait]
impl PaymentProvider for PayUProvider {
    fn id(&self) -> PaymentGateway {
        PaymentGateway::PayU
    }

    fn name(&self) -> &str {
        "PayU India Hosted Checkout"
    }

    fn is_configured(&self) -> bool {
        !self.config().merchant_key.is_empty() && !self.config().merchant_salt.is_empty()
    }

    fn is_sandbox(&self) -> bool {
        self.config().is_test()
    }

    fn supported_methods(&self) -> &[PaymentMethodClass] {
        PAYU_SUPPORTED_METHODS
    }

    fn validate_config(&self) -> Result<(), ProviderError> {
        if self.config().merchant_key.trim().is_empty() {
            return Err(ProviderError::NotConfigured {
                provider: "payu".into(),
                reason: "PAYU_MERCHANT_KEY is missing or empty".into(),
            });
        }
        if self.config().merchant_salt.trim().is_empty() {
            return Err(ProviderError::NotConfigured {
                provider: "payu".into(),
                reason: "PAYU_MERCHANT_SALT is missing or empty".into(),
            });
        }
        Ok(())
    }

    async fn create_order(
        &self,
        params: &CreateOrderParams,
    ) -> Result<CreateOrderResult, ProviderError> {
        self.check_circuit()?;

        let txnid = format!("payu_{}", params.internal_transaction_id.replace('-', ""));
        let txnid = if txnid.len() > 30 {
            txnid[..30].to_string()
        } else {
            txnid
        };

        let productinfo = format!("ESA Payment {}", params.merchant_id);
        let firstname = params.customer_name.as_deref().unwrap_or("Customer");
        let email = params
            .customer_email
            .as_deref()
            .unwrap_or("customer@example.com");
        let phone = params.customer_phone.as_deref().unwrap_or("9999999999");

        let (action_url, form_params) = self.client.build_hosted_checkout(
            &txnid,
            params.amount_minor,
            &productinfo,
            firstname,
            email,
            phone,
            &params.return_url,
            &params.return_url, // failure url redirects to return_url with status=failure
            Some(&params.return_url),
            Some(&params.merchant_id),
            None,
            None,
            None,
            None,
        );

        let raw_resp = serde_json::to_value(&form_params).unwrap_or_default();

        Ok(CreateOrderResult {
            provider_order_id: txnid.clone(),
            amount_minor: params.amount_minor,
            currency: params.currency.clone(),
            checkout_session: CheckoutSessionData::Payu {
                action_url,
                params: form_params,
            },
            raw_response: raw_resp,
        })
    }

    async fn get_order(&self, order_id: &str) -> Result<OrderDetails, ProviderError> {
        let status = self.get_payment_status(order_id).await?;
        let status_str = match status.status {
            ProviderPaymentStatus::Captured => "SUCCESS",
            ProviderPaymentStatus::Failed => "FAILED",
            ProviderPaymentStatus::Pending => "PENDING",
            _ => "UNKNOWN",
        };

        Ok(OrderDetails {
            provider_order_id: order_id.to_string(),
            amount_minor: status.amount_minor,
            currency: status.currency,
            status: status_str.to_string(),
            merchant_id: None,
            created_at: None,
            raw_response: status.raw_response,
        })
    }

    async fn get_payment_status(
        &self,
        payment_id: &str,
    ) -> Result<PaymentStatusResult, ProviderError> {
        self.check_circuit()?;
        let start = Instant::now();

        let res = self.client.verify_payment(payment_id).await;
        let elapsed = start.elapsed().as_millis() as u64;

        match res {
            Ok(body) => {
                self.record_success(elapsed);
                let txn_details = body
                    .get("transaction_details")
                    .and_then(|td| td.get(payment_id))
                    .ok_or_else(|| ProviderError::PaymentNotFound {
                        provider: "payu".into(),
                        payment_id: payment_id.to_string(),
                    })?;

                let status_str = txn_details
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let status = match status_str.to_lowercase().as_str() {
                    "success" => ProviderPaymentStatus::Captured,
                    "failure" | "failed" | "bounced" => ProviderPaymentStatus::Failed,
                    "pending" => ProviderPaymentStatus::Pending,
                    _ => ProviderPaymentStatus::Unknown,
                };

                let amt_str = txn_details
                    .get("transaction_amount")
                    .or_else(|| txn_details.get("amt"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("0.0");
                let amt_f64: f64 = amt_str.parse().unwrap_or(0.0);
                let amount_minor = (amt_f64 * 100.0).round() as u64;

                let mihpayid = txn_details
                    .get("mihpayid")
                    .and_then(|v| v.as_str())
                    .unwrap_or(payment_id)
                    .to_string();

                let mode = txn_details
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let error_msg = txn_details
                    .get("error_Message")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                Ok(PaymentStatusResult {
                    provider_payment_id: mihpayid,
                    provider_order_id: Some(payment_id.to_string()),
                    status,
                    amount_minor,
                    currency: "INR".to_string(),
                    payment_method: mode,
                    error_code: txn_details
                        .get("error_code")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    error_message: error_msg,
                    raw_response: txn_details.clone(),
                })
            }
            Err(e) => {
                self.record_failure(elapsed);
                self.trip_circuit(std::time::Duration::from_secs(10));
                Err(ProviderError::UpstreamError {
                    provider: "payu".into(),
                    status: None,
                    message: e.to_string(),
                })
            }
        }
    }

    fn verify_webhook(
        &self,
        raw_body: &[u8],
        _headers: &HashMap<String, String>,
    ) -> Result<NormalizedWebhookEvent, ProviderError> {
        // Try parsing payload as JSON first, then as urlencoded form
        let params: HashMap<String, String> = if let Ok(json) =
            serde_json::from_slice::<HashMap<String, serde_json::Value>>(raw_body)
        {
            json.into_iter()
                .map(|(k, v)| {
                    let s = if let Some(str_val) = v.as_str() {
                        str_val.to_string()
                    } else {
                        v.to_string().replace('"', "")
                    };
                    (k, s)
                })
                .collect()
        } else if let Ok(form) = serde_urlencoded::from_bytes::<HashMap<String, String>>(raw_body) {
            form
        } else {
            return Err(ProviderError::MalformedWebhook {
                provider: "payu".into(),
                reason: "Failed to parse PayU callback as JSON or urlencoded form".into(),
            });
        };

        parse_and_verify_callback(&params, &self.config().merchant_salt).map_err(|e| match e {
            crate::webhook::PayUWebhookError::InvalidHash => ProviderError::InvalidSignature {
                provider: "payu".into(),
                reason: "PayU reverse hash verification failed".into(),
            },
            crate::webhook::PayUWebhookError::MissingHash => ProviderError::InvalidSignature {
                provider: "payu".into(),
                reason: "Missing hash parameter in PayU callback".into(),
            },
            other => ProviderError::MalformedWebhook {
                provider: "payu".into(),
                reason: other.to_string(),
            },
        })
    }

    async fn create_refund(
        &self,
        params: &CreateRefundParams,
    ) -> Result<RefundResult, ProviderError> {
        self.check_circuit()?;
        let start = Instant::now();

        let mihpayid = &params.provider_payment_id;
        let res = self
            .client
            .create_refund(mihpayid, &params.refund_id, params.amount_minor)
            .await;

        let elapsed = start.elapsed().as_millis() as u64;

        match res {
            Ok(body) => {
                self.record_success(elapsed);
                let status_num = body.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
                let status = if status_num == 1 {
                    "processed"
                } else {
                    "pending"
                };

                let req_id = body
                    .get("request_id")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| params.refund_id.clone());

                Ok(RefundResult {
                    provider_refund_id: req_id,
                    payment_id: params.provider_payment_id.clone(),
                    amount_minor: params.amount_minor,
                    status: status.to_string(),
                    raw_response: body,
                })
            }
            Err(e) => {
                self.record_failure(elapsed);
                Err(ProviderError::RefundError {
                    provider: "payu".into(),
                    reason: e.to_string(),
                })
            }
        }
    }

    async fn get_refund_status(
        &self,
        refund_id: &str,
        _payment_id: Option<&str>,
    ) -> Result<RefundStatusResult, ProviderError> {
        self.check_circuit()?;
        let start = Instant::now();

        let res = self.client.check_refund_status(refund_id).await;
        let elapsed = start.elapsed().as_millis() as u64;

        match res {
            Ok(body) => {
                self.record_success(elapsed);
                let status_num = body.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
                let status = if status_num == 1 {
                    "processed"
                } else {
                    "pending"
                };

                Ok(RefundStatusResult {
                    provider_refund_id: refund_id.to_string(),
                    payment_id: "".to_string(),
                    amount_minor: 0,
                    status: status.to_string(),
                    raw_response: body,
                })
            }
            Err(e) => {
                self.record_failure(elapsed);
                Err(ProviderError::RefundError {
                    provider: "payu".into(),
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
            "PayU credentials unconfigured".to_string()
        } else if cb_state == CircuitBreakerState::Open {
            "PayU circuit open due to upstream errors".to_string()
        } else if cb_state == CircuitBreakerState::HalfOpen {
            "PayU circuit half-open (probing recovery)".to_string()
        } else {
            format!(
                "Operational ({})",
                if self.is_sandbox() {
                    "Test"
                } else {
                    "Production"
                }
            )
        };

        ProviderHealth {
            gateway: PaymentGateway::PayU,
            name: self.name().to_string(),
            is_configured: is_cfg,
            is_healthy,
            is_sandbox: self.is_sandbox(),
            status,
            p95_latency_ms: p95,
            success_rate,
            active_traffic_pct: 0.0,
            supported_methods: PAYU_SUPPORTED_METHODS.to_vec(),
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_provider() -> PayUProvider {
        let cfg = PayUConfig {
            merchant_key: "test_key".to_string(),
            merchant_salt: "test_salt".to_string(),
            payment_url: "https://test.payu.in/_payment".to_string(),
            verify_url: "https://test.payu.in/merchant/postservice?form=2".to_string(),
            env: crate::config::PayUEnv::Test,
        };
        PayUProvider::new_without_proxy(cfg)
    }

    #[test]
    fn test_payu_circuit_breaker_lifecycle() {
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
