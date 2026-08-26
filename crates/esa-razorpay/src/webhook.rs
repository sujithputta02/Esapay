use chrono::Utc;
use esa_core::{PaymentEvent, PaymentEventType, PaymentMethodClass, Region};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::warn;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("invalid webhook signature")]
    InvalidSignature,
    #[error("missing X-Razorpay-Signature header")]
    MissingSignature,
    #[error("invalid webhook JSON: {0}")]
    InvalidJson(String),
    #[error("unsupported or malformed event payload")]
    MalformedPayload,
    #[error("duplicate event")]
    DuplicateEvent,
    #[error("webhook rate limit exceeded")]
    RateLimited,
}

/// Verify Razorpay webhook HMAC-SHA256 signature (hex).
pub fn verify_signature(body: &[u8], signature: &str, webhook_secret: &str) -> bool {
    let normalized_sig = normalize_signature_header(signature);
    if normalized_sig.is_empty() {
        return false;
    }

    let computed = compute_signature_hex(body, webhook_secret);
    constant_time_eq(computed.as_bytes(), normalized_sig.as_bytes())
        || constant_time_eq(computed.to_uppercase().as_bytes(), normalized_sig.to_uppercase().as_bytes())
}

/// Test-mode fallback when webhook secret was misconfigured as API secret.
pub fn verify_signature_any(body: &[u8], signature: &str, webhook_secret: &str, key_secret: &str) -> bool {
    verify_signature(body, signature, webhook_secret)
        || (!key_secret.is_empty() && verify_signature(body, signature, key_secret))
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

    let event_name = root
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let event_type = map_event_type(event_name)?;

    let payment_entity = root
        .pointer("/payload/payment/entity")
        .or_else(|| root.pointer("/payload/order/entity"))
        .or_else(|| root.pointer("/payload/refund/entity"));

    if payment_entity.is_none() {
        return Err(WebhookError::MalformedPayload);
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
        .unwrap_or("created");

    let success = matches!(event_type, PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid)
        || status == "captured"
        || status == "authorized";

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
        .unwrap_or("created");

    let event_type = match status {
        "captured" => PaymentEventType::PaymentCaptured,
        "authorized" => PaymentEventType::PaymentAuthorized,
        "failed" => PaymentEventType::PaymentFailed,
        _ => PaymentEventType::PaymentCaptured,
    };

    let amount = entity.get("amount").and_then(|v| v.as_u64());
    let method = entity
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("upi");

    let success = status == "captured" || status == "authorized";
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
        _ => {
            warn!("Unknown Razorpay event type: {}", name);
            Ok(PaymentEventType::PaymentCaptured)
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
