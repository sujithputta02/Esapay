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

pub struct PayUProvider {
    client: PayUClient,
    latency_sum_ms: AtomicU64,
    latency_count: AtomicU64,
    success_count: AtomicU64,
    failure_count: AtomicU64,
    circuit_broken_until: RwLock<Option<Instant>>,
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
            circuit_broken_until: RwLock::new(None),
        }
    }

    pub fn client(&self) -> &PayUClient {
        &self.client
    }

    pub fn config(&self) -> &PayUConfig {
        self.client.config()
    }

    fn record_metric(&self, latency_ms: u64, success: bool) {
        self.latency_sum_ms.fetch_add(latency_ms, Ordering::Relaxed);
        self.latency_count.fetch_add(1, Ordering::Relaxed);
        if success {
            self.success_count.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failure_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn check_circuit(&self) -> Result<(), ProviderError> {
        let guard = self.circuit_broken_until.read();
        if let Some(until) = *guard {
            if Instant::now() < until {
                return Err(ProviderError::UpstreamError {
                    provider: "payu".into(),
                    status: Some(503),
                    message: "PayU circuit breaker is open due to consecutive upstream failures"
                        .into(),
                });
            }
        }
        Ok(())
    }

    fn trip_circuit(&self, duration: std::time::Duration) {
        let mut guard = self.circuit_broken_until.write();
        *guard = Some(Instant::now() + duration);
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
                self.record_metric(elapsed, true);
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
                self.record_metric(elapsed, false);
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
                self.record_metric(elapsed, true);
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
                self.record_metric(elapsed, false);
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
                self.record_metric(elapsed, true);
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
                self.record_metric(elapsed, false);
                Err(ProviderError::RefundError {
                    provider: "payu".into(),
                    reason: e.to_string(),
                })
            }
        }
    }

    fn health(&self) -> ProviderHealth {
        let is_cfg = self.is_configured();
        let is_cb_open = self.circuit_broken_until.read().is_some();

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

        let status = if !is_cfg || is_cb_open {
            WorkloadState::Degraded
        } else {
            WorkloadState::Healthy
        };

        let message = if !is_cfg {
            "PayU credentials unconfigured".to_string()
        } else if is_cb_open {
            "PayU circuit open due to upstream errors".to_string()
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
            is_healthy: is_cfg && !is_cb_open,
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
