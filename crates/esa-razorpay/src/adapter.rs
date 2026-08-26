use crate::client::RazorpayClient;
use crate::config::{RazorpayConfig, RazorpayConfigError};
use crate::dedup::EventDeduper;
use crate::rate_limit::RazorpayRateLimiter;
use crate::webhook::{parse_webhook_payload, verify_signature, verify_signature_any, WebhookError};
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
    pub fn try_from_env() -> Result<Self, RazorpayConfigError> {
        let config = RazorpayConfig::from_env()?;
        let webhook_limiter =
            RazorpayRateLimiter::per_second(config.webhook_rate_limit_per_sec);
        let client = RazorpayClient::new(config.clone());

        Ok(Self {
            config,
            client,
            webhook_limiter,
            deduper: EventDeduper::new(),
        })
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

    /// Full webhook pipeline: rate limit → signature → parse → dedupe.
    pub fn process_webhook(
        &self,
        raw_body: &[u8],
        signature_header: Option<&str>,
    ) -> Result<PaymentEvent, WebhookError> {
        if self.webhook_limiter.check().is_err() {
            warn!("Razorpay webhook rate limit exceeded");
            return Err(WebhookError::RateLimited);
        }

        let signature = signature_header.ok_or(WebhookError::MissingSignature)?;

        let verified = if self.config.test_mode_only {
            verify_signature_any(
                raw_body,
                signature,
                &self.config.webhook_secret,
                &self.config.key_secret,
            )
        } else {
            verify_signature(raw_body, signature, &self.config.webhook_secret)
        };

        if !verified {
            warn!("Razorpay webhook signature verification failed");
            return Err(WebhookError::InvalidSignature);
        }

        let event = parse_webhook_payload(raw_body)?;

        if self.deduper.is_duplicate(&event.event_id) {
            warn!("Duplicate Razorpay event ignored: {}", event.event_id);
            return Err(WebhookError::DuplicateEvent);
        }

        info!(
            "Razorpay webhook accepted: {} ({:?})",
            event.event_id,
            event.event_type
        );

        Ok(event)
    }
}
