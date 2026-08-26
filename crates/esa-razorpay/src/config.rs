use std::env;

fn clean_secret(value: String) -> String {
    let trimmed = value.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        trimmed[1..trimmed.len() - 1].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct RazorpayConfig {
    pub key_id: String,
    pub key_secret: String,
    pub webhook_secret: String,
    pub api_base: String,
    pub test_mode_only: bool,
    pub webhook_rate_limit_per_sec: u32,
    pub api_rate_limit_per_sec: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum RazorpayConfigError {
    #[error("missing {0} — set it in .env (see .env.example)")]
    MissingVar(String),
    #[error("live Razorpay keys are not allowed when RAZORPAY_MODE=test")]
    LiveKeysInTestMode,
}

impl RazorpayConfig {
    /// Load from environment (.env loaded by the API binary via dotenvy).
    pub fn from_env() -> Result<Self, RazorpayConfigError> {
        let key_id = env::var("RAZORPAY_KEY_ID")
            .map_err(|_| RazorpayConfigError::MissingVar("RAZORPAY_KEY_ID".into()))?;
        let key_secret = env::var("RAZORPAY_KEY_SECRET")
            .map_err(|_| RazorpayConfigError::MissingVar("RAZORPAY_KEY_SECRET".into()))?;
        let webhook_secret = env::var("RAZORPAY_WEBHOOK_SECRET")
            .map_err(|_| RazorpayConfigError::MissingVar("RAZORPAY_WEBHOOK_SECRET".into()))?;

        let key_id = clean_secret(key_id);
        let key_secret = clean_secret(key_secret);
        let webhook_secret = clean_secret(webhook_secret);

        let mode = env::var("RAZORPAY_MODE").unwrap_or_else(|_| "test".to_string());
        let test_mode_only = mode.eq_ignore_ascii_case("test");

        if test_mode_only && !key_id.starts_with("rzp_test_") {
            return Err(RazorpayConfigError::LiveKeysInTestMode);
        }

        let api_base = env::var("RAZORPAY_API_BASE")
            .unwrap_or_else(|_| "https://api.razorpay.com/v1".to_string());

        let webhook_rate_limit_per_sec = env::var("RAZORPAY_WEBHOOK_RATE_LIMIT_PER_SEC")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let api_rate_limit_per_sec = env::var("RAZORPAY_API_RATE_LIMIT_PER_SEC")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        Ok(Self {
            key_id,
            key_secret,
            webhook_secret,
            api_base,
            test_mode_only,
            webhook_rate_limit_per_sec,
            api_rate_limit_per_sec,
        })
    }

    pub fn is_configured() -> bool {
        env::var("RAZORPAY_KEY_ID").is_ok()
            && env::var("RAZORPAY_KEY_SECRET").is_ok()
            && env::var("RAZORPAY_WEBHOOK_SECRET").is_ok()
    }

    pub fn public_status(&self) -> serde_json::Value {
        serde_json::json!({
            "enabled": true,
            "mode": if self.test_mode_only { "test" } else { "live" },
            "key_id": self.key_id,
            "key_id_prefix": self.key_id.chars().take(12).collect::<String>() + "...",
            "webhook_rate_limit_per_sec": self.webhook_rate_limit_per_sec,
            "api_rate_limit_per_sec": self.api_rate_limit_per_sec,
        })
    }
}
