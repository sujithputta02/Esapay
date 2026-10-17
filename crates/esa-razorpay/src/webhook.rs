use chrono::Utc;
use esa_core::payment_state::Currency;
use esa_core::{PaymentEvent, PaymentEventType, PaymentMethodClass, Region};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::warn;

type HmacSha256 = Hmac<Sha256>;

/// Maximum permissible raw webhook request body size (64 KiB) before parsing.
pub const MAX_WEBHOOK_BODY_BYTES: usize = 65_536;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum WebhookError {
    #[error("invalid webhook signature")]
    InvalidSignature,
    #[error("missing X-Razorpay-Signature header")]
    MissingSignature,
    #[error("webhook secret not configured")]
    MissingWebhookSecret,
    #[error("request body exceeds maximum allowed size of {0} bytes (received {1} bytes)")]
    PayloadTooLarge(usize, usize),
    #[error("invalid webhook JSON: {0}")]
    InvalidJson(String),
    #[error("unsupported or malformed event payload: {0}")]
    MalformedPayload(String),
    #[error("unsupported event type: {0}")]
    UnsupportedEvent(String),
    #[error("unsupported payment status: {0}")]
    UnsupportedStatus(String),
    #[error("contradictory refund event '{event}' and status '{status}'")]
    ContradictoryRefundStatus { event: String, status: String },
    #[error("merchant account ID mismatch: expected '{expected}', got '{actual}'")]
    MerchantMismatch { expected: String, actual: String },
    #[error("invalid currency: {0}")]
    InvalidCurrency(String),
    #[error("invalid amount: {0}")]
    InvalidAmount(String),
    #[error("duplicate event")]
    DuplicateEvent,
    #[error("webhook rate limit exceeded")]
    RateLimited,
}

/// Normalized, validated webhook event with separate provider event ID and payment ID.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedWebhook {
    pub provider: &'static str,
    pub provider_event_id: String,
    pub event_type: PaymentEventType,
    pub payment_id: String,
    pub order_id: Option<String>,
    pub account_id: Option<String>,
    pub amount_minor: u64,
    pub currency: String,
    pub refund_id: Option<String>,
    pub refund_amount_minor: Option<u64>,
    pub status: String,
    pub payment_event: PaymentEvent,
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

/// Detailed parser and schema validator for Razorpay webhook payloads.
/// Enforces size bounding, schema integrity, provider event ID preservation,
/// merchant account validation, currency, amount limits, and payment reference separation.
pub fn parse_webhook_payload_detailed(
    body: &[u8],
    event_id_header: Option<&str>,
    expected_account_id: Option<&str>,
) -> Result<ParsedWebhook, WebhookError> {
    if body.len() > MAX_WEBHOOK_BODY_BYTES {
        return Err(WebhookError::PayloadTooLarge(
            body.len(),
            MAX_WEBHOOK_BODY_BYTES,
        ));
    }

    let root: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| WebhookError::InvalidJson(e.to_string()))?;

    if !root.is_object() {
        return Err(WebhookError::MalformedPayload(
            "Root webhook payload must be a JSON object".to_string(),
        ));
    }

    // Verify merchant account identity if expected
    let account_id = root
        .get("account_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());

    if let Some(expected) = expected_account_id {
        let expected_clean = expected.trim();
        if !expected_clean.is_empty() {
            let actual = account_id.as_deref().unwrap_or("");
            if actual != expected_clean {
                warn!(
                    "Razorpay webhook merchant mismatch: expected '{}', got '{}'",
                    expected_clean, actual
                );
                return Err(WebhookError::MerchantMismatch {
                    expected: expected_clean.to_string(),
                    actual: actual.to_string(),
                });
            }
        }
    }

    let event_name = root
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| WebhookError::MalformedPayload("Missing 'event' field".to_string()))?;

    let event_type = map_event_type(event_name)?;

    // Handle refund entity vs payment/order entity
    let (
        payment_id,
        order_id,
        amount_minor,
        currency,
        status,
        method,
        refund_id,
        refund_amount_minor,
        entity_for_region,
    ) = match event_type {
        PaymentEventType::RefundCreated
        | PaymentEventType::RefundProcessed
        | PaymentEventType::RefundFailed
        | PaymentEventType::RefundSpeedChanged => {
            let refund_entity = root.pointer("/payload/refund/entity").ok_or_else(|| {
                WebhookError::MalformedPayload(
                    "Missing '/payload/refund/entity' in refund webhook".to_string(),
                )
            })?;

            let r_id = refund_entity
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    WebhookError::MalformedPayload("Missing refund entity 'id'".to_string())
                })?
                .trim()
                .to_string();

            if r_id.is_empty() {
                return Err(WebhookError::MalformedPayload(
                    "Refund entity 'id' cannot be empty".to_string(),
                ));
            }

            let r_amount = refund_entity
                .get("amount")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    WebhookError::InvalidAmount("Missing or invalid refund amount".to_string())
                })?;

            if r_amount == 0 {
                return Err(WebhookError::InvalidAmount(
                    "Refund amount cannot be zero".to_string(),
                ));
            }

            let p_id = refund_entity
                .get("payment_id")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    root.pointer("/payload/payment/entity/id")
                        .and_then(|v| v.as_str())
                })
                .ok_or_else(|| {
                    WebhookError::MalformedPayload("Missing payment_id for refund".to_string())
                })?
                .trim()
                .to_string();

            if p_id.is_empty() {
                return Err(WebhookError::MalformedPayload(
                    "Payment ID for refund cannot be empty".to_string(),
                ));
            }

            let curr = refund_entity
                .get("currency")
                .and_then(|v| v.as_str())
                .unwrap_or("INR")
                .trim()
                .to_string();

            let default_status = match event_type {
                PaymentEventType::RefundProcessed => "processed",
                PaymentEventType::RefundFailed => "failed",
                _ => "pending",
            };

            let explicit_status = refund_entity
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.trim());

            if let Some(explicit) = explicit_status {
                match explicit {
                    "processed" | "pending" | "failed" => {}
                    other => {
                        warn!("Unsupported refund status: {}", other);
                        return Err(WebhookError::UnsupportedStatus(other.to_string()));
                    }
                }

                match event_type {
                    PaymentEventType::RefundProcessed if explicit == "failed" => {
                        warn!("Contradictory refund event 'refund.processed' with status 'failed'");
                        return Err(WebhookError::ContradictoryRefundStatus {
                            event: "refund.processed".to_string(),
                            status: explicit.to_string(),
                        });
                    }
                    PaymentEventType::RefundFailed if explicit == "processed" => {
                        warn!("Contradictory refund event 'refund.failed' with status 'processed'");
                        return Err(WebhookError::ContradictoryRefundStatus {
                            event: "refund.failed".to_string(),
                            status: explicit.to_string(),
                        });
                    }
                    PaymentEventType::RefundCreated if explicit == "failed" => {
                        warn!("Contradictory refund event 'refund.created' with status 'failed'");
                        return Err(WebhookError::ContradictoryRefundStatus {
                            event: "refund.created".to_string(),
                            status: explicit.to_string(),
                        });
                    }
                    _ => {}
                }
            }

            let st = explicit_status.unwrap_or(default_status).to_string();

            let m = root
                .pointer("/payload/payment/entity/method")
                .and_then(|v| v.as_str())
                .unwrap_or("upi")
                .to_string();

            let ord_id = root
                .pointer("/payload/payment/entity/order_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let p_amount = root
                .pointer("/payload/payment/entity/amount")
                .and_then(|v| v.as_u64())
                .unwrap_or(r_amount);

            (
                p_id,
                ord_id,
                p_amount,
                curr,
                st,
                m,
                Some(r_id),
                Some(r_amount),
                refund_entity,
            )
        }
        _ => {
            let payment_entity = root
                .pointer("/payload/payment/entity")
                .or_else(|| root.pointer("/payload/order/entity"))
                .ok_or_else(|| {
                    WebhookError::MalformedPayload(
                        "Missing payment or order entity in webhook payload".to_string(),
                    )
                })?;

            let p_id = payment_entity
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    WebhookError::MalformedPayload("Missing payment ID in payload".to_string())
                })?
                .trim()
                .to_string();

            if p_id.is_empty() {
                return Err(WebhookError::MalformedPayload(
                    "Payment ID cannot be empty".to_string(),
                ));
            }

            let ord_id = payment_entity
                .get("order_id")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string());

            let amount = payment_entity
                .get("amount")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    WebhookError::InvalidAmount("Missing or non-numeric amount".to_string())
                })?;

            if amount == 0 {
                return Err(WebhookError::InvalidAmount(
                    "Payment amount must be greater than zero".to_string(),
                ));
            }

            let curr = payment_entity
                .get("currency")
                .and_then(|v| v.as_str())
                .unwrap_or("INR")
                .trim()
                .to_string();

            let st = payment_entity
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .trim()
                .to_string();

            // Validate status
            match st.as_str() {
                "captured" | "authorized" | "failed" => {}
                other => {
                    warn!("Unsupported payment status: {}", other);
                    return Err(WebhookError::UnsupportedStatus(other.to_string()));
                }
            }

            let m = payment_entity
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("upi")
                .to_string();

            (
                p_id,
                ord_id,
                amount,
                curr,
                st,
                m,
                None,
                None,
                payment_entity,
            )
        }
    };

    // Validate currency
    let validated_currency =
        Currency::new(&currency).map_err(|e| WebhookError::InvalidCurrency(e.to_string()))?;

    // Validate minimum amount against provider policy
    let min_amount = validated_currency
        .minimum_amount_for_provider("razorpay", Some(&method))
        .map_err(|e| WebhookError::InvalidAmount(e.to_string()))?;

    if amount_minor < min_amount {
        return Err(WebhookError::InvalidAmount(format!(
            "Amount {} is below required minimum of {} for currency {}",
            amount_minor, min_amount, currency
        )));
    }

    // Determine provider_event_id:
    // 1. Explicit header X-Razorpay-Event-Id if present
    // 2. In-body 'id' or 'event_id' if present
    // 3. Fallback SHA-256 digest over the raw payload body
    // Policy for discrepancy: If both header and in-body identifier are present, they MUST match.
    // Discrepancy is rejected as MalformedPayload to prevent replay ambiguity or spoofing.
    let in_body_id = root
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| root.get("event_id").and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let provider_event_id = if let Some(hdr) = event_id_header {
        let clean_hdr = hdr.trim();
        if !clean_hdr.is_empty() {
            if let Some(ref body_id) = in_body_id {
                if body_id != clean_hdr {
                    warn!(
                        "Header X-Razorpay-Event-Id '{}' conflicts with body event ID '{}'",
                        clean_hdr, body_id
                    );
                    return Err(WebhookError::MalformedPayload(format!(
                        "Header X-Razorpay-Event-Id '{}' conflicts with body event ID '{}'",
                        clean_hdr, body_id
                    )));
                }
            }
            clean_hdr.to_string()
        } else if let Some(body_id) = in_body_id {
            body_id
        } else {
            derive_proven_fallback_event_id(event_name, &payment_id, refund_id.as_deref(), body)
        }
    } else if let Some(body_id) = in_body_id {
        body_id
    } else {
        derive_proven_fallback_event_id(event_name, &payment_id, refund_id.as_deref(), body)
    };

    let region = map_region(entity_for_region);
    let method_class = map_payment_method(&method);

    let success = matches!(
        event_type,
        PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid
    ) || status == "captured";

    let pseudo_ref = format!(
        "RZP-{}",
        payment_id.chars().rev().take(8).collect::<String>()
    );

    let payment_event = PaymentEvent {
        event_id: provider_event_id.clone(),
        event_type: event_type.clone(),
        timestamp: Utc::now(),
        region,
        payment_method_class: method_class,
        pseudonymous_reference: pseudo_ref,
        amount_cents: Some(amount_minor),
        amount_minor: Some(amount_minor),
        currency: currency.clone(),
        processing_latency_ms: None,
        success,
    };

    Ok(ParsedWebhook {
        provider: "razorpay",
        provider_event_id,
        event_type,
        payment_id,
        order_id,
        account_id,
        amount_minor,
        currency,
        refund_id,
        refund_amount_minor,
        status,
        payment_event,
    })
}

/// Derive a deterministic fallback event identifier based on a full 256-bit SHA-256 digest
/// of the raw webhook body when the provider does not supply an explicit header or body event ID.
fn derive_proven_fallback_event_id(
    event_name: &str,
    payment_id: &str,
    refund_id: Option<&str>,
    body: &[u8],
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(body);
    let digest = hasher.finalize();
    let body_hash = hex::encode(digest);

    if let Some(r_id) = refund_id {
        format!("rzp_evt_refund_{}_{}", r_id, body_hash)
    } else {
        format!("rzp_evt_{}_{}_{}", event_name, payment_id, body_hash)
    }
}

/// Parse Razorpay webhook JSON into a normalized `PaymentEvent` (no raw card data).
pub fn parse_webhook_payload(body: &[u8]) -> Result<PaymentEvent, WebhookError> {
    parse_webhook_payload_detailed(body, None, None).map(|p| p.payment_event)
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

    let currency = entity
        .get("currency")
        .and_then(|v| v.as_str())
        .unwrap_or("INR")
        .to_string();

    Ok(PaymentEvent {
        event_id: format!("rzp_{}", payment_id),
        event_type,
        timestamp: Utc::now(),
        region,
        payment_method_class: method_class,
        pseudonymous_reference: pseudo_ref,
        amount_cents: amount,
        amount_minor: amount,
        currency,
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
        "refund.processed" => Ok(PaymentEventType::RefundProcessed),
        "refund.failed" => Ok(PaymentEventType::RefundFailed),
        "refund.speed_changed" | "refund.speed_processed" => {
            Ok(PaymentEventType::RefundSpeedChanged)
        }
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
        assert!(event
            .event_id
            .starts_with("rzp_evt_payment.captured_pay_test_100_"));
        assert_eq!(event.amount_cents, Some(25000));
        assert!(event.success);
        assert_eq!(event.region, Region::IndiaWest);
    }

    #[test]
    fn test_provider_event_id_from_header() {
        let payload = serde_json::json!({
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_test_hdr",
                        "amount": 25000,
                        "currency": "INR",
                        "status": "captured"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let parsed = parse_webhook_payload_detailed(&body, Some("evt_from_header_456"), None)
            .expect("Should parse");
        assert_eq!(parsed.provider_event_id, "evt_from_header_456");
    }

    #[test]
    fn test_provider_event_id_header_and_body_match() {
        let payload = serde_json::json!({
            "id": "evt_matching_123",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_test_hdr",
                        "amount": 25000,
                        "currency": "INR",
                        "status": "captured"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let parsed = parse_webhook_payload_detailed(&body, Some("evt_matching_123"), None)
            .expect("Should parse");
        assert_eq!(parsed.provider_event_id, "evt_matching_123");
    }

    #[test]
    fn test_provider_event_id_header_and_body_conflict_fails() {
        let payload = serde_json::json!({
            "id": "evt_in_body_123",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_test_hdr",
                        "amount": 25000,
                        "currency": "INR",
                        "status": "captured"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let res = parse_webhook_payload_detailed(&body, Some("evt_from_header_456"), None);
        assert!(matches!(res, Err(WebhookError::MalformedPayload(_))));
    }

    #[test]
    fn test_provider_event_id_from_json_root_precedence() {
        // 1. Root 'id'
        let payload1 = serde_json::json!({
            "id": "evt_from_root_id_789",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_test_root",
                        "amount": 25000,
                        "currency": "INR",
                        "status": "captured"
                    }
                }
            }
        });
        let body1 = serde_json::to_vec(&payload1).unwrap();
        let parsed1 = parse_webhook_payload_detailed(&body1, None, None).expect("Should parse");
        assert_eq!(parsed1.provider_event_id, "evt_from_root_id_789");

        // 2. Root 'event_id'
        let payload2 = serde_json::json!({
            "event_id": "evt_from_root_event_id_999",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_test_root_eid",
                        "amount": 25000,
                        "currency": "INR",
                        "status": "captured"
                    }
                }
            }
        });
        let body2 = serde_json::to_vec(&payload2).unwrap();
        let parsed2 = parse_webhook_payload_detailed(&body2, None, None).expect("Should parse");
        assert_eq!(parsed2.provider_event_id, "evt_from_root_event_id_999");
    }

    #[test]
    fn test_proven_fallback_uniqueness_for_different_payloads() {
        // Two distinct webhook payloads for the SAME payment_id without header or root id
        let payload_a = serde_json::json!({
            "event": "payment.failed",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_same_123",
                        "amount": 50000,
                        "currency": "INR",
                        "status": "failed",
                        "error_code": "BAD_REQUEST_ERROR"
                    }
                }
            }
        });
        let payload_b = serde_json::json!({
            "event": "payment.failed",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_same_123",
                        "amount": 50000,
                        "currency": "INR",
                        "status": "failed",
                        "error_code": "GATEWAY_ERROR"
                    }
                }
            }
        });

        let body_a = serde_json::to_vec(&payload_a).unwrap();
        let body_b = serde_json::to_vec(&payload_b).unwrap();

        let parsed_a = parse_webhook_payload_detailed(&body_a, None, None).expect("Should parse a");
        let parsed_b = parse_webhook_payload_detailed(&body_b, None, None).expect("Should parse b");

        // Proves fallback uniqueness: they MUST NOT collide!
        assert_ne!(parsed_a.provider_event_id, parsed_b.provider_event_id);

        // Proves fallback determinism: re-parsing payload_a yields the exact same ID
        let parsed_a2 =
            parse_webhook_payload_detailed(&body_a, None, None).expect("Should parse a again");
        assert_eq!(parsed_a.provider_event_id, parsed_a2.provider_event_id);
    }

    #[test]
    fn test_webhook_payload_too_large_fails() {
        let large_body = vec![b'a'; MAX_WEBHOOK_BODY_BYTES + 10];
        let res = parse_webhook_payload_detailed(&large_body, None, None);
        assert!(matches!(res, Err(WebhookError::PayloadTooLarge(_, _))));
    }

    #[test]
    fn test_merchant_id_mismatch_fails_closed() {
        let payload = serde_json::json!({
            "account_id": "acc_attacker_999",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_test_100",
                        "amount": 25000,
                        "status": "captured"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let res = parse_webhook_payload_detailed(&body, None, Some("acc_legit_111"));
        assert_eq!(
            res,
            Err(WebhookError::MerchantMismatch {
                expected: "acc_legit_111".to_string(),
                actual: "acc_attacker_999".to_string(),
            })
        );
    }

    #[test]
    fn test_invalid_currency_fails_closed() {
        let payload = serde_json::json!({
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_curr_001",
                        "amount": 25000,
                        "currency": "FAKE_CURRENCY",
                        "status": "captured"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let res = parse_webhook_payload_detailed(&body, None, None);
        assert!(matches!(res, Err(WebhookError::InvalidCurrency(_))));
    }

    #[test]
    fn test_amount_below_minimum_fails_closed() {
        let payload = serde_json::json!({
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_min_001",
                        "amount": 50, // 50 paise is below Razorpay 100 paise min
                        "currency": "INR",
                        "status": "captured",
                        "method": "upi"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let res = parse_webhook_payload_detailed(&body, None, None);
        assert!(matches!(res, Err(WebhookError::InvalidAmount(_))));
    }

    #[test]
    fn test_refund_created_webhook_parsing() {
        let payload = serde_json::json!({
            "id": "evt_refund_9988",
            "event": "refund.created",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_abc_123",
                        "amount": 5000,
                        "currency": "INR",
                        "payment_id": "pay_orig_456",
                        "status": "processed"
                    }
                },
                "payment": {
                    "entity": {
                        "id": "pay_orig_456",
                        "amount": 20000,
                        "status": "captured"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let parsed = parse_webhook_payload_detailed(&body, None, None).unwrap();
        assert_eq!(parsed.provider_event_id, "evt_refund_9988");
        assert_eq!(parsed.payment_id, "pay_orig_456");
        assert_eq!(parsed.refund_id, Some("rfnd_abc_123".to_string()));
        assert_eq!(parsed.refund_amount_minor, Some(5000));
        assert_eq!(parsed.event_type, PaymentEventType::RefundCreated);
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

    #[test]
    fn test_refund_processed_and_failed_webhook_parsing() {
        let payload_proc = serde_json::json!({
            "id": "evt_rfnd_proc_1",
            "event": "refund.processed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_proc_100",
                        "amount": 3000,
                        "currency": "INR",
                        "payment_id": "pay_test_orig",
                        "status": "processed"
                    }
                }
            }
        });
        let body_proc = serde_json::to_vec(&payload_proc).unwrap();
        let parsed_proc = parse_webhook_payload_detailed(&body_proc, None, None).unwrap();
        assert_eq!(parsed_proc.event_type, PaymentEventType::RefundProcessed);
        assert_eq!(parsed_proc.refund_id.as_deref(), Some("rfnd_proc_100"));
        assert_eq!(parsed_proc.refund_amount_minor, Some(3000));
        assert_eq!(parsed_proc.status, "processed");

        let payload_failed = serde_json::json!({
            "id": "evt_rfnd_fail_1",
            "event": "refund.failed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_fail_200",
                        "amount": 3000,
                        "currency": "INR",
                        "payment_id": "pay_test_orig",
                        "status": "failed"
                    }
                }
            }
        });
        let body_failed = serde_json::to_vec(&payload_failed).unwrap();
        let parsed_failed = parse_webhook_payload_detailed(&body_failed, None, None).unwrap();
        assert_eq!(parsed_failed.event_type, PaymentEventType::RefundFailed);
        assert_eq!(parsed_failed.refund_id.as_deref(), Some("rfnd_fail_200"));
        assert_eq!(parsed_failed.status, "failed");

        let payload_speed = serde_json::json!({
            "id": "evt_rfnd_spd_1",
            "event": "refund.speed_changed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_spd_300",
                        "amount": 3000,
                        "currency": "INR",
                        "payment_id": "pay_test_orig",
                        "status": "pending"
                    }
                }
            }
        });
        let body_speed = serde_json::to_vec(&payload_speed).unwrap();
        let parsed_speed = parse_webhook_payload_detailed(&body_speed, None, None).unwrap();
        assert_eq!(
            parsed_speed.event_type,
            PaymentEventType::RefundSpeedChanged
        );
        assert_eq!(parsed_speed.refund_id.as_deref(), Some("rfnd_spd_300"));
    }

    #[test]
    fn test_refund_empty_id_rejected() {
        let payload = serde_json::json!({
            "event": "refund.processed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "   ",
                        "amount": 1000,
                        "currency": "INR",
                        "payment_id": "pay_123"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let res = parse_webhook_payload_detailed(&body, None, None);
        assert!(matches!(res, Err(WebhookError::MalformedPayload(_))));
    }

    #[test]
    fn test_refund_contradictory_and_valid_status_combinations() {
        // 1. refund.processed with explicitly failed status -> rejected
        let payload_proc_failed = serde_json::json!({
            "event": "refund.processed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_contra_1",
                        "amount": 1000,
                        "currency": "INR",
                        "payment_id": "pay_contra_1",
                        "status": "failed"
                    }
                }
            }
        });
        let res1 = parse_webhook_payload_detailed(
            &serde_json::to_vec(&payload_proc_failed).unwrap(),
            None,
            None,
        );
        assert!(matches!(
            res1,
            Err(WebhookError::ContradictoryRefundStatus {
                ref event,
                ref status
            }) if event == "refund.processed" && status == "failed"
        ));

        // 2. refund.failed with explicitly processed status -> rejected
        let payload_fail_proc = serde_json::json!({
            "event": "refund.failed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_contra_2",
                        "amount": 1000,
                        "currency": "INR",
                        "payment_id": "pay_contra_2",
                        "status": "processed"
                    }
                }
            }
        });
        let res2 = parse_webhook_payload_detailed(
            &serde_json::to_vec(&payload_fail_proc).unwrap(),
            None,
            None,
        );
        assert!(matches!(
            res2,
            Err(WebhookError::ContradictoryRefundStatus {
                ref event,
                ref status
            }) if event == "refund.failed" && status == "processed"
        ));

        // 3. refund.created with explicitly failed status -> rejected
        let payload_created_failed = serde_json::json!({
            "event": "refund.created",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_contra_3",
                        "amount": 1000,
                        "currency": "INR",
                        "payment_id": "pay_contra_3",
                        "status": "failed"
                    }
                }
            }
        });
        let res3 = parse_webhook_payload_detailed(
            &serde_json::to_vec(&payload_created_failed).unwrap(),
            None,
            None,
        );
        assert!(matches!(
            res3,
            Err(WebhookError::ContradictoryRefundStatus {
                ref event,
                ref status
            }) if event == "refund.created" && status == "failed"
        ));

        // 4. refund.created with explicitly processed status -> supported (instant refund)
        let payload_created_proc = serde_json::json!({
            "event": "refund.created",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_valid_inst",
                        "amount": 1000,
                        "currency": "INR",
                        "payment_id": "pay_valid_inst",
                        "status": "processed"
                    }
                }
            }
        });
        let res4 = parse_webhook_payload_detailed(
            &serde_json::to_vec(&payload_created_proc).unwrap(),
            None,
            None,
        );
        assert!(res4.is_ok());
        let parsed4 = res4.unwrap();
        assert_eq!(parsed4.event_type, PaymentEventType::RefundCreated);
        assert_eq!(parsed4.status, "processed");

        // 5. Unknown status string -> unsupported status error
        let payload_unknown_st = serde_json::json!({
            "event": "refund.processed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_unknown_st",
                        "amount": 1000,
                        "currency": "INR",
                        "payment_id": "pay_unknown_st",
                        "status": "bogus_status"
                    }
                }
            }
        });
        let res5 = parse_webhook_payload_detailed(
            &serde_json::to_vec(&payload_unknown_st).unwrap(),
            None,
            None,
        );
        assert!(matches!(res5, Err(WebhookError::UnsupportedStatus(_))));

        // 6. Legitimate omitted status defaults
        let payload_proc_no_st = serde_json::json!({
            "event": "refund.processed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_def_proc",
                        "amount": 1000,
                        "currency": "INR",
                        "payment_id": "pay_def_proc"
                    }
                }
            }
        });
        let parsed6 = parse_webhook_payload_detailed(
            &serde_json::to_vec(&payload_proc_no_st).unwrap(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(parsed6.status, "processed");

        let payload_fail_no_st = serde_json::json!({
            "event": "refund.failed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_def_fail",
                        "amount": 1000,
                        "currency": "INR",
                        "payment_id": "pay_def_fail"
                    }
                }
            }
        });
        let parsed7 = parse_webhook_payload_detailed(
            &serde_json::to_vec(&payload_fail_no_st).unwrap(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(parsed7.status, "failed");
    }
}
