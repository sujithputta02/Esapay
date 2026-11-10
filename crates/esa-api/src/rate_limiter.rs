use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter as GovRateLimiter};
use serde_json::json;
use std::num::NonZeroU32;
use std::sync::Arc;

/// Type alias for rate limit rejection responses
pub type RateLimitError = (
    StatusCode,
    [(axum::http::header::HeaderName, &'static str); 1],
    Json<serde_json::Value>,
);

/// Production Token-Bucket Rate Limiter for ESAPay API routes.
/// Combines service-wide aggregate circuit breaking with per-client source rate limiting.
/// Protects against volumetric spam, bot attacks, and AI compute exhaustion.
#[derive(Clone)]
pub struct ApiRateLimiter {
    checkout_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    aggregate_keys_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    client_gen_limiter: Arc<GovRateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>,
    client_val_limiter: Arc<GovRateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>,
    demo_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl ApiRateLimiter {
    pub fn new() -> Self {
        Self::new_with_quotas(30, 60, 20, 40, 5, 10)
    }

    pub fn new_with_quotas(
        checkout_rps: u32,
        checkout_burst: u32,
        keys_rps: u32,
        keys_burst: u32,
        demo_rps: u32,
        demo_burst: u32,
    ) -> Self {
        let checkout_quota = Quota::per_second(NonZeroU32::new(checkout_rps).unwrap())
            .allow_burst(NonZeroU32::new(checkout_burst).unwrap());
        let aggregate_keys_quota = Quota::per_second(NonZeroU32::new(keys_rps).unwrap())
            .allow_burst(NonZeroU32::new(keys_burst).unwrap());
        // Per-client quotas for key generation (strict) and validation
        let client_gen_quota = Quota::per_second(NonZeroU32::new(keys_rps.max(1)).unwrap())
            .allow_burst(NonZeroU32::new(keys_burst.max(1)).unwrap());
        let client_val_quota =
            Quota::per_second(NonZeroU32::new(keys_rps.saturating_mul(2).max(1)).unwrap())
                .allow_burst(NonZeroU32::new(keys_burst.saturating_mul(2).max(1)).unwrap());
        let demo_quota = Quota::per_second(NonZeroU32::new(demo_rps).unwrap())
            .allow_burst(NonZeroU32::new(demo_burst).unwrap());

        Self {
            checkout_limiter: Arc::new(GovRateLimiter::direct(checkout_quota)),
            aggregate_keys_limiter: Arc::new(GovRateLimiter::direct(aggregate_keys_quota)),
            client_gen_limiter: Arc::new(GovRateLimiter::keyed(client_gen_quota)),
            client_val_limiter: Arc::new(GovRateLimiter::keyed(client_val_quota)),
            demo_limiter: Arc::new(GovRateLimiter::direct(demo_quota)),
        }
    }

    pub fn check_checkout(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.checkout_limiter.check().is_err() {
            Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Payment checkout rate limit exceeded. Please back off and retry shortly."
                })),
            ))
        } else {
            Ok(())
        }
    }

    /// Aggregate check for API key operations
    #[allow(dead_code)]
    pub fn check_keys(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.aggregate_keys_limiter.check().is_err() {
            Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "API key operation rate limit exceeded. Too many requests, please back off and retry shortly."
                })),
            ))
        } else {
            Ok(())
        }
    }

    /// Per-source rate limit check for sandbox key generation with aggregate safeguard
    pub fn check_generate_sandbox(&self, client_key: &str) -> Result<(), RateLimitError> {
        if self.aggregate_keys_limiter.check().is_err() {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, "1")],
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Aggregate service-wide API key rate limit exceeded. Please back off and retry shortly."
                })),
            ));
        }

        if self
            .client_gen_limiter
            .check_key(&client_key.to_string())
            .is_err()
        {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, "1")],
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Sandbox API key generation rate limit exceeded for this client. Please back off and retry shortly."
                })),
            ));
        }

        Ok(())
    }

    /// Per-source rate limit check for key validation with aggregate safeguard
    pub fn check_validate_key(&self, client_key: &str) -> Result<(), RateLimitError> {
        if self.aggregate_keys_limiter.check().is_err() {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, "1")],
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Aggregate service-wide API key rate limit exceeded. Please back off and retry shortly."
                })),
            ));
        }

        if self
            .client_val_limiter
            .check_key(&client_key.to_string())
            .is_err()
        {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, "1")],
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "API key validation rate limit exceeded for this client. Please back off and retry shortly."
                })),
            ));
        }

        Ok(())
    }

    pub fn check_demo(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.demo_limiter.check().is_err() {
            Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Scenario simulation rate limit exceeded. Please allow current workload to stabilize."
                })),
            ))
        } else {
            Ok(())
        }
    }
}

/// Safely extract client source identifier without trusting spoofable X-Forwarded-For headers.
pub fn extract_client_source_identifier(headers: &HeaderMap) -> String {
    // Only trust reverse proxy headers if explicitly enabled by server configuration
    if std::env::var("TRUSTED_PROXIES")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
    {
        if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(client_ip) = forwarded
                .split(',')
                .next()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                return format!("proxy_ip:{}", client_ip);
            }
        }
        if let Some(cf_ip) = headers
            .get("cf-connecting-ip")
            .and_then(|v| v.to_str().ok())
        {
            let clean = cf_ip.trim();
            if !clean.is_empty() {
                return format!("cf_ip:{}", clean);
            }
        }
    }

    // If an authorization token or API key header is present, key on its hash
    if let Some(token) = crate::api_keys::extract_token(headers) {
        return format!("auth:{}", crate::api_keys::hash_api_key(&token));
    }

    // Default source bucket for unauthenticated clients when no trusted proxy is configured
    "default_client".to_string()
}

impl Default for ApiRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}
