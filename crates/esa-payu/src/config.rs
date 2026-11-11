use std::env;
use std::fmt;
use thiserror::Error;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PayUEnv {
    Test,
    Production,
}

impl PayUEnv {
    pub fn parse(raw: &str) -> Result<Self, PayUConfigError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "test" | "sandbox" | "staging" | "dev" | "uat" | "local" => Ok(PayUEnv::Test),
            "production" | "prod" | "live" => Ok(PayUEnv::Production),
            other => Err(PayUConfigError::InvalidEnv(other.to_string())),
        }
    }

    pub fn is_test(&self) -> bool {
        matches!(self, PayUEnv::Test)
    }

    pub fn is_production(&self) -> bool {
        matches!(self, PayUEnv::Production)
    }
}

#[derive(Clone)]
pub struct PayUConfig {
    pub merchant_key: String,
    pub merchant_salt: String,
    pub env: PayUEnv,
    pub payment_url: String,
    pub verify_url: String,
}

impl fmt::Debug for PayUConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PayUConfig")
            .field("merchant_key", &self.merchant_key)
            .field("merchant_salt", &"[REDACTED]")
            .field("env", &self.env)
            .field("payment_url", &self.payment_url)
            .field("verify_url", &self.verify_url)
            .finish()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PayUConfigError {
    #[error("Missing PayU environment variable: {0}")]
    MissingVar(String),

    #[error("Incomplete PayU configuration: both PAYU_MERCHANT_KEY and PAYU_MERCHANT_SALT must be set if PayU is enabled")]
    IncompleteConfig,

    #[error("Invalid PayU environment '{0}': expected 'test' or 'production'")]
    InvalidEnv(String),
}

impl PayUConfig {
    pub const TEST_PAYMENT_URL: &'static str = "https://test.payu.in/_payment";
    pub const PRODUCTION_PAYMENT_URL: &'static str = "https://secure.payu.in/_payment";

    pub const TEST_VERIFY_URL: &'static str = "https://test.payu.in/merchant/postservice?form=2";
    pub const PRODUCTION_VERIFY_URL: &'static str =
        "https://info.payu.in/merchant/postservice?form=2";

    /// Load PayU configuration from environment variables.
    /// If no PayU variables are set, returns Ok(None).
    pub fn from_env() -> Result<Option<Self>, PayUConfigError> {
        let key_raw = env::var("PAYU_MERCHANT_KEY").ok();
        let salt_raw = env::var("PAYU_MERCHANT_SALT").ok();

        let key = key_raw.map(clean_secret).filter(|s| !s.is_empty());
        let salt = salt_raw.map(clean_secret).filter(|s| !s.is_empty());

        match (key, salt) {
            (None, None) => Ok(None),
            (Some(_), None) | (None, Some(_)) => Err(PayUConfigError::IncompleteConfig),
            (Some(merchant_key), Some(merchant_salt)) => {
                let env_raw = env::var("PAYU_ENV")
                    .or_else(|_| env::var("PAYU_MODE"))
                    .unwrap_or_else(|_| "test".to_string());
                let env = PayUEnv::parse(&env_raw)?;

                let payment_url = if env.is_test() {
                    env::var("PAYU_PAYMENT_URL")
                        .unwrap_or_else(|_| Self::TEST_PAYMENT_URL.to_string())
                } else {
                    env::var("PAYU_PAYMENT_URL")
                        .unwrap_or_else(|_| Self::PRODUCTION_PAYMENT_URL.to_string())
                };

                let verify_url = if env.is_test() {
                    env::var("PAYU_VERIFY_URL")
                        .unwrap_or_else(|_| Self::TEST_VERIFY_URL.to_string())
                } else {
                    env::var("PAYU_VERIFY_URL")
                        .unwrap_or_else(|_| Self::PRODUCTION_VERIFY_URL.to_string())
                };

                Ok(Some(Self {
                    merchant_key,
                    merchant_salt,
                    env,
                    payment_url,
                    verify_url,
                }))
            }
        }
    }

    pub fn is_test(&self) -> bool {
        self.env.is_test()
    }

    pub fn public_status(&self, is_sandbox_caller: bool) -> serde_json::Value {
        let prefix: String = self.merchant_key.chars().take(8).collect();
        let display_key = if is_sandbox_caller || self.env.is_test() {
            self.merchant_key.clone()
        } else {
            format!("{}...", prefix)
        };

        serde_json::json!({
            "configured": true,
            "environment": if self.env.is_test() { "test" } else { "production" },
            "merchant_key": display_key,
        })
    }
}
