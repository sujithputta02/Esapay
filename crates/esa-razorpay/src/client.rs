use crate::config::RazorpayConfig;
use crate::rate_limit::RazorpayRateLimiter;
use anyhow::Result;
use reqwest::Client;
use std::time::Duration;
use tracing::info;

/// Outbound Razorpay REST client (Test/Live API keys via Basic auth).
pub struct RazorpayClient {
    http: Client,
    config: RazorpayConfig,
    rate_limiter: RazorpayRateLimiter,
}

impl RazorpayClient {
    pub fn new(config: RazorpayConfig) -> Self {
        let rate_limiter =
            RazorpayRateLimiter::per_second(config.api_rate_limit_per_sec);

        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build Razorpay HTTP client");

        Self {
            http,
            config,
            rate_limiter,
        }
    }

    pub fn config(&self) -> &RazorpayConfig {
        &self.config
    }

    /// Fetch a payment by ID (rate-limited). Returns JSON entity for debugging/status.
    pub async fn get_payment(&self, payment_id: &str) -> Result<serde_json::Value> {
        self.rate_limiter.wait().await;

        let url = format!("{}/payments/{}", self.config.api_base, payment_id);
        let response = self
            .http
            .get(&url)
            .basic_auth(&self.config.key_id, Some(&self.config.key_secret))
            .header("User-Agent", "ESA-Razorpay-Adapter/0.1")
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Razorpay API error: HTTP {}", response.status());
        }

        let body = response.json().await?;
        info!("Fetched Razorpay payment {}", payment_id);
        Ok(body)
    }

    pub async fn health_check(&self) -> Result<bool> {
        self.rate_limiter.wait().await;
        let url = format!("{}/payments?count=1", self.config.api_base);
        let response = self
            .http
            .get(&url)
            .basic_auth(&self.config.key_id, Some(&self.config.key_secret))
            .send()
            .await?;
        Ok(response.status().is_success())
    }

    /// Create a Razorpay order for Checkout (amount in paise).
    pub async fn create_order(
        &self,
        amount_paise: u64,
        receipt: &str,
        notes: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.rate_limiter.wait().await;

        let url = format!("{}/orders", self.config.api_base);
        let payload = serde_json::json!({
            "amount": amount_paise,
            "currency": "INR",
            "receipt": receipt,
            "notes": notes,
        });

        let response = self
            .http
            .post(&url)
            .basic_auth(&self.config.key_id, Some(&self.config.key_secret))
            .header("User-Agent", "ESA-Razorpay-Adapter/0.1")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Razorpay order error: HTTP {} — {}", status, body);
        }

        let body = response.json().await?;
        info!("Created Razorpay order for receipt {}", receipt);
        Ok(body)
    }
}
