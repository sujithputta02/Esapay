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

/// Razorpay execution mode: non-live (Test) or live (Live).
/// Non-live modes (test, sandbox, uat, staging, dev) MUST be paired with test credentials (`rzp_test_`).
/// Live mode (live, production) MUST be paired with live credentials (`rzp_live_`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RazorpayMode {
    Test,
    Live,
}

impl RazorpayMode {
    pub fn parse(raw: &str) -> Result<Self, RazorpayConfigError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "test" | "sandbox" | "uat" | "staging" | "dev" | "development" | "local"
            | "simulation" => Ok(RazorpayMode::Test),
            "live" | "production" | "prod" => Ok(RazorpayMode::Live),
            other => Err(RazorpayConfigError::InvalidMode(other.to_string())),
        }
    }

    pub fn is_test(&self) -> bool {
        matches!(self, RazorpayMode::Test)
    }

    pub fn is_live(&self) -> bool {
        matches!(self, RazorpayMode::Live)
    }
}

#[derive(Debug, Clone)]
pub struct RazorpayConfig {
    pub key_id: String,
    pub key_secret: String,
    pub webhook_secret: String,
    pub api_base: String,
    pub mode: RazorpayMode,
    pub test_mode_only: bool,
    pub webhook_rate_limit_per_sec: u32,
    pub api_rate_limit_per_sec: u32,
    pub account_id: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RazorpayConfigError {
    #[error("missing {0} — set it in .env (see .env.example)")]
    MissingVar(String),
    #[error(
        "invalid RAZORPAY_MODE '{0}' — expected non-live ('test', 'sandbox', 'uat', 'staging', 'dev') or live ('live', 'production')"
    )]
    InvalidMode(String),
    #[error(
        "live Razorpay keys are not allowed when RAZORPAY_MODE is non-live (mode='{mode}', key_id prefix='{actual_prefix}')"
    )]
    LiveKeysInNonLiveMode { mode: String, actual_prefix: String },
    #[error(
        "test Razorpay keys are not allowed when RAZORPAY_MODE is live (mode='{mode}', key_id prefix='{actual_prefix}')"
    )]
    TestKeysInLiveMode { mode: String, actual_prefix: String },
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

        let mode_raw = env::var("RAZORPAY_MODE").unwrap_or_else(|_| "test".to_string());
        let mode = RazorpayMode::parse(&mode_raw)?;
        let test_mode_only = mode.is_test();

        let actual_prefix: String = key_id.chars().take(9).collect();
        if mode.is_test() && !key_id.starts_with("rzp_test_") {
            return Err(RazorpayConfigError::LiveKeysInNonLiveMode {
                mode: mode_raw,
                actual_prefix,
            });
        }
        if mode.is_live() && !key_id.starts_with("rzp_live_") {
            return Err(RazorpayConfigError::TestKeysInLiveMode {
                mode: mode_raw,
                actual_prefix,
            });
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

        let account_id = env::var("RAZORPAY_ACCOUNT_ID").ok().map(clean_secret);

        Ok(Self {
            key_id,
            key_secret,
            webhook_secret,
            api_base,
            mode,
            test_mode_only,
            webhook_rate_limit_per_sec,
            api_rate_limit_per_sec,
            account_id,
        })
    }

    pub fn is_configured() -> bool {
        env::var("RAZORPAY_KEY_ID").is_ok()
            && env::var("RAZORPAY_KEY_SECRET").is_ok()
            && env::var("RAZORPAY_WEBHOOK_SECRET").is_ok()
    }

    pub fn is_test_mode(&self) -> bool {
        self.mode.is_test()
    }

    pub fn is_live_mode(&self) -> bool {
        self.mode.is_live()
    }

    pub fn is_test_credentials(&self) -> bool {
        self.key_id.starts_with("rzp_test_")
    }

    pub fn is_live_credentials(&self) -> bool {
        self.key_id.starts_with("rzp_live_")
    }

    pub fn is_sandbox_compatible(&self) -> bool {
        self.is_test_mode() && self.is_test_credentials()
    }

    pub fn public_status(&self) -> serde_json::Value {
        self.public_status_for_caller(false)
    }

    pub fn public_status_for_caller(&self, is_sandbox_caller: bool) -> serde_json::Value {
        let (key_id_val, key_prefix_val) = if self.is_live_credentials() && is_sandbox_caller {
            (
                "[REDACTED_LIVE_CREDENTIAL]".to_string(),
                "[REDACTED]".to_string(),
            )
        } else {
            (
                self.key_id.clone(),
                self.key_id.chars().take(12).collect::<String>() + "...",
            )
        };

        serde_json::json!({
            "enabled": true,
            "mode": if self.test_mode_only { "test" } else { "live" },
            "key_id": key_id_val,
            "key_id_prefix": key_prefix_val,
            "account_id": self.account_id,
            "webhook_rate_limit_per_sec": self.webhook_rate_limit_per_sec,
            "api_rate_limit_per_sec": self.api_rate_limit_per_sec,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn clear_env() {
        env::remove_var("RAZORPAY_KEY_ID");
        env::remove_var("RAZORPAY_KEY_SECRET");
        env::remove_var("RAZORPAY_WEBHOOK_SECRET");
        env::remove_var("RAZORPAY_MODE");
    }

    #[test]
    fn test_mode_and_credential_permutations() {
        let _guard = TEST_MUTEX.lock();

        // 1. Permitted non-live modes with rzp_test_ succeed
        let non_live_modes = ["test", "sandbox", "uat", "staging", "dev", "development"];
        for m in &non_live_modes {
            clear_env();
            env::set_var("RAZORPAY_KEY_ID", "rzp_test_abc12345678");
            env::set_var("RAZORPAY_KEY_SECRET", "sec123");
            env::set_var("RAZORPAY_WEBHOOK_SECRET", "whsec123");
            env::set_var("RAZORPAY_MODE", m);

            let cfg = RazorpayConfig::from_env().unwrap();
            assert!(cfg.is_test_mode());
            assert!(cfg.is_test_credentials());
            assert!(cfg.is_sandbox_compatible());
            assert!(!cfg.is_live_mode());
        }

        // 2. Non-live modes with rzp_live_ are strictly rejected
        for m in &non_live_modes {
            clear_env();
            env::set_var("RAZORPAY_KEY_ID", "rzp_live_abc12345678");
            env::set_var("RAZORPAY_KEY_SECRET", "sec123");
            env::set_var("RAZORPAY_WEBHOOK_SECRET", "whsec123");
            env::set_var("RAZORPAY_MODE", m);

            let err = RazorpayConfig::from_env().unwrap_err();
            assert!(
                matches!(err, RazorpayConfigError::LiveKeysInNonLiveMode { .. }),
                "Expected LiveKeysInNonLiveMode for mode {}, got {:?}",
                m,
                err
            );
        }

        // 3. Live modes with rzp_live_ succeed
        let live_modes = ["live", "production", "prod"];
        for m in &live_modes {
            clear_env();
            env::set_var("RAZORPAY_KEY_ID", "rzp_live_real9876543");
            env::set_var("RAZORPAY_KEY_SECRET", "sec123");
            env::set_var("RAZORPAY_WEBHOOK_SECRET", "whsec123");
            env::set_var("RAZORPAY_MODE", m);

            let cfg = RazorpayConfig::from_env().unwrap();
            assert!(cfg.is_live_mode());
            assert!(cfg.is_live_credentials());
            assert!(!cfg.is_sandbox_compatible());
        }

        // 4. Live modes with rzp_test_ are strictly rejected
        for m in &live_modes {
            clear_env();
            env::set_var("RAZORPAY_KEY_ID", "rzp_test_abc12345678");
            env::set_var("RAZORPAY_KEY_SECRET", "sec123");
            env::set_var("RAZORPAY_WEBHOOK_SECRET", "whsec123");
            env::set_var("RAZORPAY_MODE", m);

            let err = RazorpayConfig::from_env().unwrap_err();
            assert!(
                matches!(err, RazorpayConfigError::TestKeysInLiveMode { .. }),
                "Expected TestKeysInLiveMode for mode {}, got {:?}",
                m,
                err
            );
        }

        // 5. Invalid mode string is rejected
        clear_env();
        env::set_var("RAZORPAY_KEY_ID", "rzp_test_abc12345678");
        env::set_var("RAZORPAY_KEY_SECRET", "sec123");
        env::set_var("RAZORPAY_WEBHOOK_SECRET", "whsec123");
        env::set_var("RAZORPAY_MODE", "unsupported_mode_xyz");
        let err = RazorpayConfig::from_env().unwrap_err();
        assert!(matches!(err, RazorpayConfigError::InvalidMode(_)));

        // 6. Public status masks live credential for sandbox callers
        let live_cfg = RazorpayConfig {
            key_id: "rzp_live_verysecretkey".to_string(),
            key_secret: "secret".to_string(),
            webhook_secret: "whsec".to_string(),
            api_base: "https://api.razorpay.com/v1".to_string(),
            mode: RazorpayMode::Live,
            test_mode_only: false,
            webhook_rate_limit_per_sec: 30,
            api_rate_limit_per_sec: 10,
            account_id: None,
        };
        let status_sandbox = live_cfg.public_status_for_caller(true);
        assert_eq!(status_sandbox["key_id"], "[REDACTED_LIVE_CREDENTIAL]");

        let status_merchant = live_cfg.public_status_for_caller(false);
        assert_eq!(status_merchant["key_id"], "rzp_live_verysecretkey");

        clear_env();
    }
}
