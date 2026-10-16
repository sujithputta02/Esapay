use axum::{
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;
use uuid::Uuid;

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
#[allow(dead_code)]
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

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};

/// Key specification for Supabase JWT verification
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SupabaseVerificationKey<'a> {
    /// Symmetric HMAC key (HS256) using SUPABASE_JWT_SECRET
    Secret(&'a str),
    /// Asymmetric RSA public key in PEM format (RS256)
    RsaPem(&'a str),
    /// Auto-detect from environment (SUPABASE_JWT_SECRET for HS256, SUPABASE_JWT_PUBLIC_KEY for RS256)
    FromEnvironment,
}

/// Resolve the canonical Supabase issuer (e.g. https://<project>.supabase.co/auth/v1)
pub fn resolve_supabase_expected_issuer(param_issuer: Option<&str>) -> Option<String> {
    if let Some(param) = param_issuer {
        let clean = param.trim();
        if !clean.is_empty() {
            return Some(if clean.ends_with("/auth/v1") {
                clean.to_string()
            } else {
                format!("{}/auth/v1", clean.trim_end_matches('/'))
            });
        }
    }
    if let Ok(iss) = std::env::var("SUPABASE_JWT_ISSUER") {
        let clean = iss.trim();
        if !clean.is_empty() {
            return Some(clean.to_string());
        }
    }
    if let Ok(url) = std::env::var("SUPABASE_URL") {
        let clean = url.trim().trim_end_matches('/');
        if !clean.is_empty() {
            return Some(format!("{}/auth/v1", clean));
        }
    }
    None
}

fn map_jwt_error(err: jsonwebtoken::errors::Error) -> AuthError {
    use jsonwebtoken::errors::ErrorKind;
    match err.kind() {
        ErrorKind::ExpiredSignature => AuthError::TokenExpired,
        ErrorKind::ImmatureSignature => AuthError::TokenNotYetValid,
        ErrorKind::InvalidAudience => AuthError::InvalidAudience,
        ErrorKind::InvalidIssuer => {
            AuthError::InvalidIssuer("Issuer mismatch or missing".to_string())
        }
        ErrorKind::InvalidAlgorithmName | ErrorKind::InvalidAlgorithm => {
            AuthError::UnsupportedAlgorithm(format!("{:?}", err.kind()))
        }
        ErrorKind::InvalidSignature => AuthError::InvalidSignature,
        ErrorKind::InvalidToken => AuthError::InvalidTokenFormat,
        _ => AuthError::InvalidSignature,
    }
}

fn validate_claims_payload(
    claims: &SupabaseJwtClaims,
    expected_iss: &str,
) -> Result<(), AuthError> {
    // Subject check: must be non-empty
    if claims
        .sub
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        return Err(AuthError::MissingSubject);
    }

    // Exact issuer check
    match &claims.iss {
        Some(iss) => {
            if iss.trim() != expected_iss.trim() {
                return Err(AuthError::InvalidIssuer(iss.clone()));
            }
        }
        None => return Err(AuthError::InvalidIssuer("Missing issuer claim".to_string())),
    }

    Ok(())
}

/// Verify cryptographic signature and claims of a Supabase JWT (HS256 or RS256)
pub fn verify_supabase_jwt_with_key(
    token: &str,
    key_spec: SupabaseVerificationKey,
    expected_issuer: Option<&str>,
) -> Result<SupabaseJwtClaims, AuthError> {
    let clean_token = token.trim();
    if clean_token.is_empty() {
        return Err(AuthError::MissingToken);
    }

    let header = decode_header(clean_token).map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::InvalidAlgorithmName => {
            AuthError::UnsupportedAlgorithm("unsupported algorithm".to_string())
        }
        jsonwebtoken::errors::ErrorKind::Json(serde_err) => {
            let msg = serde_err.to_string();
            if msg.contains("unknown variant") || msg.contains("algorithm") {
                AuthError::UnsupportedAlgorithm(msg)
            } else {
                AuthError::InvalidTokenFormat
            }
        }
        jsonwebtoken::errors::ErrorKind::InvalidToken => AuthError::InvalidTokenFormat,
        _ => AuthError::InvalidTokenFormat,
    })?;

    let expected_iss = resolve_supabase_expected_issuer(expected_issuer).ok_or_else(|| {
        AuthError::InvalidIssuer(
            "Expected issuer is required for verification (configure SUPABASE_URL or pass expected_issuer)"
                .to_string(),
        )
    })?;

    match header.alg {
        Algorithm::HS256 => {
            let secret_str = match key_spec {
                SupabaseVerificationKey::Secret(s) => s.to_string(),
                SupabaseVerificationKey::FromEnvironment => std::env::var("SUPABASE_JWT_SECRET")
                    .map_err(|_| {
                        warn!(
                            "Supabase JWT verification failed: SUPABASE_JWT_SECRET not configured"
                        );
                        AuthError::InvalidSignature
                    })?,
                SupabaseVerificationKey::RsaPem(_) => {
                    return Err(AuthError::UnsupportedAlgorithm(
                        "Token uses HS256 but an RSA PEM key was supplied".to_string(),
                    ));
                }
            };

            let clean_secret = secret_str.trim();
            if clean_secret.is_empty() {
                warn!("Supabase JWT verification failed: secret is empty");
                return Err(AuthError::InvalidSignature);
            }

            let decoding_key = DecodingKey::from_secret(clean_secret.as_bytes());
            let mut validation = Validation::new(Algorithm::HS256);
            validation.validate_exp = true;
            validation.validate_nbf = true;
            validation.set_audience(&["authenticated"]);
            validation.set_issuer(&[&expected_iss]);

            let token_data = decode::<SupabaseJwtClaims>(clean_token, &decoding_key, &validation)
                .map_err(map_jwt_error)?;

            validate_claims_payload(&token_data.claims, &expected_iss)?;
            Ok(token_data.claims)
        }
        Algorithm::RS256 => {
            let pem_str = match key_spec {
                SupabaseVerificationKey::RsaPem(pem) => pem.to_string(),
                SupabaseVerificationKey::FromEnvironment => {
                    std::env::var("SUPABASE_JWT_PUBLIC_KEY").map_err(|_| {
                        warn!(
                        "Supabase JWT verification failed: SUPABASE_JWT_PUBLIC_KEY not configured"
                    );
                        AuthError::InvalidSignature
                    })?
                }
                SupabaseVerificationKey::Secret(_) => {
                    return Err(AuthError::UnsupportedAlgorithm(
                        "Token uses RS256 but an HMAC secret was supplied".to_string(),
                    ));
                }
            };

            let clean_pem = pem_str.trim();
            if clean_pem.is_empty() {
                warn!("Supabase JWT verification failed: RSA public key is empty");
                return Err(AuthError::InvalidSignature);
            }

            let decoding_key = DecodingKey::from_rsa_pem(clean_pem.as_bytes())
                .map_err(|_| AuthError::InvalidSignature)?;
            let mut validation = Validation::new(Algorithm::RS256);
            validation.validate_exp = true;
            validation.validate_nbf = true;
            validation.set_audience(&["authenticated"]);
            validation.set_issuer(&[&expected_iss]);

            let token_data = decode::<SupabaseJwtClaims>(clean_token, &decoding_key, &validation)
                .map_err(map_jwt_error)?;

            validate_claims_payload(&token_data.claims, &expected_iss)?;
            Ok(token_data.claims)
        }
        other => {
            warn!("Rejected JWT with unsupported algorithm: {:?}", other);
            Err(AuthError::UnsupportedAlgorithm(format!("{:?}", other)))
        }
    }
}

/// Verify cryptographic signature and claims of a Supabase HS256 JWT
#[allow(dead_code)]
pub fn verify_supabase_jwt(
    token: &str,
    secret: &str,
    expected_issuer: Option<&str>,
) -> Result<SupabaseJwtClaims, AuthError> {
    verify_supabase_jwt_with_key(
        token,
        SupabaseVerificationKey::Secret(secret),
        expected_issuer,
    )
}

/// Fallback for read-only backwards compatibility
pub fn parse_supabase_jwt(token: &str) -> Option<SupabaseJwtClaims> {
    let expected_iss = resolve_supabase_expected_issuer(None);
    verify_supabase_jwt_with_key(
        token,
        SupabaseVerificationKey::FromEnvironment,
        expected_iss.as_deref(),
    )
    .ok()
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
        let claims =
            verify_supabase_jwt_with_key(clean, SupabaseVerificationKey::FromEnvironment, None)?;
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
        if let Ok(claims) =
            verify_supabase_jwt_with_key(key, SupabaseVerificationKey::FromEnvironment, None)
        {
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
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use jsonwebtoken::{encode, EncodingKey, Header};

    fn generate_ephemeral_test_rsa_keys() -> Option<(String, String)> {
        let priv_out = std::process::Command::new("openssl")
            .args(["genrsa", "2048"])
            .output()
            .ok()?;
        if !priv_out.status.success() {
            return None;
        }
        let priv_pem = String::from_utf8(priv_out.stdout).ok()?;

        let mut child = std::process::Command::new("openssl")
            .args(["rsa", "-pubout"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .ok()?;

        use std::io::Write;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(priv_pem.as_bytes());
        }
        let pub_out = child.wait_with_output().ok()?;
        if !pub_out.status.success() {
            return None;
        }
        let pub_pem = String::from_utf8(pub_out.stdout).ok()?;

        Some((priv_pem, pub_pem))
    }

    fn create_hs256_token(secret: &str, claims: &serde_json::Value) -> String {
        encode(
            &Header::new(Algorithm::HS256),
            claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("Failed to encode test HS256 JWT")
    }

    fn create_rs256_token(priv_pem: &str, claims: &serde_json::Value) -> String {
        let key = EncodingKey::from_rsa_pem(priv_pem.as_bytes())
            .expect("Failed to parse test RSA private key");
        encode(&Header::new(Algorithm::RS256), claims, &key)
            .expect("Failed to encode test RS256 JWT")
    }

    #[test]
    fn test_valid_supabase_jwt_verification_hs256() {
        let secret = "test_mock_jwt_secret_for_tests_only";
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "b2c1d3e4-5f6a-7b8c-9d0e-1f2a3b4c5d6e",
            "email": "merchant@example.com",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "nbf": now - 10,
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token = create_hs256_token(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, Some("https://test.supabase.co"));
        assert!(
            res.is_ok(),
            "Valid HS256 JWT must pass verification: {:?}",
            res.err()
        );
        let verified = res.unwrap();
        assert_eq!(
            verified.sub,
            Some("b2c1d3e4-5f6a-7b8c-9d0e-1f2a3b4c5d6e".to_string())
        );
        assert_eq!(verified.email, Some("merchant@example.com".to_string()));
    }

    #[test]
    fn test_valid_supabase_jwt_verification_rs256() {
        let (priv_pem, pub_pem) = match generate_ephemeral_test_rsa_keys() {
            Some(keys) => keys,
            None => {
                eprintln!("Skipping RS256 test: openssl CLI unavailable in test environment");
                return;
            }
        };

        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_rsa_123456789",
            "email": "rsa_user@example.com",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "nbf": now - 10,
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token = create_rs256_token(&priv_pem, &claims);
        let res = verify_supabase_jwt_with_key(
            &token,
            SupabaseVerificationKey::RsaPem(&pub_pem),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert!(
            res.is_ok(),
            "Valid RS256 JWT must pass verification: {:?}",
            res.err()
        );
        let verified = res.unwrap();
        assert_eq!(verified.sub, Some("user_rsa_123456789".to_string()));
    }

    #[test]
    fn test_forged_rs256_jwt_rejected() {
        let (priv_pem1, _) = match generate_ephemeral_test_rsa_keys() {
            Some(keys) => keys,
            None => return,
        };
        let (_, pub_pem2) = match generate_ephemeral_test_rsa_keys() {
            Some(keys) => keys,
            None => return,
        };

        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_rsa_123456789",
            "email": "rsa_user@example.com",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token = create_rs256_token(&priv_pem1, &claims);
        let res = verify_supabase_jwt_with_key(
            &token,
            SupabaseVerificationKey::RsaPem(&pub_pem2),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert_eq!(res, Err(AuthError::InvalidSignature));
    }

    const TEST_JWT_SECRET: &str = "test_mock_jwt_secret_for_tests_only";
    const ATTACKER_JWT_SECRET: &str = "test_mock_attacker_key_for_tests_only";

    #[test]
    fn test_forged_jwt_signature_rejected() {
        let secret = TEST_JWT_SECRET;
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "attacker-id",
            "role": "admin",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://test.supabase.co/auth/v1"
        });

        // Signed with an attacker's key
        let forged_token = create_hs256_token(ATTACKER_JWT_SECRET, &claims);
        let res = verify_supabase_jwt(&forged_token, secret, Some("https://test.supabase.co"));
        assert_eq!(res, Err(AuthError::InvalidSignature));

        // Tampered payload
        let valid_token = create_hs256_token(secret, &claims);
        let parts: Vec<&str> = valid_token.split('.').collect();
        let tampered_payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "sub": "admin-elevated-id",
                "role": "service_role",
                "aud": "authenticated",
                "exp": now + 3600,
                "iss": "https://test.supabase.co/auth/v1"
            }))
            .unwrap(),
        );
        let tampered_token = format!("{}.{}.{}", parts[0], tampered_payload, parts[2]);
        let res_tampered =
            verify_supabase_jwt(&tampered_token, secret, Some("https://test.supabase.co"));
        assert_eq!(res_tampered, Err(AuthError::InvalidSignature));
    }

    #[test]
    fn test_expired_jwt_rejected() {
        let secret = TEST_JWT_SECRET;
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_id_123",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now - 100, // Expired
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token = create_hs256_token(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, Some("https://test.supabase.co"));
        assert_eq!(res, Err(AuthError::TokenExpired));
    }

    #[test]
    fn test_not_yet_valid_nbf_rejected() {
        let secret = TEST_JWT_SECRET;
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_id_123",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "nbf": now + 1200, // Not yet valid
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token = create_hs256_token(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, Some("https://test.supabase.co"));
        assert_eq!(res, Err(AuthError::TokenNotYetValid));
    }

    #[test]
    fn test_wrong_audience_rejected() {
        let secret = TEST_JWT_SECRET;
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_id_123",
            "role": "authenticated",
            "aud": "wrong_aud",
            "exp": now + 3600,
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token = create_hs256_token(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, Some("https://test.supabase.co"));
        assert_eq!(res, Err(AuthError::InvalidAudience));
    }

    #[test]
    fn test_wrong_issuer_rejected() {
        let secret = TEST_JWT_SECRET;
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_id_123",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://malicious.issuer.com/auth/v1"
        });

        let token = create_hs256_token(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, Some("https://trusted.supabase.co"));
        assert!(matches!(res, Err(AuthError::InvalidIssuer(_))));
    }

    #[test]
    fn test_missing_issuer_rejected() {
        let secret = TEST_JWT_SECRET;
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_id_123",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600
        });

        let token = create_hs256_token(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, Some("https://trusted.supabase.co"));
        assert!(matches!(res, Err(AuthError::InvalidIssuer(_))));
    }

    #[test]
    fn test_missing_subject_rejected() {
        let secret = TEST_JWT_SECRET;
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://trusted.supabase.co/auth/v1"
        });

        let token = create_hs256_token(secret, &claims);
        let res = verify_supabase_jwt(&token, secret, Some("https://trusted.supabase.co"));
        assert_eq!(res, Err(AuthError::MissingSubject));
    }

    #[test]
    fn test_unsupported_none_algorithm_rejected() {
        let now = chrono::Utc::now().timestamp();
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\",\"typ\":\"JWT\"}");
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "sub": "user_123",
                "role": "admin",
                "aud": "authenticated",
                "exp": now + 3600,
                "iss": "https://trusted.supabase.co/auth/v1"
            }))
            .unwrap(),
        );
        let none_token = format!("{}.{}.", header, payload);
        let res = verify_supabase_jwt(
            &none_token,
            TEST_JWT_SECRET,
            Some("https://trusted.supabase.co"),
        );
        assert!(matches!(res, Err(AuthError::UnsupportedAlgorithm(_))));
    }

    pub(crate) static TEST_ENV_MUTEX: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    #[test]
    fn test_prefix_only_bypass_eliminated() {
        let _guard = TEST_ENV_MUTEX.lock();
        std::env::set_var(
            "ESA_TEST_API_KEY",
            "esa_test_key_real_evaluation_secret_12345",
        );

        // An attacker creates an arbitrary string matching the prefix and length
        let fake_key = "esa_test_key_00000000000000000000000000000000";
        assert!(!is_key_authorized(fake_key));

        // Hardcoded dev master key is rejected
        assert!(!is_key_authorized("esa_dev_master_key"));
        assert!(!is_key_authorized("esa_dev_anything"));

        // Only the real secret is authorized
        assert!(is_key_authorized(
            "esa_test_key_real_evaluation_secret_12345"
        ));

        std::env::remove_var("ESA_TEST_API_KEY");
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
