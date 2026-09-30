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
    #[allow(dead_code)]
    keys_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    demo_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl ApiRateLimiter {
    pub fn new() -> Self {
        // 30 requests/second for checkout transactions (burst up to 60)
        let checkout_quota = Quota::per_second(NonZeroU32::new(30).unwrap())
            .allow_burst(NonZeroU32::new(60).unwrap());
        // 10 requests/second for API key generation & validation
        let keys_quota = Quota::per_second(NonZeroU32::new(10).unwrap())
            .allow_burst(NonZeroU32::new(20).unwrap());
        // 5 requests/second for high-compute demo scenarios & benchmarks
        let demo_quota = Quota::per_second(NonZeroU32::new(5).unwrap())
            .allow_burst(NonZeroU32::new(10).unwrap());

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

    #[allow(dead_code)]
    pub fn check_keys(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.keys_limiter.check().is_err() {
            Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "API key operation rate limit exceeded. Too many key requests."
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
