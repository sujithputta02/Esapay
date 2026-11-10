use axum::{http::StatusCode, Json};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter as GovRateLimiter};
use serde_json::json;
use std::num::NonZeroU32;
use std::sync::Arc;

/// Production Token-Bucket Rate Limiter for ESAPay API routes
/// Protects against volumetric spam, bot attacks, and AI compute exhaustion.
#[derive(Clone)]
pub struct ApiRateLimiter {
    checkout_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    keys_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    demo_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl ApiRateLimiter {
    pub fn new() -> Self {
        Self::new_with_quotas(30, 60, 10, 20, 5, 10)
    }

    pub fn new_with_quotas(
        checkout_rps: u32,
        checkout_burst: u32,
        keys_rps: u32,
        keys_burst: u32,
        demo_rps: u32,
        demo_burst: u32,
    ) -> Self {
        // Token-bucket quotas for respective operations
        let checkout_quota = Quota::per_second(NonZeroU32::new(checkout_rps).unwrap())
            .allow_burst(NonZeroU32::new(checkout_burst).unwrap());
        let keys_quota = Quota::per_second(NonZeroU32::new(keys_rps).unwrap())
            .allow_burst(NonZeroU32::new(keys_burst).unwrap());
        let demo_quota = Quota::per_second(NonZeroU32::new(demo_rps).unwrap())
            .allow_burst(NonZeroU32::new(demo_burst).unwrap());

        Self {
            checkout_limiter: Arc::new(GovRateLimiter::direct(checkout_quota)),
            keys_limiter: Arc::new(GovRateLimiter::direct(keys_quota)),
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

    pub fn check_keys(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.keys_limiter.check().is_err() {
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

impl Default for ApiRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}
