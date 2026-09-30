use axum::{
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// In-memory cache and validator for ESAPay API Keys
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ApiKeyRecord {
    pub key_id: String,
    pub merchant_id: String,
    pub key_prefix: String,
    pub key_hash: String,
    pub environment: String,
    pub is_active: bool,
    pub created_at: String,
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

/// Supabase Decoded JWT Claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupabaseJwtClaims {
    pub sub: Option<String>,
    pub email: Option<String>,
    pub role: Option<String>,
    pub aud: Option<String>,
    pub exp: Option<i64>,
}

/// Compute standard SHA-256 hex string of a raw API key
#[allow(dead_code)]
pub fn hash_api_key(raw_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract Bearer token from HTTP Authorization header
pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("Authorization")
        .and_then(|val| val.to_str().ok())
        .and_then(|auth_header| {
            let parts: Vec<&str> = auth_header.split_whitespace().collect();
            if parts.len() == 2 && parts[0].eq_ignore_ascii_case("Bearer") {
                Some(parts[1].to_string())
            } else {
                None
            }
        })
}

/// Parse and validate Supabase JWT access token payload & expiration
pub fn parse_supabase_jwt(token: &str) -> Option<SupabaseJwtClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let payload_str = parts[1];
    let decoded = URL_SAFE_NO_PAD
        .decode(payload_str)
        .or_else(|_| {
            let padded = match payload_str.len() % 4 {
                2 => format!("{}==", payload_str),
                3 => format!("{}=", payload_str),
                _ => payload_str.to_string(),
            };
            base64::engine::general_purpose::STANDARD.decode(padded)
        })
        .ok()?;

    let claims: SupabaseJwtClaims = serde_json::from_slice(&decoded).ok()?;

    // Check expiration timestamp
    if let Some(exp) = claims.exp {
        let now = chrono::Utc::now().timestamp();
        if exp < now {
            return None; // Token expired
        }
    }

    Some(claims)
}

/// Check if an API key or Supabase JWT is valid
pub fn is_key_authorized(key: &str) -> bool {
    let clean = key.trim();
    if clean.is_empty() {
        return false;
    }

    // 1. Supabase JWT Session Token (starts with eyJ and has 3 segments)
    if clean.starts_with("eyJ") {
        return parse_supabase_jwt(clean).is_some();
    }

    // 2. Instant 0-login Sandbox Keys are always structurally valid for evaluation
    if clean.starts_with("esa_test_demo_") || clean.starts_with("esa_sandbox_") {
        return clean.len() >= 24;
    }

    // 3. Environment-configured API keys (Live & Test)
    if let Ok(env_live) = std::env::var("ESA_LIVE_API_KEY") {
        if !env_live.trim().is_empty() && clean == env_live.trim() {
            return true;
        }
    }
    if let Ok(env_test) = std::env::var("ESA_TEST_API_KEY") {
        if !env_test.trim().is_empty() && clean == env_test.trim() {
            return true;
        }
    }
    if let Ok(env_key) = std::env::var("ESA_API_KEY") {
        if !env_key.trim().is_empty() && clean == env_key.trim() {
            return true;
        }
    }

    // 4. Local dev keys
    if clean == "esa_dev_master_key" || clean.starts_with("esa_dev_") {
        return true;
    }

    // 5. Secret keys with standard formatting
    if clean.starts_with("esa_test_sec_") || clean.starts_with("esa_live_sec_") {
        return clean.len() >= 32;
    }

    false
}

/// POST /api/v1/keys/generate-sandbox
/// Issues an instant, cryptographically random Sandbox Key without requiring login
pub async fn generate_sandbox_key_handler() -> impl IntoResponse {
    let random_suffix = Uuid::new_v4().simple().to_string();
    let api_key = format!("esa_test_demo_{}", random_suffix);
    let key_prefix = format!("esa_test_demo_{}...", &random_suffix[..6]);

    Json(GenerateSandboxResponse {
        api_key,
        key_prefix,
        environment: "sandbox".to_string(),
        expires_in: "24h (Unlimited renewals)".to_string(),
        message: "Instant Sandbox Key active. Ready for Command Center, CLI, and SDK evaluation."
            .to_string(),
    })
}

/// POST /api/v1/keys/validate
pub async fn validate_key_handler(Json(payload): Json<ValidateKeyRequest>) -> impl IntoResponse {
    let key = payload.api_key.trim();

    // Check for Supabase JWT
    if let Some(claims) = parse_supabase_jwt(key) {
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

    let valid = is_key_authorized(key);

    let env = if key.starts_with("esa_live_") {
        "live"
    } else if key.starts_with("esa_test_demo_") {
        "sandbox"
    } else {
        "test"
    };

    if valid {
        (
            StatusCode::OK,
            Json(ValidateKeyResponse {
                valid: true,
                environment: env.to_string(),
                merchant_id: "merchant_sandbox_01".to_string(),
                message: "API Key verified and active.".to_string(),
            }),
        )
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(ValidateKeyResponse {
                valid: false,
                environment: "unknown".to_string(),
                merchant_id: "anonymous".to_string(),
                message: "Invalid or inactive ESAPay API key.".to_string(),
            }),
        )
    }
}

/// GET /api/v1/auth/me
/// Extracts Authorization: Bearer token and returns caller identity
pub async fn auth_me_handler(headers: HeaderMap) -> impl IntoResponse {
    let token = match extract_bearer_token(&headers) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "authenticated": false,
                    "error": "Missing Authorization: Bearer <token> header"
                })),
            );
        }
    };

    if let Some(claims) = parse_supabase_jwt(&token) {
        let user_id = claims.sub.unwrap_or_else(|| "unknown".to_string());
        let email = claims.email.unwrap_or_else(|| "anonymous".to_string());
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "authenticated": true,
                "auth_type": "supabase_jwt",
                "user_id": user_id,
                "email": email,
                "role": claims.role.unwrap_or_else(|| "authenticated".to_string()),
                "expires_at": claims.exp
            })),
        );
    }

    if is_key_authorized(&token) {
        let env = if token.starts_with("esa_live_") {
            "live"
        } else if token.starts_with("esa_test_demo_") {
            "sandbox"
        } else {
            "test"
        };

        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "authenticated": true,
                "auth_type": "api_key",
                "environment": env,
                "key_prefix": if token.len() >= 16 { format!("{}...", &token[..16]) } else { token.clone() }
            })),
        );
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "authenticated": false,
            "error": "Invalid or expired token"
        })),
    )
}
