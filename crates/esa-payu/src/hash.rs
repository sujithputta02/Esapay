use sha2::{Digest, Sha512};
use subtle::ConstantTimeEq;

/// Helper to compute lowercase hex SHA-512 digest.
fn sha512_hex(input: &str) -> String {
    let mut hasher = Sha512::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

/// Compute PayU Hosted Checkout Request Hash.
/// Formula: sha512(key|txnid|amount|productinfo|firstname|email|udf1|udf2|udf3|udf4|udf5|udf6|udf7|udf8|udf9|udf10|salt)
#[allow(clippy::too_many_arguments)]
pub fn calculate_request_hash(
    key: &str,
    txnid: &str,
    amount: &str,
    productinfo: &str,
    firstname: &str,
    email: &str,
    udf1: Option<&str>,
    udf2: Option<&str>,
    udf3: Option<&str>,
    udf4: Option<&str>,
    udf5: Option<&str>,
    salt: &str,
) -> String {
    let u1 = udf1.unwrap_or("");
    let u2 = udf2.unwrap_or("");
    let u3 = udf3.unwrap_or("");
    let u4 = udf4.unwrap_or("");
    let u5 = udf5.unwrap_or("");

    // udf6..udf10 are empty
    let hash_string = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}||||||{}",
        key.trim(),
        txnid.trim(),
        amount.trim(),
        productinfo.trim(),
        firstname.trim(),
        email.trim(),
        u1.trim(),
        u2.trim(),
        u3.trim(),
        u4.trim(),
        u5.trim(),
        salt.trim()
    );

    sha512_hex(&hash_string)
}

/// Compute PayU Reverse Hash for Callback & Webhook Verification.
/// If additionalCharges is present:
///   sha512(additionalCharges|salt|status|udf10|udf9|udf8|udf7|udf6|udf5|udf4|udf3|udf2|udf1|email|firstname|productinfo|amount|txnid|key)
/// If additionalCharges is NOT present:
///   sha512(salt|status|udf10|udf9|udf8|udf7|udf6|udf5|udf4|udf3|udf2|udf1|email|firstname|productinfo|amount|txnid|key)
#[allow(clippy::too_many_arguments)]
pub fn calculate_reverse_hash(
    salt: &str,
    status: &str,
    udf1: Option<&str>,
    udf2: Option<&str>,
    udf3: Option<&str>,
    udf4: Option<&str>,
    udf5: Option<&str>,
    email: &str,
    firstname: &str,
    productinfo: &str,
    amount: &str,
    txnid: &str,
    key: &str,
    additional_charges: Option<&str>,
) -> String {
    let u1 = udf1.unwrap_or("");
    let u2 = udf2.unwrap_or("");
    let u3 = udf3.unwrap_or("");
    let u4 = udf4.unwrap_or("");
    let u5 = udf5.unwrap_or("");

    // udf10 down to udf6 are empty
    let udfs_reversed = format!(
        "|||||{}|{}|{}|{}|{}",
        u5.trim(),
        u4.trim(),
        u3.trim(),
        u2.trim(),
        u1.trim()
    );

    let hash_string = match additional_charges.filter(|s| !s.trim().is_empty()) {
        Some(charges) => format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            charges.trim(),
            salt.trim(),
            status.trim(),
            udfs_reversed,
            email.trim(),
            firstname.trim(),
            productinfo.trim(),
            amount.trim(),
            txnid.trim(),
            key.trim()
        ),
        None => format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            salt.trim(),
            status.trim(),
            udfs_reversed,
            email.trim(),
            firstname.trim(),
            productinfo.trim(),
            amount.trim(),
            txnid.trim(),
            key.trim()
        ),
    };

    sha512_hex(&hash_string)
}

/// Constant-time verification of PayU callback response hash.
#[allow(clippy::too_many_arguments)]
pub fn verify_response_hash(
    received_hash: &str,
    salt: &str,
    status: &str,
    udf1: Option<&str>,
    udf2: Option<&str>,
    udf3: Option<&str>,
    udf4: Option<&str>,
    udf5: Option<&str>,
    email: &str,
    firstname: &str,
    productinfo: &str,
    amount: &str,
    txnid: &str,
    key: &str,
    additional_charges: Option<&str>,
) -> bool {
    let expected = calculate_reverse_hash(
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
        amount,
        txnid,
        key,
        additional_charges,
    );

    expected
        .as_bytes()
        .ct_eq(received_hash.trim().as_bytes())
        .into()
}

/// Calculate SHA-512 hash for PayU PostService command API (verify_payment, check_action_status, cancel_refund_transaction).
/// Formula: sha512(key|command|var1|salt)
pub fn calculate_command_hash(key: &str, command: &str, var1: &str, salt: &str) -> String {
    let hash_string = format!(
        "{}|{}|{}|{}",
        key.trim(),
        command.trim(),
        var1.trim(),
        salt.trim()
    );
    sha512_hex(&hash_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payu_request_hash_calculation() {
        let key = "gtKFFx";
        let salt = "eCwWELxi";
        let txnid = "Txn12345";
        let amount = "10.00";
        let productinfo = "iPhone";
        let firstname = "Sujith";
        let email = "test@example.com";

        let hash = calculate_request_hash(
            key,
            txnid,
            amount,
            productinfo,
            firstname,
            email,
            None,
            None,
            None,
            None,
            None,
            salt,
        );

        // Verify hash string format:
        // gtKFFx|Txn12345|10.00|iPhone|Sujith|test@example.com|||||||||||eCwWELxi
        let expected_input =
            "gtKFFx|Txn12345|10.00|iPhone|Sujith|test@example.com|||||||||||eCwWELxi";
        let expected_hash = sha512_hex(expected_input);
        assert_eq!(hash, expected_hash);
    }

    #[test]
    fn test_payu_reverse_hash_verification() {
        let key = "gtKFFx";
        let salt = "eCwWELxi";
        let txnid = "Txn12345";
        let amount = "10.00";
        let productinfo = "iPhone";
        let firstname = "Sujith";
        let email = "test@example.com";
        let status = "success";

        let rev_hash = calculate_reverse_hash(
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

        // Verification must succeed with identical fields
        assert!(verify_response_hash(
            &rev_hash,
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
        ));

        // Forged hash or mismatched status fails
        assert!(!verify_response_hash(
            "badhash",
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
        ));

        assert!(!verify_response_hash(
            &rev_hash,
            salt,
            "failure", // mismatched
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
        ));
    }

    #[test]
    fn test_payu_command_hash_calculation() {
        let key = "gtKFFx";
        let salt = "eCwWELxi";
        let command = "verify_payment";
        let var1 = "Txn12345";

        let hash = calculate_command_hash(key, command, var1, salt);
        let expected = sha512_hex("gtKFFx|verify_payment|Txn12345|eCwWELxi");
        assert_eq!(hash, expected);
    }
}
