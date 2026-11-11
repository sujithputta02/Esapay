use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::Utc;
use esa_core::events::PaymentEventType;
use esa_core::provider::NormalizedWebhookEvent;
use esa_core::types::PaymentGateway;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

pub const MAX_WEBHOOK_BODY_BYTES: usize = 1024 * 1024; // 1 MB limit

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CashfreeWebhookError {
    #[error("Missing signature header 'x-webhook-signature'")]
    MissingSignature,

    #[error("Missing timestamp header 'x-webhook-timestamp'")]
    MissingTimestamp,

    #[error("Cashfree webhook secret is not configured")]
    MissingSecret,

    #[error("Payload exceeds maximum size of {0} bytes")]
    PayloadTooLarge(usize),

    #[error("Invalid webhook signature")]
    InvalidSignature,

    #[error("Malformed webhook JSON: {0}")]
    MalformedJson(String),

    #[error("Missing required order or payment field: {0}")]
    MissingRequiredField(String),
}

/// Verify Cashfree webhook signature using exact raw bytes, timestamp, and client secret.
/// Formula: Base64(HMAC-SHA256(secret, timestamp + raw_body))
pub fn verify_signature(
    raw_body: &[u8],
    timestamp: &str,
    signature_header: &str,
    client_secret: &str,
) -> bool {
    let mut mac = match HmacSha256::new_from_slice(client_secret.trim().as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };

    mac.update(timestamp.as_bytes());
    mac.update(raw_body);
    let expected_bytes = mac.finalize().into_bytes();
    let expected_b64 = BASE64_STANDARD.encode(expected_bytes);

    // Constant-time comparison prevents timing attacks
    expected_b64
        .as_bytes()
        .ct_eq(signature_header.trim().as_bytes())
        .into()
}

/// Parse and normalize a verified Cashfree webhook payload.
pub fn parse_webhook(
    raw_body: &[u8],
    timestamp: &str,
    signature_header: &str,
    client_secret: &str,
) -> Result<NormalizedWebhookEvent, CashfreeWebhookError> {
    if raw_body.len() > MAX_WEBHOOK_BODY_BYTES {
        return Err(CashfreeWebhookError::PayloadTooLarge(raw_body.len()));
    }

    if timestamp.trim().is_empty() {
        return Err(CashfreeWebhookError::MissingTimestamp);
    }

    if signature_header.trim().is_empty() {
        return Err(CashfreeWebhookError::MissingSignature);
    }

    if client_secret.trim().is_empty() {
        return Err(CashfreeWebhookError::MissingSecret);
    }

    if !verify_signature(raw_body, timestamp, signature_header, client_secret) {
        return Err(CashfreeWebhookError::InvalidSignature);
    }

    let payload: serde_json::Value = serde_json::from_slice(raw_body)
        .map_err(|e| CashfreeWebhookError::MalformedJson(e.to_string()))?;

    let event_type_str = payload
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("UNKNOWN");

    let data = payload
        .get("data")
        .ok_or_else(|| CashfreeWebhookError::MissingRequiredField("data".into()))?;

    let order = data.get("order");
    let payment = data.get("payment");
    let refund = data.get("refund");

    let order_id = order
        .and_then(|o| o.get("order_id"))
        .and_then(|v| v.as_str())
        .map(String::from);

    // Extract payment ID
    let provider_payment_id = payment
        .and_then(|p| p.get("cf_payment_id"))
        .map(|v| {
            if let Some(s) = v.as_str() {
                s.to_string()
            } else if let Some(n) = v.as_i64() {
                n.to_string()
            } else {
                v.to_string()
            }
        })
        .or_else(|| order_id.clone())
        .unwrap_or_else(|| format!("cf_{}", Utc::now().timestamp_millis()));

    // Extract amount
    let amount_f64 = payment
        .and_then(|p| p.get("payment_amount"))
        .and_then(|v| v.as_f64())
        .or_else(|| {
            order
                .and_then(|o| o.get("order_amount"))
                .and_then(|v| v.as_f64())
        })
        .unwrap_or(0.0);
    let amount_minor = (amount_f64 * 100.0).round() as u64;

    let currency = payment
        .and_then(|p| p.get("payment_currency"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            order
                .and_then(|o| o.get("order_currency"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("INR")
        .to_string();

    let (event_type, refund_id, refund_amount_minor, refund_status) = match event_type_str {
        "PAYMENT_SUCCESS_WEBHOOK" => (PaymentEventType::PaymentCaptured, None, None, None),
        "PAYMENT_FAILED_WEBHOOK" | "PAYMENT_USER_DROPPED_WEBHOOK" => {
            (PaymentEventType::PaymentFailed, None, None, None)
        }
        "REFUND_STATUS_WEBHOOK" => {
            let ref_id = refund
                .and_then(|r| r.get("refund_id"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let ref_amount = refund
                .and_then(|r| r.get("refund_amount"))
                .and_then(|v| v.as_f64())
                .map(|a| (a * 100.0).round() as u64);
            let ref_status = refund
                .and_then(|r| r.get("refund_status"))
                .and_then(|v| v.as_str())
                .unwrap_or("PENDING");

            let ev = match ref_status.to_uppercase().as_str() {
                "SUCCESS" => PaymentEventType::RefundProcessed,
                "FAILED" => PaymentEventType::RefundFailed,
                _ => PaymentEventType::RefundCreated,
            };

            (ev, ref_id, ref_amount, Some(ref_status.to_lowercase()))
        }
        _ => {
            let status = payment
                .and_then(|p| p.get("payment_status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if status.eq_ignore_ascii_case("SUCCESS") {
                (PaymentEventType::PaymentCaptured, None, None, None)
            } else if status.eq_ignore_ascii_case("FAILED") {
                (PaymentEventType::PaymentFailed, None, None, None)
            } else {
                (PaymentEventType::PaymentAuthorized, None, None, None)
            }
        }
    };

    let provider_event_id = format!(
        "cf_evt_{}_{}",
        provider_payment_id,
        timestamp.replace([':', '-'], "")
    );

    Ok(NormalizedWebhookEvent {
        provider: PaymentGateway::Cashfree,
        provider_event_id,
        event_type,
        provider_payment_id,
        provider_order_id: order_id,
        amount_minor,
        currency,
        refund_id,
        refund_amount_minor,
        refund_status,
        raw_payload: payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cashfree_signature_verification() {
        let secret = "test_secret_key_12345";
        let timestamp = "1691234567890";
        let body = br#"{"data":{"order":{"order_id":"order_123","order_amount":100.00}},"type":"PAYMENT_SUCCESS_WEBHOOK"}"#;

        // Compute expected signature
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(timestamp.as_bytes());
        mac.update(body);
        let expected_sig = BASE64_STANDARD.encode(mac.finalize().into_bytes());

        // Valid signature matches
        assert!(verify_signature(body, timestamp, &expected_sig, secret));

        // Forged signature fails
        assert!(!verify_signature(
            body,
            timestamp,
            "forged_signature_b64==",
            secret
        ));

        // Tampered body fails
        let tampered_body = br#"{"data":{"order":{"order_id":"order_123","order_amount":999.00}},"type":"PAYMENT_SUCCESS_WEBHOOK"}"#;
        assert!(!verify_signature(
            tampered_body,
            timestamp,
            &expected_sig,
            secret
        ));

        // Tampered timestamp fails
        assert!(!verify_signature(
            body,
            "1699999999999",
            &expected_sig,
            secret
        ));
    }

    #[test]
    fn test_cashfree_webhook_normalization() {
        let secret = "test_secret_key_12345";
        let timestamp = "1691234567890";
        let body = br#"{"data":{"order":{"order_id":"cf_ord_001","order_amount":250.50,"order_currency":"INR"},"payment":{"cf_payment_id":998877,"payment_status":"SUCCESS","payment_amount":250.50,"payment_currency":"INR"}},"type":"PAYMENT_SUCCESS_WEBHOOK"}"#;

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(timestamp.as_bytes());
        mac.update(body);
        let sig = BASE64_STANDARD.encode(mac.finalize().into_bytes());

        let event = parse_webhook(body, timestamp, &sig, secret).expect("must parse");
        assert_eq!(event.provider, PaymentGateway::Cashfree);
        assert_eq!(event.event_type, PaymentEventType::PaymentCaptured);
        assert_eq!(event.provider_payment_id, "998877");
        assert_eq!(event.provider_order_id, Some("cf_ord_001".to_string()));
        assert_eq!(event.amount_minor, 25050); // ₹250.50 -> 25050 paise
        assert_eq!(event.currency, "INR");
    }
}
