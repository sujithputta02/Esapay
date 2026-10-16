use axum::{
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, thiserror::Error, PartialEq)]
#[allow(dead_code)]
pub enum AuthError {
    #[error("missing authorization header or token")]
    MissingToken,
    #[error("invalid token format")]
    InvalidTokenFormat,
    #[error("unsupported JWT algorithm: {0}")]
    UnsupportedAlgorithm(String),
    #[error("invalid cryptographic signature")]
    InvalidSignature,
    #[error("token has expired")]
    TokenExpired,
    #[error("token is not yet valid (nbf in future)")]
    TokenNotYetValid,
    #[error("invalid token audience")]
    InvalidAudience,
    #[error("invalid token issuer: {0}")]
    InvalidIssuer(String),
    #[error("missing required token subject (sub)")]
    MissingSubject,
    #[error("invalid or unverified API key")]
    InvalidApiKey,
    #[error("API key has expired")]
    ApiKeyExpired,
    #[error("API key is inactive or revoked")]
    ApiKeyRevoked,
    #[error("insufficient permissions for scope: {0}")]
    InsufficientScope(String),
}

/// Constant-time slice comparison to prevent timing attacks
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= *x ^ *y;
    }
    diff == 0
}

/// Authenticated identity derived from a cryptographically verified JWT or API key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerIdentity {
    pub user_id: String,
    pub merchant_id: String,
    pub email: Option<String>,
    pub role: String,
    pub auth_type: String,   // "supabase_jwt" or "api_key"
    pub environment: String, // "live", "test", "sandbox"
    pub scopes: Vec<String>,
}

impl CallerIdentity {
    #[allow(dead_code)]
    pub fn has_scope(&self, required_scope: &str) -> bool {
        self.role == "service_role"
            || self.role == "admin"
            || self.scopes.iter().any(|s| s == required_scope || s == "*")
    }
}

/// JWT Header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupabaseJwtHeader {
    pub alg: String,
    #[serde(default)]
    pub typ: Option<String>,
    #[serde(default)]
    pub kid: Option<String>,
}

/// Supabase Decoded JWT Claims
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupabaseJwtClaims {
    pub sub: Option<String>,
    pub email: Option<String>,
    pub role: Option<String>,
    pub aud: Option<serde_json::Value>,
    pub exp: Option<i64>,
    pub nbf: Option<i64>,
    pub iss: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateSandboxResponse {
    pub api_key: String,
    pub key_prefix: String,
    pub environment: String,
    pub expires_in: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ValidateKeyRequest {
    pub api_key: String,
}

#[derive(Debug, Serialize)]
pub struct ValidateKeyResponse {
    pub valid: bool,
    pub environment: String,
    pub merchant_id: String,
    pub message: String,
}

/// Compute standard SHA-256 hex string of a raw API key
pub fn hash_api_key(raw_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract Bearer token or X-API-Key from HTTP headers
pub fn extract_token(headers: &HeaderMap) -> Option<String> {
    if let Some(auth_header) = headers
        .get("Authorization")
        .and_then(|val| val.to_str().ok())
    {
        let parts: Vec<&str> = auth_header.split_whitespace().collect();
        if parts.len() == 2 && parts[0].eq_ignore_ascii_case("Bearer") {
            return Some(parts[1].trim().to_string());
        }
    }
    if let Some(api_key_header) = headers.get("X-API-Key").and_then(|val| val.to_str().ok()) {
        let clean = api_key_header.trim();
        if !clean.is_empty() {
            return Some(clean.to_string());
        }
    }
    None
}

/// Backwards-compatible alias for existing code
#[allow(dead_code)]
pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    extract_token(headers)
}

/// Decode base64 URL-safe with or without padding
fn decode_base64_url(input: &str) -> Result<Vec<u8>, AuthError> {
    URL_SAFE_NO_PAD
        .decode(input)
        .or_else(|_| {
            let padded = match input.len() % 4 {
                2 => format!("{}==", input),
                3 => format!("{}=", input),
                _ => input.to_string(),
            };
            base64::engine::general_purpose::STANDARD.decode(padded)
        })
        .map_err(|_| AuthError::InvalidTokenFormat)
}

/// Verify cryptographic signature and claims of a Supabase HS256 JWT
pub fn verify_supabase_jwt(
    token: &str,
    secret: &str,
    expected_issuer: Option<&str>,
) -> Result<SupabaseJwtClaims, AuthError> {
    let clean_secret = secret.trim();
    if clean_secret.is_empty() {
        warn!("Supabase JWT verification failed: secret is empty");
        return Err(AuthError::InvalidSignature);
    }

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AuthError::InvalidTokenFormat);
    }

    // 1. Decode and check header algorithm
    let header_bytes = decode_base64_url(parts[0])?;
    let header: SupabaseJwtHeader =
        serde_json::from_slice(&header_bytes).map_err(|_| AuthError::InvalidTokenFormat)?;

    if header.alg != "HS256" {
        warn!("Rejected JWT with unsupported algorithm: {}", header.alg);
        return Err(AuthError::UnsupportedAlgorithm(header.alg));
    }

    // 2. Cryptographically verify signature
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let mut mac = HmacSha256::new_from_slice(clean_secret.as_bytes())
        .map_err(|_| AuthError::InvalidSignature)?;
    mac.update(signing_input.as_bytes());
    let expected_sig = mac.finalize().into_bytes();

    let actual_sig = decode_base64_url(parts[2])?;
    if !constant_time_eq(&expected_sig, &actual_sig) {
        warn!("Supabase JWT signature mismatch");
        return Err(AuthError::InvalidSignature);
    }

    // 3. Decode payload
    let payload_bytes = decode_base64_url(parts[1])?;
    let claims: SupabaseJwtClaims =
        serde_json::from_slice(&payload_bytes).map_err(|_| AuthError::InvalidTokenFormat)?;

    // 4. Validate claims
    let now = chrono::Utc::now().timestamp();

    // Expiration check (required)
    match claims.exp {
        Some(exp) if exp <= now => return Err(AuthError::TokenExpired),
        None => return Err(AuthError::TokenExpired),
        _ => {}
    }

    // Not-before check (if present)
    if let Some(nbf) = claims.nbf {
        if nbf > now {
            return Err(AuthError::TokenNotYetValid);
        }
    }

    // Audience check (must contain "authenticated")
    if let Some(aud_val) = &claims.aud {
        let has_aud = match aud_val {
            serde_json::Value::String(s) => s == "authenticated",
            serde_json::Value::Array(arr) => arr
                .iter()
                .any(|v| v.as_str().map(|s| s == "authenticated").unwrap_or(false)),
            _ => false,
        };
        if !has_aud {
            return Err(AuthError::InvalidAudience);
        }
    } else {
        return Err(AuthError::InvalidAudience);
    }

    // Issuer check (if configured)
    if let Some(expected_iss) = expected_issuer {
        if let Some(iss) = &claims.iss {
            let clean_expected = expected_iss.trim().trim_end_matches('/');
            let clean_iss = iss.trim().trim_end_matches('/');
            if !clean_iss.starts_with(clean_expected) {
                return Err(AuthError::InvalidIssuer(iss.clone()));
            }
        }
    }

    // Subject check (must be non-empty)
    if claims
        .sub
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        return Err(AuthError::MissingSubject);
    }

    Ok(claims)
}

/// Fallback for read-only backwards compatibility
pub fn parse_supabase_jwt(token: &str) -> Option<SupabaseJwtClaims> {
    let secret = std::env::var("SUPABASE_JWT_SECRET")
        .or_else(|_| std::env::var("SUPABASE_SECRET_KEY"))
        .unwrap_or_default();
    let expected_iss = std::env::var("SUPABASE_URL").ok();

    if secret.is_empty() {
        return None;
    }

    verify_supabase_jwt(token, &secret, expected_iss.as_deref()).ok()
}

/// Verify an API key against configured environment hashes (standalone mode)
pub fn verify_configured_api_key(raw_key: &str) -> Option<CallerIdentity> {
    let clean = raw_key.trim();
    if clean.is_empty() {
        return None;
    }

    let input_hash = hash_api_key(clean);

    // Live production key check
    if let Ok(env_live) = std::env::var("ESA_LIVE_API_KEY") {
        let clean_live = env_live.trim();
        if !clean_live.is_empty()
            && constant_time_eq(input_hash.as_bytes(), hash_api_key(clean_live).as_bytes())
        {
            return Some(CallerIdentity {
                user_id: "merchant_live_primary".to_string(),
                merchant_id: "merchant_live_primary".to_string(),
                email: None,
                role: "merchant".to_string(),
                auth_type: "api_key".to_string(),
                environment: "live".to_string(),
                scopes: vec![
                    "payments:write".to_string(),
                    "payments:read".to_string(),
                    "telemetry:read".to_string(),
                    "gateways:toggle".to_string(),
                    "workloads:write".to_string(),
                ],
            });
        }
    }

    // Test / sandbox key check
    if let Ok(env_test) = std::env::var("ESA_TEST_API_KEY") {
        let clean_test = env_test.trim();
        if !clean_test.is_empty()
            && constant_time_eq(input_hash.as_bytes(), hash_api_key(clean_test).as_bytes())
        {
            return Some(CallerIdentity {
                user_id: "merchant_sandbox_01".to_string(),
                merchant_id: "merchant_sandbox_01".to_string(),
                email: None,
                role: "merchant".to_string(),
                auth_type: "api_key".to_string(),
                environment: "test".to_string(),
                scopes: vec![
                    "payments:write".to_string(),
                    "payments:read".to_string(),
                    "telemetry:read".to_string(),
                    "gateways:toggle".to_string(),
                ],
            });
        }
    }

    // Universal ESA_API_KEY check
    if let Ok(env_key) = std::env::var("ESA_API_KEY") {
        let clean_key = env_key.trim();
        if !clean_key.is_empty()
            && constant_time_eq(input_hash.as_bytes(), hash_api_key(clean_key).as_bytes())
        {
            let env = if clean_key.starts_with("esa_live_") {
                "live"
            } else {
                "test"
            };
            return Some(CallerIdentity {
                user_id: "merchant_default".to_string(),
                merchant_id: "merchant_default".to_string(),
                email: None,
                role: "merchant".to_string(),
                auth_type: "api_key".to_string(),
                environment: env.to_string(),
                scopes: vec![
                    "payments:write".to_string(),
                    "payments:read".to_string(),
                    "telemetry:read".to_string(),
                ],
            });
        }
    }

    None
}

/// Unified authentication: verifies either a cryptographically signed Supabase JWT
/// or a verified API key. Fails closed.
pub fn authenticate_caller(headers: &HeaderMap) -> Result<CallerIdentity, AuthError> {
    let token = extract_token(headers).ok_or(AuthError::MissingToken)?;
    let clean = token.trim();

    if clean.is_empty() {
        return Err(AuthError::MissingToken);
    }

    // Check if token is a JWT (starts with "eyJ" and has 3 parts)
    if clean.starts_with("eyJ") && clean.split('.').count() == 3 {
        let secret = std::env::var("SUPABASE_JWT_SECRET")
            .or_else(|_| std::env::var("SUPABASE_SECRET_KEY"))
            .unwrap_or_default();
        let expected_iss = std::env::var("SUPABASE_URL").ok();

        if secret.trim().is_empty() {
            warn!("Cannot verify Supabase JWT: SUPABASE_JWT_SECRET not configured");
            return Err(AuthError::InvalidSignature);
        }

        let claims = verify_supabase_jwt(clean, &secret, expected_iss.as_deref())?;
        let user_id = claims.sub.unwrap_or_default();
        let role = claims.role.unwrap_or_else(|| "authenticated".to_string());
        let short_id = if user_id.len() >= 12 {
            &user_id[..12]
        } else {
            &user_id
        };

        let is_admin = role == "service_role" || role == "admin";
        let mut scopes = vec![
            "payments:write".to_string(),
            "payments:read".to_string(),
            "telemetry:read".to_string(),
            "gateways:toggle".to_string(),
        ];
        if is_admin {
            scopes.push("workloads:write".to_string());
            scopes.push("admin".to_string());
        }

        return Ok(CallerIdentity {
            user_id: user_id.clone(),
            merchant_id: format!("usr_{}", short_id),
            email: claims.email,
            role,
            auth_type: "supabase_jwt".to_string(),
            environment: "live".to_string(),
            scopes,
        });
    }

    // Otherwise authenticate as an API key
    if let Some(identity) = verify_configured_api_key(clean) {
        return Ok(identity);
    }

    Err(AuthError::InvalidApiKey)
}

/// Check if an API key or Supabase JWT is valid (fail-closed, no prefix-only bypasses)
pub fn is_key_authorized(key: &str) -> bool {
    let clean = key.trim();
    if clean.is_empty() {
        return false;
    }

    if clean.starts_with("eyJ") && clean.split('.').count() == 3 {
        return parse_supabase_jwt(clean).is_some();
    }

    verify_configured_api_key(clean).is_some()
}

/// POST /api/v1/keys/generate-sandbox
/// Issues an instant Sandbox Key with test prefix
pub async fn generate_sandbox_key_handler() -> impl IntoResponse {
    let random_suffix = Uuid::new_v4().simple().to_string();
    let api_key = format!("esa_test_demo_{}", random_suffix);
    let key_prefix = format!("esa_test_demo_{}...", &random_suffix[..6]);

    Json(GenerateSandboxResponse {
        api_key,
        key_prefix,
        environment: "sandbox".to_string(),
        expires_in: "24h (Evaluation mode)".to_string(),
        message:
            "Sandbox Key active for local evaluation. In live mode, register keys in the ledger."
                .to_string(),
    })
}

/// POST /api/v1/keys/validate
pub async fn validate_key_handler(Json(payload): Json<ValidateKeyRequest>) -> impl IntoResponse {
    let key = payload.api_key.trim();

    // Check if token is Supabase JWT
    if key.starts_with("eyJ") && key.split('.').count() == 3 {
        let secret = std::env::var("SUPABASE_JWT_SECRET")
            .or_else(|_| std::env::var("SUPABASE_SECRET_KEY"))
            .unwrap_or_default();
        let expected_iss = std::env::var("SUPABASE_URL").ok();

        if !secret.trim().is_empty() {
            if let Ok(claims) = verify_supabase_jwt(key, &secret, expected_iss.as_deref()) {
                let user_id = claims.sub.unwrap_or_else(|| "user_unknown".to_string());
                let email = claims
                    .email
                    .unwrap_or_else(|| "authenticated_user".to_string());
                let short_id = if user_id.len() >= 12 {
                    &user_id[..12]
                } else {
                    &user_id
                };

                return (
                    StatusCode::OK,
                    Json(ValidateKeyResponse {
                        valid: true,
                        environment: "production".to_string(),
                        merchant_id: format!("usr_{}", short_id),
                        message: format!("Supabase JWT validated for {}", email),
                    }),
                );
            }
        }
    }

    // Check API key
    if let Some(identity) = verify_configured_api_key(key) {
        return (
            StatusCode::OK,
            Json(ValidateKeyResponse {
                valid: true,
                environment: identity.environment,
                merchant_id: identity.merchant_id,
                message: "API Key verified and active.".to_string(),
            }),
        );
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(ValidateKeyResponse {
            valid: false,
            environment: "unknown".to_string(),
            merchant_id: "anonymous".to_string(),
            message: "Invalid, inactive, or unverified ESAPay API key.".to_string(),
        }),
    )
}

/// GET /api/v1/auth/me
pub async fn auth_me_handler(headers: HeaderMap) -> impl IntoResponse {
    match authenticate_caller(&headers) {
        Ok(identity) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "authenticated": true,
                "auth_type": identity.auth_type,
                "user_id": identity.user_id,
                "merchant_id": identity.merchant_id,
                "email": identity.email,
                "role": identity.role,
                "environment": identity.environment,
                "scopes": identity.scopes,
            })),
        ),
        Err(err) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "authenticated": false,
                "error": err.to_string(),
            })),
        ),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn create_test_jwt(secret: &str, claims_json: &serde_json::Value) -> String {
        let header = serde_json::json!({
            "alg": "HS256",
            "typ": "JWT"
        });
        let header_str = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let payload_str = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims_json).unwrap());

        let signing_input = format!("{}.{}", header_str, payload_str);
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(signing_input.as_bytes());
        let sig_str = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

        format!("{}.{}", signing_input, sig_str)
    }

    #[test]
    fn test_valid_supabase_jwt_verification() {
        let secret = "test_super_secret_jwt_key_12345678";
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "b2c1d3e4-5f6a-7b8c-9d0e-1f2a3b4c5d6e",
            "email": "merchant@example.com",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token = create_test_jwt(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, Some("https://test.supabase.co"));
        assert!(
            res.is_ok(),
            "Valid JWT must pass verification: {:?}",
            res.err()
        );
        let verified = res.unwrap();
        assert_eq!(
            verified.sub,
            Some("b2c1d3e4-5f6a-7b8c-9d0e-1f2a3b4c5d6e".to_string())
        );
    }

    #[test]
    fn test_forged_jwt_signature_rejected() {
        let secret = "test_super_secret_jwt_key_12345678";
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "attacker-id",
            "role": "admin",
            "aud": "authenticated",
            "exp": now + 3600
        });

        // Token signed with an attacker's key/secret
        let forged_token = create_test_jwt("attacker_secret_key_8765432100", &claims);
        let res = verify_supabase_jwt(&forged_token, secret, None);
        assert_eq!(res, Err(AuthError::InvalidSignature));

        // Token with modified payload
        let valid_token = create_test_jwt(secret, &claims);
        let parts: Vec<&str> = valid_token.split('.').collect();
        let tampered_payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "sub": "admin-elevated-id",
                "role": "service_role",
                "aud": "authenticated",
                "exp": now + 3600
            }))
            .unwrap(),
        );
        let tampered_token = format!("{}.{}.{}", parts[0], tampered_payload, parts[2]);
        let res_tampered = verify_supabase_jwt(&tampered_token, secret, None);
        assert_eq!(res_tampered, Err(AuthError::InvalidSignature));
    }

    #[test]
    fn test_expired_jwt_rejected() {
        let secret = "test_super_secret_jwt_key_12345678";
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_id_123",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now - 100 // Expired
        });

        let token = create_test_jwt(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, None);
        assert_eq!(res, Err(AuthError::TokenExpired));
    }

    #[test]
    fn test_wrong_audience_rejected() {
        let secret = "test_super_secret_jwt_key_12345678";
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_id_123",
            "role": "authenticated",
            "aud": "wrong_aud",
            "exp": now + 3600
        });

        let token = create_test_jwt(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, None);
        assert_eq!(res, Err(AuthError::InvalidAudience));
    }

    #[test]
    fn test_wrong_issuer_rejected() {
        let secret = "test_super_secret_jwt_key_12345678";
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_id_123",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://malicious.issuer.com"
        });

        let token = create_test_jwt(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, Some("https://trusted.supabase.co"));
        assert!(matches!(res, Err(AuthError::InvalidIssuer(_))));
    }

    pub(crate) static TEST_ENV_MUTEX: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    #[test]
    fn test_prefix_only_bypass_eliminated() {
        let _guard = TEST_ENV_MUTEX.lock();
        std::env::set_var(
            "ESA_LIVE_API_KEY",
            "esa_live_sec_real_production_secret_12345",
        );

        // An attacker creates an arbitrary string matching the prefix and length
        let fake_key = "esa_live_sec_00000000000000000000000000000000";
        assert!(!is_key_authorized(fake_key));

        // Hardcoded dev master key is rejected
        assert!(!is_key_authorized("esa_dev_master_key"));
        assert!(!is_key_authorized("esa_dev_anything"));

        // Only the real secret is authorized
        assert!(is_key_authorized(
            "esa_live_sec_real_production_secret_12345"
        ));

        std::env::remove_var("ESA_LIVE_API_KEY");
    }

    #[test]
    fn test_constant_time_comparison() {
        let a = b"exact_secret_matching_12345";
        let b = b"exact_secret_matching_12345";
        let c = b"different_secret_matching_12";

        assert!(constant_time_eq(a, b));
        assert!(!constant_time_eq(a, c));
    }
}
