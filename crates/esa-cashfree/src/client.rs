use crate::config::CashfreeConfig;
use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use std::time::Duration;
use tracing::{info, warn};

pub struct CashfreeClient {
    http: Client,
    config: CashfreeConfig,
}

impl CashfreeClient {
    pub fn new(config: CashfreeConfig) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build Cashfree HTTP client");

        Self { http, config }
    }

    /// Build a client with system proxy discovery disabled (useful in sandboxed test environments).
    pub fn new_without_proxy(config: CashfreeConfig) -> Self {
        let http = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build Cashfree HTTP client");

        Self { http, config }
    }

    pub fn config(&self) -> &CashfreeConfig {
        &self.config
    }

    /// Health check to verify credentials and connectivity to Cashfree.
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/orders?limit=1", self.config.api_base);
        let resp = self
            .http
            .get(&url)
            .header("x-client-id", &self.config.client_id)
            .header("x-client-secret", &self.config.client_secret)
            .header("x-api-version", &self.config.api_version)
            .send()
            .await?;

        Ok(resp.status().is_success() || resp.status() == StatusCode::OK)
    }

    /// Create an upstream Cashfree Order for Checkout.
    /// Cashfree requires amount in major INR units (rupees, e.g. 500.00 for 50000 paise).
    #[allow(clippy::too_many_arguments)]
    pub async fn create_order(
        &self,
        order_id: &str,
        amount_minor: u64,
        currency: &str,
        customer_id: &str,
        customer_email: Option<&str>,
        customer_phone: Option<&str>,
        customer_name: Option<&str>,
        return_url: &str,
        notify_url: Option<&str>,
        order_note: Option<&str>,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/orders", self.config.api_base);
        let amount_rupees = (amount_minor as f64) / 100.0;

        let mut customer_details = serde_json::json!({
            "customer_id": customer_id,
            "customer_phone": customer_phone.unwrap_or("9999999999"),
        });
        if let Some(email) = customer_email {
            customer_details["customer_email"] = serde_json::Value::String(email.to_string());
        }
        if let Some(name) = customer_name {
            customer_details["customer_name"] = serde_json::Value::String(name.to_string());
        }

        let mut order_meta = serde_json::json!({
            "return_url": return_url,
        });
        if let Some(notify) = notify_url {
            order_meta["notify_url"] = serde_json::Value::String(notify.to_string());
        }

        let mut payload = serde_json::json!({
            "order_id": order_id,
            "order_amount": amount_rupees,
            "order_currency": currency,
            "customer_details": customer_details,
            "order_meta": order_meta,
        });

        if let Some(note) = order_note {
            payload["order_note"] = serde_json::Value::String(note.to_string());
        }

        let resp = self
            .http
            .post(&url)
            .header("x-client-id", &self.config.client_id)
            .header("x-client-secret", &self.config.client_secret)
            .header("x-api-version", &self.config.api_version)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to dispatch Cashfree create_order request")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            warn!(
                "Cashfree order creation error: HTTP {} — payload: {}",
                status, body_text
            );
            anyhow::bail!("Cashfree API error (HTTP {}): {}", status, body_text);
        }

        let body: serde_json::Value = serde_json::from_str(&body_text)
            .context("Failed to parse Cashfree create_order response JSON")?;
        info!("Successfully created Cashfree order {}", order_id);
        Ok(body)
    }

    /// Retrieve an order by order_id.
    pub async fn get_order(&self, order_id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/orders/{}", self.config.api_base, order_id);
        let resp = self
            .http
            .get(&url)
            .header("x-client-id", &self.config.client_id)
            .header("x-client-secret", &self.config.client_secret)
            .header("x-api-version", &self.config.api_version)
            .send()
            .await
            .context("Failed to dispatch Cashfree get_order request")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!("Cashfree get_order error (HTTP {}): {}", status, body_text);
        }

        let body: serde_json::Value = serde_json::from_str(&body_text)?;
        Ok(body)
    }

    /// Retrieve all payment attempts for an order.
    pub async fn get_order_payments(&self, order_id: &str) -> Result<Vec<serde_json::Value>> {
        let url = format!("{}/orders/{}/payments", self.config.api_base, order_id);
        let resp = self
            .http
            .get(&url)
            .header("x-client-id", &self.config.client_id)
            .header("x-client-secret", &self.config.client_secret)
            .header("x-api-version", &self.config.api_version)
            .send()
            .await
            .context("Failed to dispatch Cashfree get_order_payments request")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!(
                "Cashfree get_order_payments error (HTTP {}): {}",
                status,
                body_text
            );
        }

        let list: Vec<serde_json::Value> = serde_json::from_str(&body_text)?;
        Ok(list)
    }

    /// Create a refund for an order.
    pub async fn create_refund(
        &self,
        order_id: &str,
        refund_id: &str,
        amount_minor: u64,
        refund_note: Option<&str>,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/orders/{}/refunds", self.config.api_base, order_id);
        let amount_rupees = (amount_minor as f64) / 100.0;

        let mut payload = serde_json::json!({
            "refund_amount": amount_rupees,
            "refund_id": refund_id,
        });
        if let Some(note) = refund_note {
            payload["refund_note"] = serde_json::Value::String(note.to_string());
        }

        let resp = self
            .http
            .post(&url)
            .header("x-client-id", &self.config.client_id)
            .header("x-client-secret", &self.config.client_secret)
            .header("x-api-version", &self.config.api_version)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to dispatch Cashfree create_refund request")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!(
                "Cashfree create_refund error (HTTP {}): {}",
                status,
                body_text
            );
        }

        let body: serde_json::Value = serde_json::from_str(&body_text)?;
        info!("Successfully created Cashfree refund {}", refund_id);
        Ok(body)
    }

    /// Retrieve refund details by order_id and refund_id.
    pub async fn get_refund(&self, order_id: &str, refund_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/orders/{}/refunds/{}",
            self.config.api_base, order_id, refund_id
        );
        let resp = self
            .http
            .get(&url)
            .header("x-client-id", &self.config.client_id)
            .header("x-client-secret", &self.config.client_secret)
            .header("x-api-version", &self.config.api_version)
            .send()
            .await
            .context("Failed to dispatch Cashfree get_refund request")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!("Cashfree get_refund error (HTTP {}): {}", status, body_text);
        }

        let body: serde_json::Value = serde_json::from_str(&body_text)?;
        Ok(body)
    }
}
