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
    #[error("unknown or unresolvable key identifier (kid): {0}")]
    UnknownKeyId(String),
    #[error("JWKS key discovery failed: {0}")]
    KeyDiscoveryFailed(String),
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

use jsonwebtoken::{
    decode, decode_header,
    jwk::{AlgorithmParameters, JwkSet, KeyAlgorithm},
    Algorithm, DecodingKey, Validation,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Cached decoded cryptographic public key with associated algorithm
#[derive(Clone)]
pub struct CachedVerificationKey {
    pub decoding_key: DecodingKey,
    pub algorithm: Algorithm,
}

impl std::fmt::Debug for CachedVerificationKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedVerificationKey")
            .field("algorithm", &self.algorithm)
            .finish()
    }
}

/// Thread-safe bounded cache for Supabase JWKS keys with TTL and cooldown
#[derive(Clone)]
pub struct JwksKeyCache {
    keys: Arc<RwLock<HashMap<String, CachedVerificationKey>>>,
    last_fetched: Arc<RwLock<Option<Instant>>>,
    last_attempt: Arc<RwLock<Option<Instant>>>,
    ttl: Duration,
    cooldown: Duration,
    jwks_url: Option<String>,
}

fn fetch_jwks_endpoint(endpoint: &str, timeout: Duration) -> Result<String, AuthError> {
    let endpoint = endpoint.to_string();
    let runner = move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| {
                AuthError::KeyDiscoveryFailed(format!("HTTP client build failed: {}", e))
            })?;
        let resp = client.get(&endpoint).send().map_err(|e| {
            AuthError::KeyDiscoveryFailed(format!("Failed to fetch JWKS from endpoint: {}", e))
        })?;
        if !resp.status().is_success() {
            return Err(AuthError::KeyDiscoveryFailed(format!(
                "JWKS endpoint returned HTTP status {}",
                resp.status()
            )));
        }
        resp.text().map_err(|e| {
            AuthError::KeyDiscoveryFailed(format!("Failed to read JWKS response body: {}", e))
        })
    };

    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::spawn(runner)
            .join()
            .map_err(|_| AuthError::KeyDiscoveryFailed("JWKS fetch thread panicked".to_string()))?
    } else {
        runner()
    }
}

impl Default for JwksKeyCache {
    fn default() -> Self {
        Self::new()
    }
}

impl JwksKeyCache {
    pub fn new() -> Self {
        let ttl_secs = std::env::var("SUPABASE_JWKS_TTL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600); // 10 minutes TTL
        let cooldown_secs = std::env::var("SUPABASE_JWKS_COOLDOWN_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10); // 10 seconds cooldown between network refreshes

        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
            last_fetched: Arc::new(RwLock::new(None)),
            last_attempt: Arc::new(RwLock::new(None)),
            ttl: Duration::from_secs(ttl_secs),
            cooldown: Duration::from_secs(cooldown_secs),
            jwks_url: None,
        }
    }

    pub fn with_jwks_url(mut self, url: String) -> Self {
        self.jwks_url = Some(url);
        self
    }

    #[allow(dead_code)]
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    #[allow(dead_code)]
    pub fn with_cooldown(mut self, cooldown: Duration) -> Self {
        self.cooldown = cooldown;
        self
    }

    #[cfg(test)]
    pub fn expire_cache_for_test(&self) {
        *self.last_fetched.write() = Some(Instant::now() - self.ttl - Duration::from_secs(1));
    }

    #[cfg(test)]
    pub fn clear_cooldown_for_test(&self) {
        *self.last_attempt.write() = None;
    }

    pub fn load_jwk_set(&self, set: &JwkSet) -> Result<usize, AuthError> {
        let mut count = 0;
        let mut new_map = HashMap::new();
        for jwk in &set.keys {
            let alg = match jwk.common.key_algorithm {
                Some(KeyAlgorithm::RS256) => Algorithm::RS256,
                Some(KeyAlgorithm::ES256) => Algorithm::ES256,
                _ => match &jwk.algorithm {
                    AlgorithmParameters::RSA(_) => Algorithm::RS256,
                    AlgorithmParameters::EllipticCurve(_) => Algorithm::ES256,
                    _ => continue,
                },
            };
            if let Ok(decoding_key) = DecodingKey::from_jwk(jwk) {
                let kid = jwk.common.key_id.clone().unwrap_or_default();
                new_map.insert(
                    kid,
                    CachedVerificationKey {
                        decoding_key,
                        algorithm: alg,
                    },
                );
                count += 1;
            }
        }
        if count == 0 && !set.keys.is_empty() {
            return Err(AuthError::KeyDiscoveryFailed(
                "JWKS set contained keys but none had supported algorithms or valid formats"
                    .to_string(),
            ));
        }
        // Atomically replace active key set, removing old keys not present in new set
        *self.keys.write() = new_map;
        *self.last_fetched.write() = Some(Instant::now());
        Ok(count)
    }

    #[allow(dead_code)]
    pub fn load_jwks_json(&self, json_str: &str) -> Result<usize, AuthError> {
        let set: JwkSet = serde_json::from_str(json_str).map_err(|e| {
            AuthError::KeyDiscoveryFailed(format!("Failed to parse JWKS JSON: {}", e))
        })?;
        self.load_jwk_set(&set)
    }

    #[allow(dead_code)]
    pub fn insert_key(&self, kid: String, decoding_key: DecodingKey, alg: Algorithm) {
        let mut map = self.keys.write();
        map.insert(
            kid,
            CachedVerificationKey {
                decoding_key,
                algorithm: alg,
            },
        );
        *self.last_fetched.write() = Some(Instant::now());
    }

    pub fn resolve_key(
        &self,
        kid: Option<&str>,
        target_alg: Algorithm,
        custom_jwks_url: Option<&str>,
    ) -> Result<CachedVerificationKey, AuthError> {
        let target_kid = kid.unwrap_or("").trim();
        let now = Instant::now();

        // 1. Check in-memory cache if and only if the cache is currently fresh (within TTL)
        let is_fresh = self
            .last_fetched
            .read()
            .map(|t| now.duration_since(t) < self.ttl)
            .unwrap_or(false);

        if is_fresh {
            let keys = self.keys.read();
            if !target_kid.is_empty() {
                if let Some(key) = keys.get(target_kid) {
                    if key.algorithm == target_alg {
                        return Ok(key.clone());
                    } else {
                        return Err(AuthError::UnsupportedAlgorithm(format!(
                            "Key ID '{}' configured with algorithm {:?}, but token requires {:?}",
                            target_kid, key.algorithm, target_alg
                        )));
                    }
                }
                // target_kid not found in fresh cache: key might have rotated remotely, proceed to refresh
            } else {
                let matching: Vec<_> = keys
                    .values()
                    .filter(|k| k.algorithm == target_alg)
                    .collect();
                if matching.len() == 1 {
                    return Ok(matching[0].clone());
                } else if matching.len() > 1 {
                    return Err(AuthError::UnknownKeyId(format!(
                        "Multiple keys found with algorithm {:?}; kid is required",
                        target_alg
                    )));
                }
                // 0 matching keys: proceed to refresh
            }
        }

        // 2. Refresh needed: resolve URL and verify cooldown
        let url = custom_jwks_url
            .map(|s| s.to_string())
            .or_else(|| self.jwks_url.clone())
            .or_else(|| std::env::var("SUPABASE_JWKS_URL").ok())
            .or_else(|| {
                std::env::var("SUPABASE_URL").ok().map(|u| {
                    format!(
                        "{}/auth/v1/.well-known/jwks.json",
                        u.trim().trim_end_matches('/')
                    )
                })
            });

        let endpoint = match url {
            Some(u) => u,
            None => {
                if !is_fresh {
                    return Err(AuthError::KeyDiscoveryFailed(
                        "JWKS cache expired and no JWKS URL configured to refresh".to_string(),
                    ));
                } else {
                    return Err(AuthError::UnknownKeyId(target_kid.to_string()));
                }
            }
        };

        let allow_fetch = {
            let mut last = self.last_attempt.write();
            match *last {
                Some(prev) if now.duration_since(prev) < self.cooldown => false,
                _ => {
                    *last = Some(now);
                    true
                }
            }
        };

        if !allow_fetch {
            if !is_fresh {
                return Err(AuthError::KeyDiscoveryFailed(
                    "JWKS cache expired; refresh currently on cooldown following recent attempt"
                        .to_string(),
                ));
            } else {
                return Err(AuthError::UnknownKeyId(target_kid.to_string()));
            }
        }

        // Execute fetch without holding locks across network I/O
        let body = match fetch_jwks_endpoint(&endpoint, Duration::from_secs(4)) {
            Ok(b) => b,
            Err(e) => {
                if !is_fresh {
                    return Err(AuthError::KeyDiscoveryFailed(format!(
                        "Failed to refresh expired JWKS cache: {}",
                        e
                    )));
                } else {
                    return Err(AuthError::UnknownKeyId(format!(
                        "Key ID '{}' not found and JWKS refresh failed: {}",
                        target_kid, e
                    )));
                }
            }
        };

        if let Err(e) = self.load_jwks_json(&body) {
            if !is_fresh {
                return Err(AuthError::KeyDiscoveryFailed(format!(
                    "Failed to parse refreshed JWKS response: {}",
                    e
                )));
            } else {
                return Err(AuthError::UnknownKeyId(format!(
                    "Key ID '{}' not found and refreshed JWKS was invalid: {}",
                    target_kid, e
                )));
            }
        }

        // 3. Cache is now freshly populated after authoritative refresh
        let keys = self.keys.read();
        if !target_kid.is_empty() {
            if let Some(key) = keys.get(target_kid) {
                if key.algorithm == target_alg {
                    return Ok(key.clone());
                } else {
                    return Err(AuthError::UnsupportedAlgorithm(format!(
                        "Key ID '{}' has algorithm {:?} but token requires {:?}",
                        target_kid, key.algorithm, target_alg
                    )));
                }
            }
            return Err(AuthError::UnknownKeyId(target_kid.to_string()));
        }

        let matching: Vec<_> = keys
            .values()
            .filter(|k| k.algorithm == target_alg)
            .collect();
        if matching.len() == 1 {
            Ok(matching[0].clone())
        } else if matching.is_empty() {
            Err(AuthError::UnknownKeyId(format!(
                "No key found matching algorithm {:?}",
                target_alg
            )))
        } else {
            Err(AuthError::UnknownKeyId(format!(
                "Multiple keys found matching algorithm {:?}; kid is required",
                target_alg
            )))
        }
    }
}

static GLOBAL_JWKS_CACHE: parking_lot::Mutex<Option<Arc<JwksKeyCache>>> =
    parking_lot::Mutex::new(None);

pub fn get_global_jwks_cache() -> Arc<JwksKeyCache> {
    let mut lock = GLOBAL_JWKS_CACHE.lock();
    if let Some(cache) = lock.as_ref() {
        return cache.clone();
    }
    let url = std::env::var("SUPABASE_JWKS_URL").ok().or_else(|| {
        std::env::var("SUPABASE_URL").ok().map(|u| {
            format!(
                "{}/auth/v1/.well-known/jwks.json",
                u.trim().trim_end_matches('/')
            )
        })
    });
    let mut cache = JwksKeyCache::new();
    if let Some(u) = url {
        cache = cache.with_jwks_url(u);
    }
    let arc = Arc::new(cache);
    *lock = Some(arc.clone());
    arc
}

/// Key specification for Supabase JWT verification
#[derive(Clone)]
#[allow(dead_code)]
pub enum SupabaseVerificationKey<'a> {
    /// Symmetric HMAC key (HS256) using explicit secret
    Secret(&'a str),
    /// Asymmetric RSA public key in PEM format (RS256)
    RsaPem(&'a str),
    /// Asymmetric EC public key in PEM format (ES256)
    EcPem(&'a str),
    /// Preloaded JWK Set
    JwkSet(&'a JwkSet),
    /// Preconfigured JWKS cache reference
    JwksCache(Arc<JwksKeyCache>),
    /// Auto-detect from environment (prefers asymmetric JWKS / public keys; allows HS256 secret only if explicitly set)
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

/// Verify cryptographic signature and claims of a Supabase JWT (HS256, RS256, or ES256)
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

    let kid = header.kid.as_deref();
    let expected_iss = resolve_supabase_expected_issuer(expected_issuer).ok_or_else(|| {
        AuthError::InvalidIssuer(
            "Expected issuer is required for verification (configure SUPABASE_URL or pass expected_issuer)"
                .to_string(),
        )
    })?;

    let (decoding_key, validation_alg) = match header.alg {
        Algorithm::HS256 => {
            let secret_str = match &key_spec {
                SupabaseVerificationKey::Secret(s) => s.to_string(),
                SupabaseVerificationKey::FromEnvironment => {
                    std::env::var("SUPABASE_JWT_SECRET").map_err(|_| {
                        warn!("Supabase JWT verification failed: HS256 token received but SUPABASE_JWT_SECRET not configured");
                        AuthError::UnsupportedAlgorithm(
                            "HS256 symmetric signing is only allowed when SUPABASE_JWT_SECRET is explicitly configured".to_string()
                        )
                    })?
                }
                _ => {
                    return Err(AuthError::UnsupportedAlgorithm(
                        "Token uses HS256 but key specification is configured for asymmetric verification".to_string()
                    ));
                }
            };
            let clean_secret = secret_str.trim();
            if clean_secret.is_empty() {
                warn!("Supabase JWT verification failed: secret is empty");
                return Err(AuthError::InvalidSignature);
            }
            (
                DecodingKey::from_secret(clean_secret.as_bytes()),
                Algorithm::HS256,
            )
        }
        Algorithm::RS256 => {
            let key = match &key_spec {
                SupabaseVerificationKey::RsaPem(pem) => {
                    let clean = pem.trim();
                    if clean.is_empty() {
                        return Err(AuthError::InvalidSignature);
                    }
                    DecodingKey::from_rsa_pem(clean.as_bytes())
                        .map_err(|_| AuthError::InvalidSignature)?
                }
                SupabaseVerificationKey::JwkSet(set) => {
                    let cache = JwksKeyCache::new();
                    let _ = cache.load_jwk_set(set);
                    cache.resolve_key(kid, Algorithm::RS256, None)?.decoding_key
                }
                SupabaseVerificationKey::JwksCache(cache) => {
                    cache.resolve_key(kid, Algorithm::RS256, None)?.decoding_key
                }
                SupabaseVerificationKey::FromEnvironment => {
                    if let Ok(pem) = std::env::var("SUPABASE_JWT_PUBLIC_KEY") {
                        let clean = pem.trim();
                        if !clean.is_empty() {
                            DecodingKey::from_rsa_pem(clean.as_bytes())
                                .map_err(|_| AuthError::InvalidSignature)?
                        } else {
                            let cache = get_global_jwks_cache();
                            cache.resolve_key(kid, Algorithm::RS256, None)?.decoding_key
                        }
                    } else {
                        let cache = get_global_jwks_cache();
                        cache.resolve_key(kid, Algorithm::RS256, None)?.decoding_key
                    }
                }
                SupabaseVerificationKey::Secret(_) | SupabaseVerificationKey::EcPem(_) => {
                    return Err(AuthError::UnsupportedAlgorithm(
                        "Token uses RS256 but key specification is not RSA or JWKS".to_string(),
                    ));
                }
            };
            (key, Algorithm::RS256)
        }
        Algorithm::ES256 => {
            let key = match &key_spec {
                SupabaseVerificationKey::EcPem(pem) => {
                    let clean = pem.trim();
                    if clean.is_empty() {
                        return Err(AuthError::InvalidSignature);
                    }
                    DecodingKey::from_ec_pem(clean.as_bytes())
                        .map_err(|_| AuthError::InvalidSignature)?
                }
                SupabaseVerificationKey::JwkSet(set) => {
                    let cache = JwksKeyCache::new();
                    let _ = cache.load_jwk_set(set);
                    cache.resolve_key(kid, Algorithm::ES256, None)?.decoding_key
                }
                SupabaseVerificationKey::JwksCache(cache) => {
                    cache.resolve_key(kid, Algorithm::ES256, None)?.decoding_key
                }
                SupabaseVerificationKey::FromEnvironment => {
                    if let Ok(pem) = std::env::var("SUPABASE_JWT_EC_PUBLIC_KEY") {
                        let clean = pem.trim();
                        if !clean.is_empty() {
                            DecodingKey::from_ec_pem(clean.as_bytes())
                                .map_err(|_| AuthError::InvalidSignature)?
                        } else {
                            let cache = get_global_jwks_cache();
                            cache.resolve_key(kid, Algorithm::ES256, None)?.decoding_key
                        }
                    } else {
                        let cache = get_global_jwks_cache();
                        cache.resolve_key(kid, Algorithm::ES256, None)?.decoding_key
                    }
                }
                SupabaseVerificationKey::Secret(_) | SupabaseVerificationKey::RsaPem(_) => {
                    return Err(AuthError::UnsupportedAlgorithm(
                        "Token uses ES256 but key specification is not EC or JWKS".to_string(),
                    ));
                }
            };
            (key, Algorithm::ES256)
        }
        other => {
            warn!("Rejected JWT with unsupported algorithm: {:?}", other);
            return Err(AuthError::UnsupportedAlgorithm(format!("{:?}", other)));
        }
    };

    let mut validation = Validation::new(validation_alg);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.set_audience(&["authenticated"]);
    validation.set_issuer(&[&expected_iss]);

    let token_data = decode::<SupabaseJwtClaims>(clean_token, &decoding_key, &validation)
        .map_err(map_jwt_error)?;

    validate_claims_payload(&token_data.claims, &expected_iss)?;
    Ok(token_data.claims)
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

    fn start_mock_jwks_server() -> (
        String,
        Arc<parking_lot::RwLock<String>>,
        Arc<std::sync::atomic::AtomicBool>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use std::io::{Read, Write};
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral test port");
        let port = listener.local_addr().unwrap().port();
        let body_holder = Arc::new(parking_lot::RwLock::new(String::new()));
        let return_500 = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let request_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let body_clone = body_holder.clone();
        let return_500_clone = return_500.clone();
        let request_count_clone = request_count.clone();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => break,
                };
                request_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);

                if return_500_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes());
                } else {
                    let body = body_clone.read().clone();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
                let _ = stream.flush();
            }
        });

        (
            format!("http://127.0.0.1:{}/jwks.json", port),
            body_holder,
            return_500,
            request_count,
        )
    }

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

    fn generate_ephemeral_test_ec_keys() -> Option<(String, String)> {
        let sec1_out = std::process::Command::new("openssl")
            .args(["ecparam", "-name", "prime256v1", "-genkey", "-noout"])
            .output()
            .ok()?;
        if !sec1_out.status.success() {
            return None;
        }

        let mut pkcs8_child = std::process::Command::new("openssl")
            .args(["pkcs8", "-topk8", "-nocrypt"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        use std::io::Write;
        if let Some(mut stdin) = pkcs8_child.stdin.take() {
            let _ = stdin.write_all(&sec1_out.stdout);
        }
        let priv_out = pkcs8_child.wait_with_output().ok()?;
        if !priv_out.status.success() {
            return None;
        }
        let priv_pem = String::from_utf8(priv_out.stdout).ok()?;

        let mut pub_child = std::process::Command::new("openssl")
            .args(["ec", "-pubout"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        if let Some(mut stdin) = pub_child.stdin.take() {
            let _ = stdin.write_all(&sec1_out.stdout);
        }
        let pub_out = pub_child.wait_with_output().ok()?;
        if !pub_out.status.success() {
            return None;
        }
        let pub_pem = String::from_utf8(pub_out.stdout).ok()?;

        Some((priv_pem, pub_pem))
    }

    fn create_es256_token(priv_pem: &str, claims: &serde_json::Value, kid: Option<&str>) -> String {
        let key = EncodingKey::from_ec_pem(priv_pem.as_bytes())
            .expect("Failed to parse test EC private key");
        let mut header = Header::new(Algorithm::ES256);
        header.kid = kid.map(|s| s.to_string());
        encode(&header, claims, &key).expect("Failed to encode test ES256 JWT")
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

    #[test]
    fn test_valid_supabase_jwt_verification_es256() {
        let (priv_pem, pub_pem) = match generate_ephemeral_test_ec_keys() {
            Some(keys) => keys,
            None => {
                eprintln!("Skipping ES256 test: openssl CLI unavailable in test environment");
                return;
            }
        };

        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_es256_987654321",
            "email": "es256_merchant@example.com",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "nbf": now - 10,
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token = create_es256_token(&priv_pem, &claims, Some("ec-key-1"));
        let res = verify_supabase_jwt_with_key(
            &token,
            SupabaseVerificationKey::EcPem(&pub_pem),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert!(
            res.is_ok(),
            "Valid ES256 JWT must pass verification: {:?}",
            res.err()
        );
        let verified = res.unwrap();
        assert_eq!(verified.sub, Some("user_es256_987654321".to_string()));
    }

    #[test]
    fn test_forged_es256_jwt_rejected() {
        let (priv_pem1, _) = match generate_ephemeral_test_ec_keys() {
            Some(keys) => keys,
            None => return,
        };
        let (_, pub_pem2) = match generate_ephemeral_test_ec_keys() {
            Some(keys) => keys,
            None => return,
        };

        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_es256_hacker",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token = create_es256_token(&priv_pem1, &claims, None);
        let res = verify_supabase_jwt_with_key(
            &token,
            SupabaseVerificationKey::EcPem(&pub_pem2),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert_eq!(res, Err(AuthError::InvalidSignature));
    }

    #[test]
    fn test_jwks_cache_resolution_and_key_rotation() {
        let (priv_pem_v1, pub_pem_v1) = match generate_ephemeral_test_ec_keys() {
            Some(keys) => keys,
            None => return,
        };
        let (priv_pem_v2, pub_pem_v2) = match generate_ephemeral_test_ec_keys() {
            Some(keys) => keys,
            None => return,
        };

        let cache = Arc::new(JwksKeyCache::new());

        // 1. Initially cache only contains key v1
        let dec_v1 = DecodingKey::from_ec_pem(pub_pem_v1.as_bytes()).unwrap();
        cache.insert_key("kid-v1".to_string(), dec_v1, Algorithm::ES256);

        let now = chrono::Utc::now().timestamp();
        let claims_v1 = serde_json::json!({
            "sub": "user_rotation_test",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://test.supabase.co/auth/v1"
        });

        let token_v1 = create_es256_token(&priv_pem_v1, &claims_v1, Some("kid-v1"));
        let res_v1 = verify_supabase_jwt_with_key(
            &token_v1,
            SupabaseVerificationKey::JwksCache(cache.clone()),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert!(res_v1.is_ok(), "Token with kid-v1 must pass verification");

        // 2. Token arrives with rotated key kid-v2 before cache has it -> fails with UnknownKeyId
        let claims_v2 = serde_json::json!({
            "sub": "user_rotation_test",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://test.supabase.co/auth/v1"
        });
        let token_v2 = create_es256_token(&priv_pem_v2, &claims_v2, Some("kid-v2"));
        let res_v2_before = verify_supabase_jwt_with_key(
            &token_v2,
            SupabaseVerificationKey::JwksCache(cache.clone()),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert_eq!(
            res_v2_before,
            Err(AuthError::UnknownKeyId("kid-v2".to_string()))
        );

        // 3. Key rotation occurs: rotated key v2 is loaded into cache
        let dec_v2 = DecodingKey::from_ec_pem(pub_pem_v2.as_bytes()).unwrap();
        cache.insert_key("kid-v2".to_string(), dec_v2, Algorithm::ES256);

        let res_v2_after = verify_supabase_jwt_with_key(
            &token_v2,
            SupabaseVerificationKey::JwksCache(cache.clone()),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert!(
            res_v2_after.is_ok(),
            "Rotated token with kid-v2 must pass verification after rotation"
        );

        // 4. Old token with kid-v1 still validates (graceful key overlap during rotation)
        let res_v1_overlap = verify_supabase_jwt_with_key(
            &token_v1,
            SupabaseVerificationKey::JwksCache(cache.clone()),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert!(
            res_v1_overlap.is_ok(),
            "Existing key v1 remains valid during key rotation overlap"
        );

        // 5. Cache TTL expires -> cached keys cannot be trusted without refresh
        cache.expire_cache_for_test();
        let res_v1_expired = verify_supabase_jwt_with_key(
            &token_v1,
            SupabaseVerificationKey::JwksCache(cache.clone()),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert!(
            matches!(res_v1_expired, Err(AuthError::KeyDiscoveryFailed(_))),
            "Expired cache without refresh endpoint must fail closed: {:?}",
            res_v1_expired
        );

        // 6. Refreshed JWKS set replaces active keys (kid-v1 removed, only kid-v2 present)
        let ec_jwks_v2_only = r#"{
            "keys": [
                {
                    "kty": "EC",
                    "crv": "P-256",
                    "kid": "kid-v2",
                    "alg": "ES256",
                    "x": "f83OJ3D2xFmTNgTeib43uVrn8UWCazaEvbQDolZ_2QI",
                    "y": "x_daQauBhQ0tZxFlGQM2Q_Gfiw02z9q0aEnW65GQ-hs"
                }
            ]
        }"#;
        let count = cache
            .load_jwks_json(ec_jwks_v2_only)
            .expect("Valid JWKS JSON must parse");
        assert_eq!(count, 1);

        // Kid-v2 is present in refreshed set
        let res_v2_resolved = cache.resolve_key(Some("kid-v2"), Algorithm::ES256, None);
        assert!(res_v2_resolved.is_ok(), "Rotated kid-v2 must be resolved");

        // Old key kid-v1 was removed during refresh and is now strictly rejected!
        let res_v1_after_refresh = cache.resolve_key(Some("kid-v1"), Algorithm::ES256, None);
        assert_eq!(
            res_v1_after_refresh.err(),
            Some(AuthError::UnknownKeyId("kid-v1".to_string())),
            "Old key kid-v1 must be rejected after removal from refreshed JWKS"
        );
    }

    #[test]
    fn test_jwks_network_refresh_rotation_and_failed_refresh_safety() {
        let (server_url, body_holder, return_500, request_count) = start_mock_jwks_server();

        let jwks_v1 = r#"{
            "keys": [
                {
                    "kty": "EC",
                    "crv": "P-256",
                    "kid": "server-kid-1",
                    "alg": "ES256",
                    "x": "f83OJ3D2xFmTNgTeib43uVrn8UWCazaEvbQDolZ_2QI",
                    "y": "x_daQauBhQ0tZxFlGQM2Q_Gfiw02z9q0aEnW65GQ-hs"
                }
            ]
        }"#;
        *body_holder.write() = jwks_v1.to_string();

        let cache = Arc::new(JwksKeyCache::new().with_jwks_url(server_url));

        // 1. First resolution: cache is cold, triggers initial network fetch
        let key1 = cache.resolve_key(Some("server-kid-1"), Algorithm::ES256, None);
        assert!(
            key1.is_ok(),
            "Initial JWKS fetch must resolve key: {:?}",
            key1.err()
        );
        assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        // 2. Fresh cached key works without contacting the network
        let key1_cached = cache.resolve_key(Some("server-kid-1"), Algorithm::ES256, None);
        assert!(key1_cached.is_ok(), "Fresh cached key must succeed");
        assert_eq!(
            request_count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "Fresh cached key must NOT trigger extra network requests"
        );

        // 3. Rotate keys on server: server now returns server-kid-2 only (server-kid-1 revoked)
        let jwks_v2 = r#"{
            "keys": [
                {
                    "kty": "EC",
                    "crv": "P-256",
                    "kid": "server-kid-2",
                    "alg": "ES256",
                    "x": "f83OJ3D2xFmTNgTeib43uVrn8UWCazaEvbQDolZ_2QI",
                    "y": "x_daQauBhQ0tZxFlGQM2Q_Gfiw02z9q0aEnW65GQ-hs"
                }
            ]
        }"#;
        *body_holder.write() = jwks_v2.to_string();

        // 4. Cache expires: must refresh before existing key is trusted again
        cache.expire_cache_for_test();
        cache.clear_cooldown_for_test();

        // Requesting old key server-kid-1 triggers refresh, but server no longer advertises it
        let old_key_res = cache.resolve_key(Some("server-kid-1"), Algorithm::ES256, None);
        assert_eq!(
            old_key_res.err(),
            Some(AuthError::UnknownKeyId("server-kid-1".to_string())),
            "Old key must be rejected after removal from authoritative JWKS"
        );
        assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 2);

        // Requesting new key server-kid-2 succeeds from the freshly populated cache
        let new_key_res = cache.resolve_key(Some("server-kid-2"), Algorithm::ES256, None);
        assert!(
            new_key_res.is_ok(),
            "New key server-kid-2 must be available"
        );

        // 5. Failed refresh does not silently trust an expired or revoked key
        cache.expire_cache_for_test();
        cache.clear_cooldown_for_test();
        return_500.store(true, std::sync::atomic::Ordering::SeqCst);

        let failed_refresh_res = cache.resolve_key(Some("server-kid-2"), Algorithm::ES256, None);
        assert!(
            matches!(failed_refresh_res, Err(AuthError::KeyDiscoveryFailed(_))),
            "Failed refresh must return KeyDiscoveryFailed and never trust expired key: {:?}",
            failed_refresh_res
        );

        // 6. Unknown kid fails safely
        cache.clear_cooldown_for_test();
        return_500.store(false, std::sync::atomic::Ordering::SeqCst);
        let unknown_res = cache.resolve_key(Some("non_existent_kid"), Algorithm::ES256, None);
        assert_eq!(
            unknown_res.err(),
            Some(AuthError::UnknownKeyId("non_existent_kid".to_string()))
        );

        // 7. Unsupported algorithm mismatch fails safely
        let alg_mismatch_res = cache.resolve_key(Some("server-kid-2"), Algorithm::RS256, None);
        assert!(
            matches!(alg_mismatch_res, Err(AuthError::UnsupportedAlgorithm(_))),
            "Algorithm mismatch must return UnsupportedAlgorithm: {:?}",
            alg_mismatch_res
        );
    }

    #[test]
    fn test_hs256_token_rejected_when_asymmetric_only() {
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "user_hs256_attack",
            "role": "authenticated",
            "aud": "authenticated",
            "exp": now + 3600,
            "iss": "https://test.supabase.co/auth/v1"
        });

        // Attacker creates HS256 token signed with symmetric secret
        let token = create_hs256_token("attacker_hmac_secret", &claims);

        // Verifier configured with EC PEM
        let (_, pub_pem) = generate_ephemeral_test_ec_keys().unwrap();
        let res = verify_supabase_jwt_with_key(
            &token,
            SupabaseVerificationKey::EcPem(&pub_pem),
            Some("https://test.supabase.co/auth/v1"),
        );
        assert!(matches!(res, Err(AuthError::UnsupportedAlgorithm(_))));
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
