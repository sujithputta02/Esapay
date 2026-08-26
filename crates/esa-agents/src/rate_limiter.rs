use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter as GovRateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;

/// Rate limiter for AI agent requests
pub struct AgentRateLimiter {
    limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl AgentRateLimiter {
    pub fn new(requests_per_second: u32) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(requests_per_second).unwrap());
        let limiter = GovRateLimiter::direct(quota);

        Self {
            limiter: Arc::new(limiter),
        }
    }

    pub async fn check(&self) -> Result<(), String> {
        match self.limiter.check() {
            Ok(_) => Ok(()),
            Err(_) => Err("Rate limit exceeded".to_string()),
        }
    }

    pub async fn wait(&self) {
        self.limiter.until_ready().await;
    }
}

impl Clone for AgentRateLimiter {
    fn clone(&self) -> Self {
        Self {
            limiter: Arc::clone(&self.limiter),
        }
    }
}
