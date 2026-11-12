use crate::client::RazorpayClient;
use crate::config::{RazorpayConfig, RazorpayConfigError};
use crate::dedup::EventDeduper;
use crate::rate_limit::RazorpayRateLimiter;
use crate::webhook::{
    parse_webhook_payload_detailed, verify_signature, ParsedWebhook, WebhookError,
    MAX_WEBHOOK_BODY_BYTES,
};
use async_trait::async_trait;
use esa_core::provider::{
    CreateOrderParams, CreateOrderResult, CreateRefundParams, NormalizedWebhookEvent, OrderDetails,
    PaymentProvider, PaymentStatusResult, ProviderError, ProviderHealth, ProviderPaymentStatus,
    RefundResult, RefundStatusResult,
};
use esa_core::types::{CheckoutSessionData, PaymentGateway, PaymentMethodClass, WorkloadState};
use esa_core::PaymentEvent;
use tracing::{info, warn};

/// Payment adapter: webhook validation, deduplication, normalization (PRD §8.2, §9).
pub struct RazorpayAdapter {
    config: RazorpayConfig,
    client: RazorpayClient,
    webhook_limiter: RazorpayRateLimiter,
    deduper: EventDeduper,
}

impl RazorpayAdapter {
    pub fn new(config: RazorpayConfig) -> Self {
        Self::with_config(config)
    }

    pub fn with_config(config: RazorpayConfig) -> Self {
        let webhook_limiter = RazorpayRateLimiter::per_second(config.webhook_rate_limit_per_sec);
        let client = RazorpayClient::new(config.clone());

        Self {
            config,
            client,
            webhook_limiter,
            deduper: EventDeduper::new(),
        }
    }

    pub fn try_from_env() -> Result<Self, RazorpayConfigError> {
        let config = RazorpayConfig::from_env()?;
        Ok(Self::with_config(config))
    }

    pub fn config(&self) -> &RazorpayConfig {
        &self.config
    }

    pub fn client(&self) -> &RazorpayClient {
        &self.client
    }

    pub fn status(&self) -> serde_json::Value {
        self.status_for_caller(false)
    }

    pub fn status_for_caller(&self, is_sandbox_caller: bool) -> serde_json::Value {
        serde_json::json!({
            "razorpay": self.config.public_status_for_caller(is_sandbox_caller),
            "dedupe_cache_size": self.deduper.count(),
        })
    }

    /// Secure webhook verification and payload validation.
    /// Strictly verifies HMAC-SHA256 signature against exact raw body,
    /// enforces maximum body size limit, and extracts provider event ID and payment ID.
    pub fn process_webhook_detailed(
        &self,
        raw_body: &[u8],
        signature_header: Option<&str>,
        event_id_header: Option<&str>,
    ) -> Result<ParsedWebhook, WebhookError> {
        if raw_body.len() > MAX_WEBHOOK_BODY_BYTES {
            warn!(
                "Razorpay webhook payload rejected: size {} exceeds limit of {} bytes",
                raw_body.len(),
                MAX_WEBHOOK_BODY_BYTES
            );
            return Err(WebhookError::PayloadTooLarge(
                raw_body.len(),
                MAX_WEBHOOK_BODY_BYTES,
            ));
        }

        if self.webhook_limiter.check().is_err() {
            warn!("Razorpay webhook rate limit exceeded");
            return Err(WebhookError::RateLimited);
        }

        let signature = signature_header.ok_or(WebhookError::MissingSignature)?;

        if self.config.webhook_secret.trim().is_empty() {
            warn!("Razorpay webhook secret not configured");
            return Err(WebhookError::MissingWebhookSecret);
        }

        let verified = verify_signature(raw_body, signature, &self.config.webhook_secret);

        if !verified {
            warn!("Razorpay webhook signature verification failed");
            return Err(WebhookError::InvalidSignature);
        }

        let parsed = parse_webhook_payload_detailed(
            raw_body,
            event_id_header,
            self.config.account_id.as_deref(),
        )?;

        info!(
            "Razorpay webhook verified: provider_event_id={} payment_id={} event_type={:?}",
            parsed.provider_event_id, parsed.payment_id, parsed.event_type
        );

        Ok(parsed)
    }

    /// Full webhook pipeline: rate limit → signature → parse → dedupe.
    pub fn process_webhook(
        &self,
        raw_body: &[u8],
        signature_header: Option<&str>,
    ) -> Result<PaymentEvent, WebhookError> {
        let parsed = self.process_webhook_detailed(raw_body, signature_header, None)?;

        if self.deduper.is_duplicate(&parsed.provider_event_id) {
            warn!(
                "Duplicate Razorpay event ignored: {}",
                parsed.provider_event_id
            );
            return Err(WebhookError::DuplicateEvent);
        }

        info!(
            "Razorpay webhook accepted: {} ({:?})",
            parsed.provider_event_id, parsed.event_type
        );

        Ok(parsed.payment_event)
    }
}

pub type RazorpayProvider = RazorpayAdapter;

const RAZORPAY_SUPPORTED_METHODS: &[PaymentMethodClass] = &[
    PaymentMethodClass::Upi,
    PaymentMethodClass::Card,
    PaymentMethodClass::NetBanking,
    PaymentMethodClass::Wallet,
];

#[async_trait]
impl PaymentProvider for RazorpayAdapter {
    fn id(&self) -> PaymentGateway {
        PaymentGateway::Razorpay
    }

    fn name(&self) -> &str {
        "Razorpay"
    }

    fn is_configured(&self) -> bool {
        !self.config.key_id.is_empty() && !self.config.key_secret.is_empty()
    }

    fn is_sandbox(&self) -> bool {
        self.config.mode.is_test()
    }

    fn supported_methods(&self) -> &[PaymentMethodClass] {
        RAZORPAY_SUPPORTED_METHODS
    }

    fn validate_config(&self) -> Result<(), ProviderError> {
        if self.config.key_id.trim().is_empty() {
            return Err(ProviderError::NotConfigured {
                provider: "razorpay".into(),
                reason: "RAZORPAY_KEY_ID is missing or empty".into(),
            });
        }
        if self.config.key_secret.trim().is_empty() {
            return Err(ProviderError::NotConfigured {
                provider: "razorpay".into(),
                reason: "RAZORPAY_KEY_SECRET is missing or empty".into(),
            });
        }
        Ok(())
    }

    async fn create_order(
        &self,
        params: &CreateOrderParams,
    ) -> Result<CreateOrderResult, ProviderError> {
        let receipt = format!("esa-{}", params.internal_transaction_id);
        let notes = serde_json::json!({
            "esa_transaction_id": params.internal_transaction_id,
            "esa_merchant_id": params.merchant_id,
        });

        let order = self
            .client
            .create_order(params.amount_minor, &receipt, notes)
            .await
            .map_err(|e| ProviderError::UpstreamError {
                provider: "razorpay".into(),
                status: None,
                message: e.to_string(),
            })?;

        let order_id = order
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        Ok(CreateOrderResult {
            provider_order_id: order_id.clone(),
            amount_minor: params.amount_minor,
            currency: params.currency.clone(),
            checkout_session: CheckoutSessionData::Razorpay {
                order_id,
                key_id: self.config.key_id.clone(),
                amount: params.amount_minor,
                currency: params.currency.clone(),
                notes: serde_json::to_value(&params.notes).unwrap_or_default(),
            },
            raw_response: order,
        })
    }

    async fn get_order(&self, order_id: &str) -> Result<OrderDetails, ProviderError> {
        let order =
            self.client
                .get_order(order_id)
                .await
                .map_err(|e| ProviderError::UpstreamError {
                    provider: "razorpay".into(),
                    status: None,
                    message: e.to_string(),
                })?;

        let amount_minor = order.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
        let currency = order
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("INR")
            .to_string();
        let status = order
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("created")
            .to_string();
        let merchant_id = order
            .get("notes")
            .and_then(|n| n.get("esa_merchant_id"))
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(OrderDetails {
            provider_order_id: order_id.to_string(),
            amount_minor,
            currency,
            status,
            merchant_id,
            created_at: None,
            raw_response: order,
        })
    }

    async fn get_payment_status(
        &self,
        payment_id: &str,
    ) -> Result<PaymentStatusResult, ProviderError> {
        let payment = self.client.get_payment(payment_id).await.map_err(|e| {
            ProviderError::UpstreamError {
                provider: "razorpay".into(),
                status: None,
                message: e.to_string(),
            }
        })?;

        let status_str = payment.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let status = match status_str {
            "captured" => ProviderPaymentStatus::Captured,
            "authorized" => ProviderPaymentStatus::Authorized,
            "failed" => ProviderPaymentStatus::Failed,
            "refunded" => ProviderPaymentStatus::Refunded,
            _ => ProviderPaymentStatus::Unknown,
        };

        let amount_minor = payment.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
        let currency = payment
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("INR")
            .to_string();
        let order_id = payment
            .get("order_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let method = payment
            .get("method")
            .and_then(|v| v.as_str())
            .map(String::from);
        let error_code = payment
            .get("error_code")
            .and_then(|v| v.as_str())
            .map(String::from);
        let error_message = payment
            .get("error_description")
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(PaymentStatusResult {
            provider_payment_id: payment_id.to_string(),
            provider_order_id: order_id,
            status,
            amount_minor,
            currency,
            payment_method: method,
            error_code,
            error_message,
            raw_response: payment,
        })
    }

    fn verify_webhook(
        &self,
        raw_body: &[u8],
        headers: &std::collections::HashMap<String, String>,
    ) -> Result<NormalizedWebhookEvent, ProviderError> {
        let signature = headers
            .get("x-razorpay-signature")
            .or_else(|| headers.get("X-Razorpay-Signature"))
            .map(|s| s.as_str());

        let event_id = headers
            .get("x-razorpay-event-id")
            .or_else(|| headers.get("X-Razorpay-Event-Id"))
            .map(|s| s.as_str());

        let parsed = self
            .process_webhook_detailed(raw_body, signature, event_id)
            .map_err(|e| ProviderError::InvalidSignature {
                provider: "razorpay".into(),
                reason: e.to_string(),
            })?;

        let refund_id = parsed.refund_id.clone();
        let refund_amount_minor = parsed.refund_amount_minor;
        let refund_status = if parsed.refund_id.is_some() {
            Some(parsed.status.clone())
        } else {
            None
        };

        Ok(NormalizedWebhookEvent {
            provider: PaymentGateway::Razorpay,
            provider_event_id: parsed.provider_event_id,
            event_type: parsed.event_type,
            provider_payment_id: parsed.payment_id,
            provider_order_id: parsed.order_id,
            amount_minor: parsed.amount_minor,
            currency: parsed.currency,
            refund_id,
            refund_amount_minor,
            refund_status,
            raw_payload: serde_json::from_slice(raw_body).unwrap_or_default(),
        })
    }

    async fn create_refund(
        &self,
        params: &CreateRefundParams,
    ) -> Result<RefundResult, ProviderError> {
        let notes = serde_json::json!({
            "esa_refund_id": params.refund_id,
            "merchant_id": params.merchant_id,
        });

        let body = self
            .client
            .create_refund(&params.provider_payment_id, params.amount_minor, notes)
            .await
            .map_err(|e| ProviderError::RefundError {
                provider: "razorpay".into(),
                reason: e.to_string(),
            })?;

        let provider_refund_id = body
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or(&params.refund_id)
            .to_string();
        let status = body
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("processed")
            .to_string();

        Ok(RefundResult {
            provider_refund_id,
            payment_id: params.provider_payment_id.clone(),
            amount_minor: params.amount_minor,
            status,
            raw_response: body,
        })
    }

    async fn get_refund_status(
        &self,
        refund_id: &str,
        _payment_id: Option<&str>,
    ) -> Result<RefundStatusResult, ProviderError> {
        let body =
            self.client
                .get_refund(refund_id)
                .await
                .map_err(|e| ProviderError::RefundError {
                    provider: "razorpay".into(),
                    reason: e.to_string(),
                })?;

        let status = body
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("processed")
            .to_string();
        let amount_minor = body.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
        let payment_id = body
            .get("payment_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(RefundStatusResult {
            provider_refund_id: refund_id.to_string(),
            payment_id,
            amount_minor,
            status,
            raw_response: body,
        })
    }

    fn health(&self) -> ProviderHealth {
        let is_cfg = self.is_configured();
        ProviderHealth {
            gateway: PaymentGateway::Razorpay,
            name: self.name().to_string(),
            is_configured: is_cfg,
            is_healthy: is_cfg,
            is_sandbox: self.is_sandbox(),
            status: if is_cfg {
                WorkloadState::Healthy
            } else {
                WorkloadState::Degraded
            },
            p95_latency_ms: 85.0,
            success_rate: if is_cfg { 0.995 } else { 0.0 },
            active_traffic_pct: 0.0,
            supported_methods: RAZORPAY_SUPPORTED_METHODS.to_vec(),
            message: if is_cfg {
                format!(
                    "Operational ({})",
                    if self.is_sandbox() {
                        "Test Mode"
                    } else {
                        "Live"
                    }
                )
            } else {
                "Razorpay credentials unconfigured".into()
            },
        }
    }
}
