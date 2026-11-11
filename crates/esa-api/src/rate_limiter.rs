use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use governor::clock::DefaultClock;
use governor::nanos::Nanos;
use governor::state::keyed::{DefaultKeyedStateStore, ShrinkableKeyedStateStore};
use governor::state::{InMemoryState, NotKeyed, StateStore};
use governor::{Quota, RateLimiter as GovRateLimiter};
use parking_lot::Mutex;
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
pub const MAX_KEYED_ENTRIES: usize = 10_000;

/// Bounded keyed state store backed by Governor's `DefaultKeyedStateStore`.
///
/// Guarantees that the number of active distinct rate-limiter keys can never exceed `capacity`.
/// Under high concurrency, key admission is synchronized such that once capacity is reached,
/// all new distinct keys are atomically routed to a shared `overflow_key` bucket.
/// Established keys bypass the lock and execute concurrently.
pub struct BoundedKeyedStateStore<K = String> {
    inner: DefaultKeyedStateStore<K>,
    capacity: usize,
    overflow_key: K,
    admission_lock: Mutex<()>,
}

impl<K: std::hash::Hash + Eq + Clone + Send + Sync + 'static> BoundedKeyedStateStore<K> {
    pub fn new(capacity: usize, overflow_key: K) -> Self {
        Self {
            inner: DefaultKeyedStateStore::default(),
            capacity: capacity.max(1),
            overflow_key,
            admission_lock: Mutex::new(()),
        }
    }

    #[allow(dead_code)]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<K: std::hash::Hash + Eq + Clone + Send + Sync + 'static> StateStore
    for BoundedKeyedStateStore<K>
{
    type Key = K;

    fn measure_and_replace<T, F, E>(&self, key: &Self::Key, f: F) -> Result<T, E>
    where
        F: Fn(Option<Nanos>) -> Result<(T, Nanos), E>,
    {
        // Fast path for established keys: lock-free concurrent measurement
        if self.inner.contains_key(key) {
            return self.inner.measure_and_replace(key, f);
        }

        // New key path: synchronize admission to strictly bound capacity under concurrency
        let _guard = self.admission_lock.lock();

        // Double-check: was the key admitted by another thread while waiting for the lock?
        if self.inner.contains_key(key) {
            return self.inner.measure_and_replace(key, f);
        }

        // Enforce capacity bound: if at capacity, route to overflow_key
        let has_overflow = self.inner.contains_key(&self.overflow_key);
        let effective_limit = self
            .capacity
            .saturating_sub(if has_overflow { 0 } else { 1 });

        let target_key = if self.inner.len() >= effective_limit {
            &self.overflow_key
        } else {
            key
        };

        self.inner.measure_and_replace(target_key, f)
    }
}

impl<K: std::hash::Hash + Eq + Clone + Send + Sync + 'static> ShrinkableKeyedStateStore<K>
    for BoundedKeyedStateStore<K>
{
    fn retain_recent(&self, drop_below: Nanos) {
        let _guard = self.admission_lock.lock();
        self.inner.retain_recent(drop_below);
    }

    fn shrink_to_fit(&self) {
        let _guard = self.admission_lock.lock();
        self.inner.shrink_to_fit();
    }

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

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
///
/// Differentiates between generic reverse proxies (which must never be trusted to forward arbitrary
/// provider-specific headers like `CF-Connecting-IP`) and specifically configured Cloudflare edge proxies.
#[derive(Debug, Clone)]
pub struct TrustedProxyConfig {
    generic_proxies: Vec<IpCidr>,
    cloudflare_proxies: Vec<IpCidr>,
}

impl TrustedProxyConfig {
    pub fn from_env() -> Self {
        let generic_val = std::env::var("TRUSTED_PROXIES").unwrap_or_default();
        let cloudflare_val = std::env::var("CLOUDFLARE_PROXIES").unwrap_or_default();
        Self::parse(&generic_val, &cloudflare_val)
    }

    pub fn parse(generic_str: &str, cloudflare_str: &str) -> Self {
        let generic_str = generic_str.trim();

        // The boolean "true" or "1" shorthand is explicitly rejected to avoid unintentionally
        // trusting broad internal networks, containers, or application workloads.
        // Reverse-proxy trust MUST be established with explicit proxy CIDRs/IPs.
        let generic_proxies = if generic_str.is_empty()
            || generic_str.eq_ignore_ascii_case("false")
            || generic_str == "0"
            || generic_str.eq_ignore_ascii_case("true")
            || generic_str == "1"
        {
            if generic_str.eq_ignore_ascii_case("true") || generic_str == "1" {
                tracing::warn!(
                    "TRUSTED_PROXIES=true shorthand is rejected. Please configure explicit proxy CIDRs (e.g. TRUSTED_PROXIES=10.0.0.1/32) to identify actual reverse-proxy infrastructure."
                );
            }
            Vec::new()
        } else {
            generic_str.split(',').filter_map(IpCidr::parse).collect()
        };

        let cloudflare_proxies = cloudflare_str
            .split(',')
            .filter_map(IpCidr::parse)
            .collect();

        Self {
            generic_proxies,
            cloudflare_proxies,
        }
    }

    pub fn is_trusted_generic(&self, ip: IpAddr) -> bool {
        self.generic_proxies.iter().any(|cidr| cidr.contains(ip))
    }

    pub fn is_trusted_cloudflare(&self, ip: IpAddr) -> bool {
        self.cloudflare_proxies.iter().any(|cidr| cidr.contains(ip))
    }

    pub fn is_trusted_any(&self, ip: IpAddr) -> bool {
        self.is_trusted_generic(ip) || self.is_trusted_cloudflare(ip)
    }
}

/// Resolves the authentic client IP address using peer address and trusted reverse proxy hops.
///
/// Follows strict reverse-proxy security rules:
/// 1. The direct socket peer IP is used unless it matches a specifically configured trusted proxy CIDR.
/// 2. CF-Connecting-IP is ONLY evaluated if the direct peer is a specifically configured Cloudflare proxy
///    (`CLOUDFLARE_PROXIES`), where the upstream edge is guaranteed to strip and overwrite client-supplied headers.
///    Generic reverse proxies NEVER trust CF-Connecting-IP, preventing client-supplied headers from bypassing XFF checks.
/// 3. In X-Forwarded-For, hops are traversed right-to-left (most recent proxy to earliest).
///    The traversal stops at the first untrusted IP, preventing forged X-Forwarded-For spoofing.
/// 4. Arbitrary unauthenticated headers or malformed IPs fail closed and are safely ignored.
pub fn resolve_client_ip(
    peer_ip: Option<IpAddr>,
    headers: &HeaderMap,
    proxy_config: &TrustedProxyConfig,
) -> IpAddr {
    let peer = peer_ip.unwrap_or_else(|| IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));

    // Direct peer is untrusted: ignore all forwarding headers
    if !proxy_config.is_trusted_any(peer) {
        return peer;
    }

    // Direct peer is a specifically verified Cloudflare edge proxy: CF-Connecting-IP is trustworthy
    if proxy_config.is_trusted_cloudflare(peer) {
        if let Some(cf_ip_str) = headers
            .get("cf-connecting-ip")
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(cf_ip) = cf_ip_str.trim().parse::<IpAddr>() {
                return cf_ip;
            }
        }
    }

    // Direct peer is a trusted generic or Cloudflare proxy: parse X-Forwarded-For
    // using trusted-hop right-to-left traversal
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        let hops: Vec<IpAddr> = xff
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<IpAddr>().ok())
            .collect();

        if !hops.is_empty() {
            for hop in hops.iter().rev() {
                if !proxy_config.is_trusted_any(*hop) {
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
    client_gen_limiter: Arc<GovRateLimiter<String, BoundedKeyedStateStore<String>, DefaultClock>>,
    client_val_limiter: Arc<GovRateLimiter<String, BoundedKeyedStateStore<String>, DefaultClock>>,
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
        Self::new_with_bounded_capacity(
            checkout_rps,
            checkout_burst,
            aggregate_keys_rps,
            aggregate_keys_burst,
            client_gen_rps,
            client_gen_burst,
            client_val_rps,
            client_val_burst,
            demo_rps,
            demo_burst,
            MAX_KEYED_ENTRIES,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_bounded_capacity(
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
        keyed_capacity: usize,
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

        let clock = DefaultClock::default();
        let client_gen_limiter = Arc::new(GovRateLimiter::new(
            client_gen_quota,
            BoundedKeyedStateStore::new(keyed_capacity, "overflow_client".to_string()),
            &clock,
        ));
        let client_val_limiter = Arc::new(GovRateLimiter::new(
            client_val_quota,
            BoundedKeyedStateStore::new(keyed_capacity, "overflow_client".to_string()),
            &clock,
        ));

        Self {
            checkout_limiter: Arc::new(GovRateLimiter::direct(checkout_quota)),
            aggregate_keys_limiter: Arc::new(GovRateLimiter::direct(aggregate_keys_quota)),
            client_gen_limiter,
            client_val_limiter,
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

        // Periodic pruning of expired/replenished entries
        #[allow(clippy::manual_is_multiple_of)]
        if self.call_count.fetch_add(1, Ordering::Relaxed) % 256 == 0 {
            self.client_gen_limiter.retain_recent();
        }

        if self
            .client_gen_limiter
            .check_key(&client_key.to_string())
            .is_err()
        {
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

        // Periodic pruning of expired/replenished entries
        #[allow(clippy::manual_is_multiple_of)]
        if self.call_count.fetch_add(1, Ordering::Relaxed) % 256 == 0 {
            self.client_val_limiter.retain_recent();
        }

        if self
            .client_val_limiter
            .check_key(&client_key.to_string())
            .is_err()
        {
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

    #[allow(dead_code)]
    pub fn gen_len(&self) -> usize {
        self.client_gen_limiter.len()
    }

    #[allow(dead_code)]
    pub fn val_len(&self) -> usize {
        self.client_val_limiter.len()
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
        let proxy_config = TrustedProxyConfig::parse("10.0.0.0/8,172.16.0.0/12", "");

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
    fn test_generic_trusted_proxy_ignores_cf_connecting_ip() {
        // A generic reverse proxy in TRUSTED_PROXIES must NOT trust CF-Connecting-IP
        let proxy_config = TrustedProxyConfig::parse("10.0.0.0/8", "");

        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", "1.1.1.1".parse().unwrap());
        headers.insert("x-forwarded-for", "203.0.113.77".parse().unwrap());

        let client = resolve_client_ip(Some("10.0.0.1".parse().unwrap()), &headers, &proxy_config);
        // Must resolve from verified XFF, NOT the spoofed CF-Connecting-IP
        assert_eq!(
            client,
            "203.0.113.77".parse::<IpAddr>().unwrap(),
            "Generic proxy must ignore CF-Connecting-IP and use verified XFF"
        );
    }

    #[test]
    fn test_cloudflare_proxy_trusts_cf_connecting_ip() {
        // Specifically configured Cloudflare proxy accepts CF-Connecting-IP
        let proxy_config = TrustedProxyConfig::parse("", "173.245.48.0/20");

        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", "203.0.113.88".parse().unwrap());
        headers.insert("x-forwarded-for", "1.2.3.4".parse().unwrap());

        let client = resolve_client_ip(
            Some("173.245.48.5".parse().unwrap()),
            &headers,
            &proxy_config,
        );
        assert_eq!(
            client,
            "203.0.113.88".parse::<IpAddr>().unwrap(),
            "Specifically configured Cloudflare proxy must accept CF-Connecting-IP"
        );
    }

    #[test]
    fn test_boolean_trusted_proxies_shorthand_rejected() {
        let proxy_config = TrustedProxyConfig::parse("true", "");
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "1.1.1.1".parse().unwrap());

        // Because "true" shorthand was rejected, 127.0.0.1 is not trusted and XFF is ignored
        let client = resolve_client_ip(Some("127.0.0.1".parse().unwrap()), &headers, &proxy_config);
        assert_eq!(
            client,
            "127.0.0.1".parse::<IpAddr>().unwrap(),
            "TRUSTED_PROXIES=true shorthand must fail closed"
        );
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
    fn test_bounded_keyed_state_store_strict_capacity_under_concurrency() {
        let capacity = 5;
        let limiter = ApiRateLimiter::new_with_bounded_capacity(
            10, 10, 100, 100, 10, 10, 10, 10, 10, 10, capacity,
        );

        // Concurrently attempt to insert 50 distinct keys
        let handles: Vec<_> = (0..50)
            .map(|i| {
                let lim = limiter.clone();
                std::thread::spawn(move || {
                    let key = format!("ip:198.51.100.{}", i);
                    let _ = lim.check_generate_sandbox(&key);
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // Strict guarantee: len CANNOT exceed capacity!
        assert!(
            limiter.gen_len() <= capacity,
            "Limiter entries {} exceeded strict capacity {}",
            limiter.gen_len(),
            capacity
        );
    }
}
