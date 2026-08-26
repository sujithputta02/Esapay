use governor::{Quota, RateLimiter as GovRateLimiter};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use std::num::NonZeroU32;
use std::sync::Arc;

/// Token-bucket rate limiter for Razorpay inbound webhooks and outbound API calls.
pub struct RazorpayRateLimiter {
    limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl RazorpayRateLimiter {
    pub fn per_second(requests_per_second: u32) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(requests_per_second.max(1)).unwrap());
        Self {
            limiter: Arc::new(GovRateLimiter::direct(quota)),
        }
    }

    pub fn check(&self) -> Result<(), String> {
        match self.limiter.check() {
            Ok(_) => Ok(()),
            Err(_) => Err("Razorpay rate limit exceeded".to_string()),
        }
    }

    pub async fn wait(&self) {
        self.limiter.until_ready().await;
    }
}

impl Clone for RazorpayRateLimiter {
    fn clone(&self) -> Self {
        Self {
            limiter: Arc::clone(&self.limiter),
        }
    }
}
