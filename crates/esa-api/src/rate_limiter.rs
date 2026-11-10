use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter as GovRateLimiter};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Type alias for rate limit rejection responses
pub type RateLimitError = (
    StatusCode,
    [(axum::http::header::HeaderName, &'static str); 1],
    Json<serde_json::Value>,
);

/// Maximum number of active tracked keys in keyed rate limiters to prevent unbounded memory growth
const MAX_KEYED_ENTRIES: usize = 10_000;

/// Represents an IPv4 or IPv6 CIDR block for trusted proxy IP verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpCidr {
    V4 { network: u32, mask: u32 },
    V6 { network: u128, mask: u128 },
}

impl IpCidr {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let (ip_str, prefix_len_opt) = match s.split_once('/') {
            Some((ip, prefix)) => (ip.trim(), Some(prefix.trim().parse::<u32>().ok()?)),
            None => (s, None),
        };

        if let Ok(ipv4) = ip_str.parse::<Ipv4Addr>() {
            let prefix_len = prefix_len_opt.unwrap_or(32);
            if prefix_len > 32 {
                return None;
            }
            let mask = if prefix_len == 0 {
                0
            } else {
                !0u32 << (32 - prefix_len)
            };
            let network = u32::from(ipv4) & mask;
            Some(IpCidr::V4 { network, mask })
        } else if let Ok(ipv6) = ip_str.parse::<Ipv6Addr>() {
            let prefix_len = prefix_len_opt.unwrap_or(128);
            if prefix_len > 128 {
                return None;
            }
            let mask = if prefix_len == 0 {
                0
            } else {
                !0u128 << (128 - prefix_len)
            };
            let network = u128::from(ipv6) & mask;
            Some(IpCidr::V6 { network, mask })
        } else {
            None
        }
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self, ip) {
            (IpCidr::V4 { network, mask }, IpAddr::V4(ipv4)) => {
                (u32::from(ipv4) & mask) == *network
            }
            (IpCidr::V6 { network, mask }, IpAddr::V6(ipv6)) => {
                (u128::from(ipv6) & mask) == *network
            }
            _ => false,
        }
    }
}

/// Configuration for trusted reverse proxy networks.
#[derive(Debug, Clone)]
pub struct TrustedProxyConfig {
    cidrs: Vec<IpCidr>,
}

impl TrustedProxyConfig {
    pub fn from_env() -> Self {
        let val = std::env::var("TRUSTED_PROXIES").unwrap_or_default();
        Self::parse(&val)
    }

    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("false") || s == "0" {
            return Self { cidrs: Vec::new() };
        }

        if s.eq_ignore_ascii_case("true") || s == "1" {
            // Default loopback and RFC 1918 / private networks
            let defaults = [
                "127.0.0.0/8",
                "10.0.0.0/8",
                "172.16.0.0/12",
                "192.168.0.0/16",
                "169.254.0.0/16",
                "::1/128",
                "fc00::/7",
                "fe80::/10",
            ];
            let cidrs = defaults.iter().filter_map(|c| IpCidr::parse(c)).collect();
            return Self { cidrs };
        }

        let cidrs = s.split(',').filter_map(IpCidr::parse).collect();
        Self { cidrs }
    }

    pub fn is_trusted(&self, ip: IpAddr) -> bool {
        self.cidrs.iter().any(|cidr| cidr.contains(ip))
    }
}

/// Resolves the authentic client IP address using peer address and trusted reverse proxy hops.
///
/// Follows standard reverse-proxy security rules:
/// 1. The direct socket peer IP is used unless it matches a specifically configured trusted proxy CIDR.
/// 2. If the direct peer is a trusted proxy, forwarded headers are evaluated.
/// 3. In X-Forwarded-For, hops are traversed right-to-left (most recent proxy to earliest).
///    The traversal stops at the first untrusted IP, preventing forged X-Forwarded-For spoofing.
/// 4. Arbitrary unauthenticated headers (e.g. fake tokens or client-forged XFF) are never trusted.
pub fn resolve_client_ip(
    peer_ip: Option<IpAddr>,
    headers: &HeaderMap,
    proxy_config: &TrustedProxyConfig,
) -> IpAddr {
    let peer = peer_ip.unwrap_or_else(|| IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));

    // Direct peer is untrusted: ignore all forwarding headers
    if !proxy_config.is_trusted(peer) {
        return peer;
    }

    // Direct peer is a trusted proxy. Check CF-Connecting-IP first if valid
    if let Some(cf_ip_str) = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
    {
        if let Ok(cf_ip) = cf_ip_str.trim().parse::<IpAddr>() {
            return cf_ip;
        }
    }

    // Parse X-Forwarded-For using trusted-hop right-to-left traversal
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        let hops: Vec<IpAddr> = xff
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<IpAddr>().ok())
            .collect();

        if !hops.is_empty() {
            for hop in hops.iter().rev() {
                if !proxy_config.is_trusted(*hop) {
                    return *hop;
                }
            }
            // If all hops are trusted proxies, the earliest hop is the client
            return hops[0];
        }
    }

    peer
}

/// Safely extracts client source identifier for rate limiting.
///
/// Priority:
/// 1. Verified authenticated identity (e.g. `merchant_id`), if caller is authenticated.
/// 2. Verified client IP address resolved via `resolve_client_ip`.
///
/// Unauthenticated `Authorization` or `X-API-Key` headers are NEVER used as rate-limiting identities,
/// preventing attackers from generating arbitrary fake tokens to bypass per-client buckets.
pub fn extract_client_source_identifier(
    peer_addr: Option<SocketAddr>,
    headers: &HeaderMap,
    verified_identity: Option<&str>,
) -> String {
    if let Some(id) = verified_identity.filter(|s| !s.trim().is_empty()) {
        return format!("auth_merchant:{}", id.trim());
    }

    let proxy_config = TrustedProxyConfig::from_env();
    let client_ip = resolve_client_ip(peer_addr.map(|sa| sa.ip()), headers, &proxy_config);
    format!("ip:{}", client_ip)
}

/// Backwards-compatible helper when peer address is not available.
#[allow(dead_code)]
pub fn extract_client_source_identifier_from_headers(headers: &HeaderMap) -> String {
    extract_client_source_identifier(None, headers, None)
}

/// Token-Bucket Rate Limiter for ESAPay API routes.
/// Combines per-process aggregate circuit breaking with per-client source rate limiting.
///
/// Note: The aggregate rate limiter is per-process (in-memory) and provides node-level protection.
/// It is not cluster-wide or distributed.
#[derive(Clone)]
pub struct ApiRateLimiter {
    checkout_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    aggregate_keys_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    client_gen_limiter: Arc<GovRateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>,
    client_val_limiter: Arc<GovRateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>,
    demo_limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    call_count: Arc<AtomicU64>,
}

impl ApiRateLimiter {
    pub fn new() -> Self {
        Self::new_with_all_quotas(30, 60, 20, 40, 2, 5, 10, 20, 5, 10)
    }

    #[allow(dead_code)]
    pub fn new_with_quotas(
        checkout_rps: u32,
        checkout_burst: u32,
        keys_rps: u32,
        keys_burst: u32,
        demo_rps: u32,
        demo_burst: u32,
    ) -> Self {
        Self::new_with_all_quotas(
            checkout_rps,
            checkout_burst,
            keys_rps,
            keys_burst,
            keys_rps.clamp(1, 5),
            keys_burst.max(1),
            keys_rps.saturating_mul(2).max(1),
            keys_burst.saturating_mul(2).max(1),
            demo_rps,
            demo_burst,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_all_quotas(
        checkout_rps: u32,
        checkout_burst: u32,
        aggregate_keys_rps: u32,
        aggregate_keys_burst: u32,
        client_gen_rps: u32,
        client_gen_burst: u32,
        client_val_rps: u32,
        client_val_burst: u32,
        demo_rps: u32,
        demo_burst: u32,
    ) -> Self {
        let checkout_quota = Quota::per_second(NonZeroU32::new(checkout_rps).unwrap())
            .allow_burst(NonZeroU32::new(checkout_burst).unwrap());
        let aggregate_keys_quota = Quota::per_second(NonZeroU32::new(aggregate_keys_rps).unwrap())
            .allow_burst(NonZeroU32::new(aggregate_keys_burst).unwrap());
        let client_gen_quota = Quota::per_second(NonZeroU32::new(client_gen_rps).unwrap())
            .allow_burst(NonZeroU32::new(client_gen_burst).unwrap());
        let client_val_quota = Quota::per_second(NonZeroU32::new(client_val_rps).unwrap())
            .allow_burst(NonZeroU32::new(client_val_burst).unwrap());
        let demo_quota = Quota::per_second(NonZeroU32::new(demo_rps).unwrap())
            .allow_burst(NonZeroU32::new(demo_burst).unwrap());

        Self {
            checkout_limiter: Arc::new(GovRateLimiter::direct(checkout_quota)),
            aggregate_keys_limiter: Arc::new(GovRateLimiter::direct(aggregate_keys_quota)),
            client_gen_limiter: Arc::new(GovRateLimiter::keyed(client_gen_quota)),
            client_val_limiter: Arc::new(GovRateLimiter::keyed(client_val_quota)),
            demo_limiter: Arc::new(GovRateLimiter::direct(demo_quota)),
            call_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn check_checkout(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.checkout_limiter.check().is_err() {
            Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Payment checkout rate limit exceeded. Please back off and retry shortly."
                })),
            ))
        } else {
            Ok(())
        }
    }

    /// Aggregate check for API key operations
    #[allow(dead_code)]
    pub fn check_keys(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.aggregate_keys_limiter.check().is_err() {
            Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Process-level API key operation rate limit exceeded. Too many requests, please back off and retry shortly."
                })),
            ))
        } else {
            Ok(())
        }
    }

    #[allow(clippy::manual_is_multiple_of)]
    fn bound_and_sanitize_key(
        &self,
        limiter: &GovRateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>,
        client_key: &str,
    ) -> String {
        // Periodically prune expired entries
        if self.call_count.fetch_add(1, Ordering::Relaxed) % 256 == 0 {
            limiter.retain_recent();
        }

        if limiter.len() > MAX_KEYED_ENTRIES {
            limiter.retain_recent();
            limiter.shrink_to_fit();
        }

        if limiter.len() > MAX_KEYED_ENTRIES {
            // Memory bound protection: if active unique identities exceed limit,
            // fall back to a shared overflow bucket to prevent memory exhaustion
            "overflow_client".to_string()
        } else {
            client_key.to_string()
        }
    }

    /// Per-source rate limit check for sandbox key generation with process-local aggregate safeguard
    pub fn check_generate_sandbox(&self, client_key: &str) -> Result<(), RateLimitError> {
        if self.aggregate_keys_limiter.check().is_err() {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, "1")],
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Process-level aggregate API key operation rate limit exceeded. Please back off and retry shortly."
                })),
            ));
        }

        let bounded_key = self.bound_and_sanitize_key(&self.client_gen_limiter, client_key);

        if self.client_gen_limiter.check_key(&bounded_key).is_err() {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, "1")],
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Sandbox API key generation rate limit exceeded for this client. Please back off and retry shortly."
                })),
            ));
        }

        Ok(())
    }

    /// Per-source rate limit check for key validation with process-local aggregate safeguard
    pub fn check_validate_key(&self, client_key: &str) -> Result<(), RateLimitError> {
        if self.aggregate_keys_limiter.check().is_err() {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, "1")],
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Process-level aggregate API key operation rate limit exceeded. Please back off and retry shortly."
                })),
            ));
        }

        let bounded_key = self.bound_and_sanitize_key(&self.client_val_limiter, client_key);

        if self.client_val_limiter.check_key(&bounded_key).is_err() {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, "1")],
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "API key validation rate limit exceeded for this client. Please back off and retry shortly."
                })),
            ));
        }

        Ok(())
    }

    pub fn check_demo(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.demo_limiter.check().is_err() {
            Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "code": 429,
                    "message": "Scenario simulation rate limit exceeded. Please allow current workload to stabilize."
                })),
            ))
        } else {
            Ok(())
        }
    }
}

impl Default for ApiRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cidr_parsing_and_matching() {
        let v4_cidr = IpCidr::parse("192.168.1.0/24").unwrap();
        assert!(v4_cidr.contains("192.168.1.5".parse().unwrap()));
        assert!(!v4_cidr.contains("192.168.2.1".parse().unwrap()));

        let single_ip = IpCidr::parse("10.0.0.1").unwrap();
        assert!(single_ip.contains("10.0.0.1".parse().unwrap()));
        assert!(!single_ip.contains("10.0.0.2".parse().unwrap()));

        let v6_cidr = IpCidr::parse("fd00::/8").unwrap();
        assert!(v6_cidr.contains("fd00::1".parse().unwrap()));
        assert!(!v6_cidr.contains("fe80::1".parse().unwrap()));
    }

    #[test]
    fn test_trusted_proxy_forwarded_traversal() {
        let proxy_config = TrustedProxyConfig::parse("10.0.0.0/8,172.16.0.0/12");

        // Case 1: Direct untrusted peer ignores XFF completely
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "1.2.3.4".parse().unwrap());
        let client = resolve_client_ip(
            Some("198.51.100.1".parse().unwrap()),
            &headers,
            &proxy_config,
        );
        assert_eq!(client, "198.51.100.1".parse::<IpAddr>().unwrap());

        // Case 2: Trusted proxy single hop
        let client = resolve_client_ip(Some("10.0.0.1".parse().unwrap()), &headers, &proxy_config);
        assert_eq!(client, "1.2.3.4".parse::<IpAddr>().unwrap());

        // Case 3: Trusted proxy multi-hop right-to-left traversal stopping at untrusted client
        headers.insert(
            "x-forwarded-for",
            "forged.ip, 203.0.113.50, 172.16.0.5".parse().unwrap(),
        );
        let client = resolve_client_ip(Some("10.0.0.1".parse().unwrap()), &headers, &proxy_config);
        assert_eq!(client, "203.0.113.50".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_unauthenticated_fake_auth_tokens_do_not_bypass_peer_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer fake_token_abc".parse().unwrap());
        let id1 = extract_client_source_identifier(
            Some("198.51.100.25:4567".parse().unwrap()),
            &headers,
            None,
        );

        headers.insert("authorization", "Bearer fake_token_xyz".parse().unwrap());
        let id2 = extract_client_source_identifier(
            Some("198.51.100.25:4567".parse().unwrap()),
            &headers,
            None,
        );

        assert_eq!(id1, "ip:198.51.100.25");
        assert_eq!(id2, "ip:198.51.100.25");
        assert_eq!(id1, id2, "Fake tokens must not alter the peer IP identity");
    }

    #[test]
    fn test_authenticated_caller_identity_precedence() {
        let headers = HeaderMap::new();
        let id = extract_client_source_identifier(
            Some("198.51.100.25:4567".parse().unwrap()),
            &headers,
            Some("merchant_acme_corp"),
        );
        assert_eq!(id, "auth_merchant:merchant_acme_corp");
    }

    #[test]
    fn test_independent_peer_buckets_and_exhaustion() {
        let limiter = ApiRateLimiter::new_with_all_quotas(10, 10, 100, 100, 2, 2, 10, 10, 10, 10);
        let peer_a = "ip:198.51.100.1";
        let peer_b = "ip:198.51.100.2";

        // Peer A exhausts its bucket
        assert!(limiter.check_generate_sandbox(peer_a).is_ok());
        assert!(limiter.check_generate_sandbox(peer_a).is_ok());
        assert!(limiter.check_generate_sandbox(peer_a).is_err());

        // Peer B is independent and unaffected
        assert!(limiter.check_generate_sandbox(peer_b).is_ok());
        assert!(limiter.check_generate_sandbox(peer_b).is_ok());
        assert!(limiter.check_generate_sandbox(peer_b).is_err());
    }
}
