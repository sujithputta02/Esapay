use crate::hash::verify_response_hash;
use chrono::Utc;
use esa_core::events::PaymentEventType;
use esa_core::provider::NormalizedWebhookEvent;
use esa_core::types::PaymentGateway;
use std::collections::HashMap;
use thiserror::Error;

pub const MAX_PAYU_PAYLOAD_BYTES: usize = 1024 * 1024; // 1 MB limit

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PayUWebhookError {
    #[error("Missing PayU hash field in callback/webhook payload")]
    MissingHash,

    #[error("Invalid PayU reverse hash signature")]
    InvalidHash,

    #[error("Payload exceeds maximum size of {0} bytes")]
    PayloadTooLarge(usize),

    #[error("Missing required transaction field: {0}")]
    MissingRequiredField(String),

    #[error("Malformed payload: {0}")]
    MalformedPayload(String),
}

/// Verify PayU callback / webhook response hash and normalize into a common event.
pub fn parse_and_verify_callback(
    params: &HashMap<String, String>,
    salt: &str,
) -> Result<NormalizedWebhookEvent, PayUWebhookError> {
    let received_hash = params.get("hash").ok_or(PayUWebhookError::MissingHash)?;

    let status = params
        .get("status")
        .ok_or_else(|| PayUWebhookError::MissingRequiredField("status".into()))?;

    let txnid = params
        .get("txnid")
        .ok_or_else(|| PayUWebhookError::MissingRequiredField("txnid".into()))?;

    let amount_str = params
        .get("amount")
        .ok_or_else(|| PayUWebhookError::MissingRequiredField("amount".into()))?;

    let productinfo = params.get("productinfo").map(String::as_str).unwrap_or("");
    let firstname = params.get("firstname").map(String::as_str).unwrap_or("");
    let email = params.get("email").map(String::as_str).unwrap_or("");
    let key = params.get("key").map(String::as_str).unwrap_or("");

    let udf1 = params.get("udf1").map(String::as_str);
    let udf2 = params.get("udf2").map(String::as_str);
    let udf3 = params.get("udf3").map(String::as_str);
    let udf4 = params.get("udf4").map(String::as_str);
    let udf5 = params.get("udf5").map(String::as_str);

    let additional_charges = params.get("additionalCharges").map(String::as_str);

    let valid = verify_response_hash(
        received_hash,
        salt,
        status,
        udf1,
        udf2,
        udf3,
        udf4,
        udf5,
        email,
        firstname,
        productinfo,
        amount_str,
        txnid,
        key,
        additional_charges,
    );

    if !valid {
        return Err(PayUWebhookError::InvalidHash);
    }

    let amount_f64: f64 = amount_str.parse().unwrap_or(0.0);
    let amount_minor = (amount_f64 * 100.0).round() as u64;

    let mihpayid = params
        .get("mihpayid")
        .cloned()
        .unwrap_or_else(|| txnid.to_string());

    let event_type = match status.to_lowercase().as_str() {
        "success" => PaymentEventType::PaymentCaptured,
        "failure" | "bounced" | "failed" => PaymentEventType::PaymentFailed,
        "pending" => PaymentEventType::PaymentAuthorized,
        _ => PaymentEventType::PaymentFailed,
    };

    let provider_event_id = format!("payu_evt_{}_{}", mihpayid, Utc::now().timestamp_millis());

    let payload_val = serde_json::to_value(params)
        .map_err(|e| PayUWebhookError::MalformedPayload(e.to_string()))?;

    Ok(NormalizedWebhookEvent {
        provider: PaymentGateway::PayU,
        provider_event_id,
        event_type,
        provider_payment_id: mihpayid,
        provider_order_id: Some(txnid.to_string()),
        amount_minor,
        currency: "INR".to_string(),
        refund_id: None,
        refund_amount_minor: None,
        refund_status: None,
        raw_payload: payload_val,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::calculate_reverse_hash;

    #[test]
    fn test_payu_callback_parsing_and_verification() {
        let salt = "test_salt_secret";
        let key = "merchant_key_1";
        let txnid = "tx_123456";
        let amount = "500.00";
        let status = "success";
        let productinfo = "Order #1";
        let firstname = "Rahul";
        let email = "rahul@example.com";

        let hash = calculate_reverse_hash(
            salt,
            status,
            None,
            None,
            None,
            None,
            None,
            email,
            firstname,
            productinfo,
            amount,
            txnid,
            key,
            None,
        );

        let mut params = HashMap::new();
        params.insert("key".to_string(), key.to_string());
        params.insert("txnid".to_string(), txnid.to_string());
        params.insert("amount".to_string(), amount.to_string());
        params.insert("productinfo".to_string(), productinfo.to_string());
        params.insert("firstname".to_string(), firstname.to_string());
        params.insert("email".to_string(), email.to_string());
        params.insert("status".to_string(), status.to_string());
        params.insert("mihpayid".to_string(), "987654321".to_string());
        params.insert("hash".to_string(), hash);

        let event = parse_and_verify_callback(&params, salt).expect("must parse and verify");
        assert_eq!(event.provider, PaymentGateway::PayU);
        assert_eq!(event.event_type, PaymentEventType::PaymentCaptured);
        assert_eq!(event.provider_payment_id, "987654321");
        assert_eq!(event.provider_order_id, Some("tx_123456".to_string()));
        assert_eq!(event.amount_minor, 50000);
    }
}
