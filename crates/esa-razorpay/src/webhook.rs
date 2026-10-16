use chrono::Utc;
use esa_core::{PaymentEvent, PaymentEventType, PaymentMethodClass, Region};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::warn;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum WebhookError {
    #[error("invalid webhook signature")]
    InvalidSignature,
    #[error("missing X-Razorpay-Signature header")]
    MissingSignature,
    #[error("webhook secret not configured")]
    MissingWebhookSecret,
    #[error("invalid webhook JSON: {0}")]
    InvalidJson(String),
    #[error("unsupported or malformed event payload: {0}")]
    MalformedPayload(String),
    #[error("unsupported event type: {0}")]
    UnsupportedEvent(String),
    #[error("unsupported payment status: {0}")]
    UnsupportedStatus(String),
    #[error("duplicate event")]
    DuplicateEvent,
    #[error("webhook rate limit exceeded")]
    RateLimited,
}

/// Verify Razorpay webhook HMAC-SHA256 signature (hex).
/// Strictly requires a non-empty webhook secret and validates constant-time.
pub fn verify_signature(body: &[u8], signature: &str, webhook_secret: &str) -> bool {
    let clean_secret = webhook_secret.trim();
    if clean_secret.is_empty() {
        warn!("Razorpay webhook signature rejected: webhook secret is empty");
        return false;
    }
    let normalized_sig = normalize_signature_header(signature);
    if normalized_sig.is_empty() {
        return false;
    }

    let computed = compute_signature_hex(body, clean_secret);
    if computed.is_empty() {
        return false;
    }
    constant_time_eq(computed.as_bytes(), normalized_sig.as_bytes())
        || constant_time_eq(
            computed.to_uppercase().as_bytes(),
            normalized_sig.to_uppercase().as_bytes(),
        )
}

fn compute_signature_hex(body: &[u8], secret: &str) -> String {
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return String::new(),
    };
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

fn normalize_signature_header(signature: &str) -> String {
    let s = signature.trim();
    let s = s.strip_prefix("sha256=").unwrap_or(s);
    s.trim().to_string()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= *x ^ *y;
    }
    diff == 0
}

/// Parse Razorpay webhook JSON into a normalized `PaymentEvent` (no raw card data).
pub fn parse_webhook_payload(body: &[u8]) -> Result<PaymentEvent, WebhookError> {
    let root: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| WebhookError::InvalidJson(e.to_string()))?;

    let event_name = root.get("event").and_then(|v| v.as_str()).unwrap_or("");

    let event_type = map_event_type(event_name)?;

    let payment_entity = root
        .pointer("/payload/payment/entity")
        .or_else(|| root.pointer("/payload/order/entity"))
        .or_else(|| root.pointer("/payload/refund/entity"));

    if payment_entity.is_none() {
        return Err(WebhookError::MalformedPayload(
            "Missing payment/order/refund entity in payload".to_string(),
        ));
    }

    let entity = payment_entity.unwrap();

    let payment_id = entity
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            root.get("account_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        });

    let amount = entity.get("amount").and_then(|v| v.as_u64());
    let method = entity
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("upi");

    let status = entity
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let success = matches!(
        event_type,
        PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid
    ) || status == "captured";

    let region = map_region(entity);
    let method_class = map_payment_method(method);

    // Pseudonymous reference only — never store PAN, card number, or customer PII
    let pseudo_ref = format!(
        "RZP-{}",
        payment_id.chars().rev().take(8).collect::<String>()
    );

    Ok(PaymentEvent {
        event_id: format!("rzp_{}", payment_id),
        event_type,
        timestamp: Utc::now(),
        region,
        payment_method_class: method_class,
        pseudonymous_reference: pseudo_ref,
        amount_cents: amount,
        processing_latency_ms: None,
        success,
    })
}

/// Build a normalized event from a Razorpay payment API entity (checkout confirm fallback).
pub fn payment_entity_to_event(entity: &serde_json::Value) -> Result<PaymentEvent, WebhookError> {
    let payment_id = entity
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let status = entity
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let event_type = match status {
        "captured" => PaymentEventType::PaymentCaptured,
        "authorized" => PaymentEventType::PaymentAuthorized,
        "failed" => PaymentEventType::PaymentFailed,
        other => {
            warn!("Rejected payment entity with unsupported status: {}", other);
            return Err(WebhookError::UnsupportedStatus(other.to_string()));
        }
    };

    let amount = entity.get("amount").and_then(|v| v.as_u64());
    let method = entity
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("upi");

    let success = matches!(
        event_type,
        PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid
    );
    let region = map_region(entity);
    let method_class = map_payment_method(method);

    let pseudo_ref = format!(
        "RZP-{}",
        payment_id.chars().rev().take(8).collect::<String>()
    );

    Ok(PaymentEvent {
        event_id: format!("rzp_{}", payment_id),
        event_type,
        timestamp: Utc::now(),
        region,
        payment_method_class: method_class,
        pseudonymous_reference: pseudo_ref,
        amount_cents: amount,
        processing_latency_ms: None,
        success,
    })
}

fn map_event_type(name: &str) -> Result<PaymentEventType, WebhookError> {
    match name {
        "payment.authorized" => Ok(PaymentEventType::PaymentAuthorized),
        "payment.captured" => Ok(PaymentEventType::PaymentCaptured),
        "payment.failed" => Ok(PaymentEventType::PaymentFailed),
        "order.paid" => Ok(PaymentEventType::OrderPaid),
        "refund.created" => Ok(PaymentEventType::RefundCreated),
        other => {
            warn!("Rejected unsupported Razorpay event type: {}", other);
            Err(WebhookError::UnsupportedEvent(other.to_string()))
        }
    }
}

fn map_payment_method(method: &str) -> PaymentMethodClass {
    match method.to_lowercase().as_str() {
        "card" | "credit" | "debit" => PaymentMethodClass::Card,
        "netbanking" | "nb" => PaymentMethodClass::NetBanking,
        "wallet" => PaymentMethodClass::Wallet,
        _ => PaymentMethodClass::Upi,
    }
}

fn map_region(entity: &serde_json::Value) -> Region {
    let notes_region = entity
        .pointer("/notes/region")
        .or_else(|| entity.pointer("/notes/esa_region"))
        .and_then(|v| v.as_str());

    match notes_region {
        Some("IN-WEST") | Some("india-west") => Region::IndiaWest,
        Some("IN-NORTH") | Some("india-north") => Region::IndiaNorth,
        Some("IN-SOUTH") | Some("india-south") => Region::IndiaSouth,
        _ => Region::IndiaSouth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_signature_valid() {
        let secret = "whsec_test_secret_12345";
        let body = b"{\"event\":\"payment.captured\",\"payload\":{}}";
        let sig = compute_signature_hex(body, secret);

        assert!(verify_signature(body, &sig, secret));
        assert!(verify_signature(body, &format!("sha256={}", sig), secret));
    }

    #[test]
    fn test_verify_signature_tampered_fails() {
        let secret = "whsec_test_secret_12345";
        let body = b"{\"event\":\"payment.captured\",\"payload\":{}}";
        let sig = compute_signature_hex(body, secret);

        let tampered_body = b"{\"event\":\"payment.captured\",\"payload\":{\"evil\":true}}";
        assert!(!verify_signature(tampered_body, &sig, secret));
    }

    #[test]
    fn test_verify_signature_empty_secret_fails_closed() {
        let body = b"test payload";
        assert!(!verify_signature(body, "any_sig", ""));
        assert!(!verify_signature(body, "any_sig", "   "));
    }

    #[test]
    fn test_unknown_event_type_fails_closed() {
        let payload = serde_json::json!({
            "event": "unknown.custom.event",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_test_001",
                        "amount": 50000,
                        "status": "captured"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let res = parse_webhook_payload(&body);
        assert!(matches!(res, Err(WebhookError::UnsupportedEvent(_))));
    }

    #[test]
    fn test_valid_captured_webhook_payload() {
        let payload = serde_json::json!({
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_test_100",
                        "amount": 25000,
                        "status": "captured",
                        "method": "upi",
                        "notes": { "region": "IN-WEST" }
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let event = parse_webhook_payload(&body).expect("Should parse captured event");
        assert_eq!(event.event_type, PaymentEventType::PaymentCaptured);
        assert_eq!(event.event_id, "rzp_pay_test_100");
        assert_eq!(event.amount_cents, Some(25000));
        assert!(event.success);
        assert_eq!(event.region, Region::IndiaWest);
    }

    #[test]
    fn test_payment_entity_to_event_unknown_status_fails_closed() {
        let entity = serde_json::json!({
            "id": "pay_test_200",
            "status": "unexpected_future_status",
            "amount": 10000
        });
        let res = payment_entity_to_event(&entity);
        assert!(matches!(res, Err(WebhookError::UnsupportedStatus(_))));
    }

    #[test]
    fn test_payment_entity_to_event_valid_statuses() {
        let entity_captured = serde_json::json!({
            "id": "pay_cap_1",
            "status": "captured",
            "amount": 1000
        });
        let res_captured = payment_entity_to_event(&entity_captured).unwrap();
        assert_eq!(res_captured.event_type, PaymentEventType::PaymentCaptured);
        assert!(res_captured.success);

        let entity_failed = serde_json::json!({
            "id": "pay_fail_1",
            "status": "failed",
            "amount": 1000
        });
        let res_failed = payment_entity_to_event(&entity_failed).unwrap();
        assert_eq!(res_failed.event_type, PaymentEventType::PaymentFailed);
        assert!(!res_failed.success);
    }
}
