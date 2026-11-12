use crate::config::PayUConfig;
use crate::hash::{calculate_command_hash, calculate_request_hash};
use anyhow::{Context, Result};
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{info, warn};

pub struct PayUClient {
    http: Client,
    config: PayUConfig,
}

impl PayUClient {
    pub fn new(config: PayUConfig) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build PayU HTTP client");

        Self { http, config }
    }

    /// Build a client with system proxy discovery disabled (useful in sandboxed test environments).
    pub fn new_without_proxy(config: PayUConfig) -> Self {
        let http = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build PayU HTTP client");

        Self { http, config }
    }

    pub fn config(&self) -> &PayUConfig {
        &self.config
    }

    /// Build parameters and form action URL for PayU Hosted Checkout.
    /// Returns (action_url, form_params_map).
    #[allow(clippy::too_many_arguments)]
    pub fn build_hosted_checkout(
        &self,
        txnid: &str,
        amount_minor: u64,
        productinfo: &str,
        firstname: &str,
        email: &str,
        phone: &str,
        surl: &str,
        furl: &str,
        curl: Option<&str>,
        udf1: Option<&str>,
        udf2: Option<&str>,
        udf3: Option<&str>,
        udf4: Option<&str>,
        udf5: Option<&str>,
    ) -> (String, HashMap<String, String>) {
        let amount_str = format!("{:.2}", (amount_minor as f64) / 100.0);

        let hash = calculate_request_hash(
            &self.config.merchant_key,
            txnid,
            &amount_str,
            productinfo,
            firstname,
            email,
            udf1,
            udf2,
            udf3,
            udf4,
            udf5,
            &self.config.merchant_salt,
        );

        let mut params = HashMap::new();
        params.insert("key".to_string(), self.config.merchant_key.clone());
        params.insert("txnid".to_string(), txnid.to_string());
        params.insert("amount".to_string(), amount_str);
        params.insert("productinfo".to_string(), productinfo.to_string());
        params.insert("firstname".to_string(), firstname.to_string());
        params.insert("email".to_string(), email.to_string());
        params.insert("phone".to_string(), phone.to_string());
        params.insert("surl".to_string(), surl.to_string());
        params.insert("furl".to_string(), furl.to_string());
        if let Some(c) = curl {
            params.insert("curl".to_string(), c.to_string());
        }
        params.insert("hash".to_string(), hash);
        params.insert("service_provider".to_string(), "payu_paisa".to_string());

        if let Some(u) = udf1 {
            params.insert("udf1".to_string(), u.to_string());
        }
        if let Some(u) = udf2 {
            params.insert("udf2".to_string(), u.to_string());
        }
        if let Some(u) = udf3 {
            params.insert("udf3".to_string(), u.to_string());
        }
        if let Some(u) = udf4 {
            params.insert("udf4".to_string(), u.to_string());
        }
        if let Some(u) = udf5 {
            params.insert("udf5".to_string(), u.to_string());
        }

        (self.config.payment_url.clone(), params)
    }

    /// Server-to-server authoritative payment verification via PayU PostService API.
    pub async fn verify_payment(&self, txnid: &str) -> Result<serde_json::Value> {
        let command = "verify_payment";
        let hash = calculate_command_hash(
            &self.config.merchant_key,
            command,
            txnid,
            &self.config.merchant_salt,
        );

        let form_params = [
            ("key", self.config.merchant_key.as_str()),
            ("command", command),
            ("var1", txnid),
            ("hash", hash.as_str()),
        ];

        let resp = self
            .http
            .post(&self.config.verify_url)
            .form(&form_params)
            .send()
            .await
            .context("Failed to dispatch PayU verify_payment request")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            warn!("PayU verify_payment error: HTTP {} — {}", status, body_text);
            anyhow::bail!("PayU API error (HTTP {}): {}", status, body_text);
        }

        let body: serde_json::Value = serde_json::from_str(&body_text)
            .context("Failed to parse PayU verify_payment response JSON")?;

        info!("Verified PayU payment for txnid {}", txnid);
        Ok(body)
    }

    /// Initiate an upstream refund via PayU cancel_refund_transaction command.
    pub async fn create_refund(
        &self,
        mihpayid: &str,
        refund_token: &str,
        amount_minor: u64,
    ) -> Result<serde_json::Value> {
        let command = "cancel_refund_transaction";
        let amount_str = format!("{:.2}", (amount_minor as f64) / 100.0);

        let hash = calculate_command_hash(
            &self.config.merchant_key,
            command,
            mihpayid,
            &self.config.merchant_salt,
        );

        let form_params = [
            ("key", self.config.merchant_key.as_str()),
            ("command", command),
            ("var1", mihpayid),
            ("var2", refund_token),
            ("var3", amount_str.as_str()),
            ("hash", hash.as_str()),
        ];

        let resp = self
            .http
            .post(&self.config.verify_url)
            .form(&form_params)
            .send()
            .await
            .context("Failed to dispatch PayU refund request")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!("PayU refund error (HTTP {}): {}", status, body_text);
        }

        let body: serde_json::Value = serde_json::from_str(&body_text)
            .context("Failed to parse PayU refund response JSON")?;

        info!(
            "Initiated PayU refund token {} for mihpayid {}",
            refund_token, mihpayid
        );
        Ok(body)
    }

    /// Check refund status via check_action_status command.
    pub async fn check_refund_status(&self, request_id: &str) -> Result<serde_json::Value> {
        let command = "check_action_status";
        let hash = calculate_command_hash(
            &self.config.merchant_key,
            command,
            request_id,
            &self.config.merchant_salt,
        );

        let form_params = [
            ("key", self.config.merchant_key.as_str()),
            ("command", command),
            ("var1", request_id),
            ("hash", hash.as_str()),
        ];

        let resp = self
            .http
            .post(&self.config.verify_url)
            .form(&form_params)
            .send()
            .await
            .context("Failed to dispatch PayU check_action_status request")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!(
                "PayU check_action_status error (HTTP {}): {}",
                status,
                body_text
            );
        }

        let body: serde_json::Value = serde_json::from_str(&body_text)?;
        Ok(body)
    }
}
