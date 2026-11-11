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
pub enum CashfreeEnv {
    Sandbox,
    Production,
}

impl CashfreeEnv {
    pub fn parse(raw: &str) -> Result<Self, CashfreeConfigError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "sandbox" | "test" | "staging" | "dev" | "uat" | "local" => Ok(CashfreeEnv::Sandbox),
            "production" | "prod" | "live" => Ok(CashfreeEnv::Production),
            other => Err(CashfreeConfigError::InvalidEnv(other.to_string())),
        }
    }

    pub fn is_sandbox(&self) -> bool {
        matches!(self, CashfreeEnv::Sandbox)
    }

    pub fn is_production(&self) -> bool {
        matches!(self, CashfreeEnv::Production)
    }
}

#[derive(Clone)]
pub struct CashfreeConfig {
    pub client_id: String,
    pub client_secret: String,
    pub api_version: String,
    pub env: CashfreeEnv,
    pub api_base: String,
}

impl fmt::Debug for CashfreeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let masked_secret = if self.client_secret.len() > 6 {
            format!("{}...[REDACTED]", &self.client_secret[..3])
        } else {
            "[REDACTED]".to_string()
        };
        f.debug_struct("CashfreeConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &masked_secret)
            .field("api_version", &self.api_version)
            .field("env", &self.env)
            .field("api_base", &self.api_base)
            .finish()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CashfreeConfigError {
    #[error("Missing Cashfree environment variable: {0}")]
    MissingVar(String),

    #[error("Incomplete Cashfree configuration: both CASHFREE_CLIENT_ID and CASHFREE_CLIENT_SECRET must be set if Cashfree is enabled")]
    IncompleteConfig,

    #[error("Invalid Cashfree environment '{0}': expected 'sandbox' or 'production'")]
    InvalidEnv(String),

    #[error("Live Cashfree credentials ('{client_id_prefix}') cannot be used in sandbox mode ('{mode}')")]
    LiveKeysInSandboxMode {
        mode: String,
        client_id_prefix: String,
    },

    #[error("Test Cashfree credentials ('{client_id_prefix}') cannot be used in production mode")]
    TestKeysInProductionMode { client_id_prefix: String },
}

impl CashfreeConfig {
    pub const DEFAULT_API_VERSION: &'static str = "2023-08-01";
    pub const SANDBOX_API_BASE: &'static str = "https://sandbox.cashfree.com/pg";
    pub const PRODUCTION_API_BASE: &'static str = "https://api.cashfree.com/pg";

    /// Load Cashfree configuration from environment variables.
    /// If no Cashfree variables are set, returns Ok(None).
    pub fn from_env() -> Result<Option<Self>, CashfreeConfigError> {
        let client_id_raw = env::var("CASHFREE_CLIENT_ID").ok();
        let client_secret_raw = env::var("CASHFREE_CLIENT_SECRET").ok();

        let client_id = client_id_raw.map(clean_secret).filter(|s| !s.is_empty());
        let client_secret = client_secret_raw
            .map(clean_secret)
            .filter(|s| !s.is_empty());

        match (client_id, client_secret) {
            (None, None) => Ok(None),
            (Some(_), None) | (None, Some(_)) => Err(CashfreeConfigError::IncompleteConfig),
            (Some(client_id), Some(client_secret)) => {
                let env_raw = env::var("CASHFREE_ENV")
                    .or_else(|_| env::var("CASHFREE_MODE"))
                    .unwrap_or_else(|_| "sandbox".to_string());
                let cf_env = CashfreeEnv::parse(&env_raw)?;

                let api_version = env::var("CASHFREE_API_VERSION")
                    .map(clean_secret)
                    .unwrap_or_else(|_| Self::DEFAULT_API_VERSION.to_string());

                let api_base = if cf_env.is_sandbox() {
                    env::var("CASHFREE_API_BASE")
                        .unwrap_or_else(|_| Self::SANDBOX_API_BASE.to_string())
                } else {
                    env::var("CASHFREE_API_BASE")
                        .unwrap_or_else(|_| Self::PRODUCTION_API_BASE.to_string())
                };

                let prefix: String = client_id.chars().take(8).collect();
                if cf_env.is_production() && prefix.to_uppercase().starts_with("TEST") {
                    return Err(CashfreeConfigError::TestKeysInProductionMode {
                        client_id_prefix: prefix,
                    });
                }

                Ok(Some(Self {
                    client_id,
                    client_secret,
                    api_version,
                    env: cf_env,
                    api_base,
                }))
            }
        }
    }

    pub fn is_sandbox(&self) -> bool {
        self.env.is_sandbox()
    }

    pub fn public_status(&self, is_sandbox_caller: bool) -> serde_json::Value {
        let prefix: String = self.client_id.chars().take(8).collect();
        let display_id = if is_sandbox_caller || self.env.is_sandbox() {
            self.client_id.clone()
        } else {
            format!("{}...", prefix)
        };

        serde_json::json!({
            "configured": true,
            "environment": if self.env.is_sandbox() { "sandbox" } else { "production" },
            "client_id": display_id,
            "api_version": self.api_version,
        })
    }
}
