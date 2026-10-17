use crate::client::RazorpayClient;
use crate::config::{RazorpayConfig, RazorpayConfigError};
use crate::dedup::EventDeduper;
use crate::rate_limit::RazorpayRateLimiter;
use crate::webhook::{
    parse_webhook_payload_detailed, verify_signature, ParsedWebhook, WebhookError,
    MAX_WEBHOOK_BODY_BYTES,
};
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
        serde_json::json!({
            "razorpay": self.config.public_status(),
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
