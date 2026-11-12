use std::collections::BTreeSet;

/// Result of redacting sensitive information from an AI prompt, response, or error string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionResult {
    pub redacted: String,
    pub redaction_count: usize,
    pub redacted_categories: Vec<String>,
}

/// Scrubs credentials, payment card numbers, CVVs, secrets, and customer PII from text.
pub fn redact_sensitive_text(input: &str) -> RedactionResult {
    let mut categories = BTreeSet::new();
    let mut count = 0usize;

    let mut current = redact_key_value_secrets(input, &mut categories, &mut count);
    current = redact_prefixed_tokens(&current, &mut categories, &mut count);
    current = redact_bearer_and_jwt(&current, &mut categories, &mut count);
    current = redact_luhn_pans(&current, &mut categories, &mut count);
    current = redact_emails(&current, &mut categories, &mut count);
    current = redact_international_phones(&current, &mut categories, &mut count);

    RedactionResult {
        redacted: current,
        redaction_count: count,
        redacted_categories: categories.into_iter().collect(),
    }
}

/// Convenience helper returning only the redacted prompt string.
pub fn sanitize_prompt(input: &str) -> String {
    redact_sensitive_text(input).redacted
}

/// Strips hidden chain-of-thought blocks (`<think>...</think>`) from model outputs.
pub fn strip_chain_of_thought(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = find_ascii_case_insensitive(rest, "<think>") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + "<think>".len()..];
        if let Some(end) = find_ascii_case_insensitive(after_open, "</think>") {
            rest = &after_open[end + "</think>".len()..];
        } else {
            // Unclosed <think> tag: drop trailing reasoning block safely.
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Truncates text cleanly on a UTF-8 character boundary.
pub fn truncate_utf8(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let char_count = input.chars().count();
    if char_count <= max_chars {
        return input.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut end_byte = 0usize;
    for (idx, (byte_pos, _)) in input.char_indices().enumerate() {
        if idx == keep {
            end_byte = byte_pos;
            break;
        }
    }
    if end_byte == 0 && keep > 0 {
        end_byte = input.len();
    }
    format!("{}...", &input[..end_byte])
}

/// Maps raw inference error text into a bounded, non-sensitive error category.
pub fn categorize_error(err: &str) -> String {
    let lower = err.to_ascii_lowercase();
    if lower.contains("timeout") || lower.contains("timed out") {
        "timeout".to_string()
    } else if lower.contains("429") || lower.contains("rate limit") || lower.contains("too many") {
        "rate_limited".to_string()
    } else if lower.contains("circuit breaker") {
        "circuit_open".to_string()
    } else if lower.contains("http ") || lower.contains("status") {
        "upstream_http_error".to_string()
    } else if lower.contains("parse") || lower.contains("json") || lower.contains("deserialize") {
        "parse_error".to_string()
    } else if lower.contains("connect") || lower.contains("dns") || lower.contains("refused") {
        "connection_error".to_string()
    } else {
        "inference_error".to_string()
    }
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    for i in 0..=h.len() - n.len() {
        if h[i..i + n.len()].eq_ignore_ascii_case(n) {
            return Some(i);
        }
    }
    None
}

fn redact_key_value_secrets(
    input: &str,
    categories: &mut BTreeSet<String>,
    count: &mut usize,
) -> String {
    const SECRET_KEYS: &[(&str, &str, &str)] = &[
        ("webhook_secret", "webhook_secret", "[REDACTED_SECRET]"),
        ("signing_secret", "webhook_secret", "[REDACTED_SECRET]"),
        ("client_secret", "api_secret", "[REDACTED_SECRET]"),
        ("key_secret", "api_secret", "[REDACTED_SECRET]"),
        ("merchant_salt", "api_secret", "[REDACTED_SECRET]"),
        ("private_key", "credential", "[REDACTED_SECRET]"),
        ("access_token", "auth_token", "[REDACTED_AUTH_TOKEN]"),
        ("refresh_token", "auth_token", "[REDACTED_AUTH_TOKEN]"),
        ("auth_token", "auth_token", "[REDACTED_AUTH_TOKEN]"),
        ("x-api-key", "api_key", "[REDACTED_SECRET]"),
        ("api_key", "api_key", "[REDACTED_SECRET]"),
        ("apikey", "api_key", "[REDACTED_SECRET]"),
        ("password", "password", "[REDACTED_SECRET]"),
        ("passwd", "password", "[REDACTED_SECRET]"),
        ("card_number", "payment_card", "[REDACTED_PAN]"),
        ("cvv", "cvv", "[REDACTED_CVV]"),
        ("cvc", "cvv", "[REDACTED_CVV]"),
        ("pin", "credential", "[REDACTED_SECRET]"),
        ("secret", "api_secret", "[REDACTED_SECRET]"),
    ];

    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let mut matched = false;
        for &(key, cat, placeholder) in SECRET_KEYS {
            if i + key.len() <= bytes.len()
                && bytes[i..i + key.len()].eq_ignore_ascii_case(key.as_bytes())
                && is_key_boundary(bytes, i, key.len())
            {
                let mut pos = i + key.len();
                // Optional closing quote around JSON key
                if pos < bytes.len() && (bytes[pos] == b'"' || bytes[pos] == b'\'') {
                    pos += 1;
                }
                // Skip whitespace
                while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                if pos < bytes.len() && (bytes[pos] == b':' || bytes[pos] == b'=') {
                    pos += 1;
                    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                        pos += 1;
                    }
                    if pos < bytes.len() {
                        if bytes[pos] == b'"' || bytes[pos] == b'\'' {
                            let quote = bytes[pos];
                            let val_start = pos + 1;
                            let mut val_end = val_start;
                            while val_end < bytes.len() && bytes[val_end] != quote {
                                if bytes[val_end] == b'\\' && val_end + 1 < bytes.len() {
                                    val_end += 2;
                                } else {
                                    val_end += 1;
                                }
                            }
                            if val_end < bytes.len() && val_end > val_start {
                                out.push_str(&input[i..val_start]);
                                out.push_str(placeholder);
                                out.push(quote as char);
                                i = val_end + 1;
                                categories.insert(cat.to_string());
                                *count += 1;
                                matched = true;
                                break;
                            }
                        } else {
                            let val_start = pos;
                            let mut val_end = val_start;
                            while val_end < bytes.len()
                                && !bytes[val_end].is_ascii_whitespace()
                                && !matches!(bytes[val_end], b',' | b'}' | b']' | b';')
                            {
                                val_end += 1;
                            }
                            if val_end > val_start {
                                out.push_str(&input[i..val_start]);
                                out.push_str(placeholder);
                                i = val_end;
                                categories.insert(cat.to_string());
                                *count += 1;
                                matched = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        if !matched {
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }

    out
}

fn is_key_boundary(bytes: &[u8], start: usize, len: usize) -> bool {
    let before_ok =
        start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
    let after = start + len;
    let after_ok =
        after >= bytes.len() || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'_');
    before_ok && after_ok
}

fn redact_prefixed_tokens(
    input: &str,
    categories: &mut BTreeSet<String>,
    count: &mut usize,
) -> String {
    const PREFIXES: &[(&str, &str, &str)] = &[
        ("esa_live_", "api_key", "[REDACTED_SECRET]"),
        ("esa_test_", "api_key", "[REDACTED_SECRET]"),
        ("rzp_live_", "payment_credential", "[REDACTED_SECRET]"),
        ("rzp_test_", "payment_credential", "[REDACTED_SECRET]"),
        ("sk_live_", "payment_credential", "[REDACTED_SECRET]"),
        ("sk_test_", "payment_credential", "[REDACTED_SECRET]"),
        ("hf_", "auth_token", "[REDACTED_AUTH_TOKEN]"),
    ];

    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let mut matched = false;
        let before_ok = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if before_ok {
            for &(prefix, cat, placeholder) in PREFIXES {
                if input[i..].starts_with(prefix) {
                    let mut end = i + prefix.len();
                    while end < bytes.len()
                        && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'-'))
                    {
                        end += 1;
                    }
                    if end > i + prefix.len() + 3 {
                        out.push_str(placeholder);
                        i = end;
                        categories.insert(cat.to_string());
                        *count += 1;
                        matched = true;
                        break;
                    }
                }
            }
        }
        if !matched {
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn redact_bearer_and_jwt(
    input: &str,
    categories: &mut BTreeSet<String>,
    count: &mut usize,
) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if i + 7 <= bytes.len() && bytes[i..i + 7].eq_ignore_ascii_case(b"Bearer ") {
            let mut end = i + 7;
            while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                end += 1;
            }
            let tok_start = end;
            while end < bytes.len()
                && !bytes[end].is_ascii_whitespace()
                && !matches!(bytes[end], b'"' | b'\'' | b',' | b'}' | b']')
            {
                end += 1;
            }
            if end > tok_start {
                out.push_str("Bearer [REDACTED_AUTH_TOKEN]");
                i = end;
                categories.insert("auth_token".to_string());
                *count += 1;
                continue;
            }
        }
        if input[i..].starts_with("eyJ") && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            let mut end = i;
            let mut dots = 0usize;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'-' | b'_' | b'.'))
            {
                if bytes[end] == b'.' {
                    dots += 1;
                }
                end += 1;
            }
            if dots == 2 && end - i >= 20 {
                out.push_str("[REDACTED_AUTH_TOKEN]");
                i = end;
                categories.insert("auth_token".to_string());
                *count += 1;
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn redact_luhn_pans(input: &str, categories: &mut BTreeSet<String>, count: &mut usize) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i].is_ascii_digit() && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            let mut end = i;
            let mut digits = Vec::new();
            while end < bytes.len() {
                if bytes[end].is_ascii_digit() {
                    digits.push((bytes[end] - b'0') as u32);
                    end += 1;
                } else if (bytes[end] == b' ' || bytes[end] == b'-')
                    && end + 1 < bytes.len()
                    && bytes[end + 1].is_ascii_digit()
                {
                    end += 1;
                } else {
                    break;
                }
            }
            let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
            if after_ok && (13..=19).contains(&digits.len()) && luhn_valid(&digits) {
                out.push_str("[REDACTED_PAN]");
                i = end;
                categories.insert("payment_card".to_string());
                *count += 1;
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn luhn_valid(digits: &[u32]) -> bool {
    let mut sum = 0u32;
    let mut double = false;
    for &d in digits.iter().rev() {
        let mut val = d;
        if double {
            val *= 2;
            if val > 9 {
                val -= 9;
            }
        }
        sum += val;
        double = !double;
    }
    sum.is_multiple_of(10)
}

fn redact_emails(input: &str, categories: &mut BTreeSet<String>, count: &mut usize) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if is_email_local_char(bytes[i]) && (i == 0 || !is_email_local_char(bytes[i - 1])) {
            let mut end = i;
            while end < bytes.len() && is_email_local_char(bytes[end]) {
                end += 1;
            }
            if end > i && end < bytes.len() && bytes[end] == b'@' {
                let dom_start = end + 1;
                let mut dom_end = dom_start;
                let mut has_dot = false;
                while dom_end < bytes.len()
                    && (bytes[dom_end].is_ascii_alphanumeric()
                        || matches!(bytes[dom_end], b'.' | b'-'))
                {
                    if bytes[dom_end] == b'.' {
                        has_dot = true;
                    }
                    dom_end += 1;
                }
                if has_dot
                    && dom_end > dom_start + 2
                    && bytes[dom_end - 1] != b'.'
                    && bytes[dom_start] != b'.'
                {
                    out.push_str("[REDACTED_EMAIL]");
                    i = dom_end;
                    categories.insert("pii_email".to_string());
                    *count += 1;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_email_local_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn redact_international_phones(
    input: &str,
    categories: &mut BTreeSet<String>,
    count: &mut usize,
) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'+'
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_digit()
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
        {
            let mut end = i + 1;
            let mut digit_count = 0usize;
            while end < bytes.len()
                && (bytes[end].is_ascii_digit() || matches!(bytes[end], b'-' | b' '))
            {
                if bytes[end].is_ascii_digit() {
                    digit_count += 1;
                }
                end += 1;
            }
            if (8..=15).contains(&digit_count) {
                out.push_str("[REDACTED_PHONE]");
                i = end;
                categories.insert("pii_phone".to_string());
                *count += 1;
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redacts_secrets_tokens_pans_and_pii() {
        let raw = r#"{"api_key":"esa_live_secret_123456","webhook_secret":"whsec_abc123","cvv":"456","card_number":"4111-1111-1111-1111","email":"alice@example.com","phone":"+91 9876543210","auth":"Bearer hf_supersecrettoken12345"}"#;
        let res = redact_sensitive_text(raw);
        assert!(!res.redacted.contains("esa_live_secret_123456"));
        assert!(!res.redacted.contains("whsec_abc123"));
        assert!(!res.redacted.contains("456"));
        assert!(!res.redacted.contains("4111-1111-1111-1111"));
        assert!(!res.redacted.contains("alice@example.com"));
        assert!(!res.redacted.contains("9876543210"));
        assert!(!res.redacted.contains("hf_supersecrettoken12345"));
        assert!(res.redaction_count >= 6);
    }

    #[test]
    fn test_preserves_safe_telemetry_metrics() {
        let safe = r#"{"workload_id":"payment-upi-india-south","p95_latency_ms":245.5,"rate_per_min":10282,"error_rate":0.06}"#;
        let res = redact_sensitive_text(safe);
        assert_eq!(res.redacted, safe);
        assert_eq!(res.redaction_count, 0);
    }

    #[test]
    fn test_strips_chain_of_thought_tags() {
        let raw = "<think>internal hidden reasoning</think>{\"root_cause\":\"CAPACITY_ISSUE\"}";
        assert_eq!(
            strip_chain_of_thought(raw),
            "{\"root_cause\":\"CAPACITY_ISSUE\"}"
        );
    }
}
