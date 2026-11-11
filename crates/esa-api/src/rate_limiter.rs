use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use governor::clock::DefaultClock;
use governor::nanos::Nanos;
use governor::state::keyed::{DefaultKeyedStateStore, ShrinkableKeyedStateStore};
use governor::state::{InMemoryState, NotKeyed, StateStore};
use governor::{Quota, RateLimiter as GovRateLimiter};
use parking_lot::{Mutex, RwLock};
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

#[cfg(test)]
pub type ObserveHook<K> = Option<Arc<dyn Fn(&K) + Send + Sync>>;

/// Maximum number of active tracked keys in keyed rate limiters to prevent unbounded memory growth
pub const MAX_KEYED_ENTRIES: usize = 10_000;

/// Bounded keyed state store backed by Governor's `DefaultKeyedStateStore`.
///
/// Guarantees that the number of active distinct rate-limiter keys can never exceed `capacity`.
///
/// Synchronization design:
/// 1. `prune_gate: RwLock<()>`:
///    - `measure_and_replace()` acquires a read guard across the key existence check and inner measurement.
///    - `retain_recent()` and `shrink_to_fit()` acquire a write guard.
///    - This guarantees that pruning can never delete an established key while a measurement operation is in progress,
///      eliminating the race where a pruned key is recreated after its capacity slot was reallocated.
/// 2. `admission_lock: Mutex<()>`:
///    - Synchronizes new-key admission so that concurrent requests attempting to insert distinct keys
///      cannot exceed `capacity`. When at capacity, requests are atomically redirected to `overflow_key`.
/// 3. Established keys measure concurrently using DashMap's fine-grained shard locking under the read guard.
#[derive(Clone)]
pub struct BoundedKeyedStateStore<K = String> {
    inner: Arc<DefaultKeyedStateStore<K>>,
    capacity: usize,
    overflow_key: K,
    admission_lock: Arc<Mutex<()>>,
    prune_gate: Arc<RwLock<()>>,
    #[cfg(test)]
    pub(crate) test_observe_hook: ObserveHook<K>,
}

impl<K: std::hash::Hash + Eq + Clone + Send + Sync + 'static> BoundedKeyedStateStore<K> {
    pub fn new(capacity: usize, overflow_key: K) -> Self {
        Self {
            inner: Arc::new(DefaultKeyedStateStore::default()),
            capacity: capacity.max(1),
            overflow_key,
            admission_lock: Arc::new(Mutex::new(())),
            prune_gate: Arc::new(RwLock::new(())),
            #[cfg(test)]
            test_observe_hook: None,
        }
    }

    #[allow(dead_code)]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn prune_gate(&self) -> &RwLock<()> {
        &self.prune_gate
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn force_remove(&self, key: &K) {
        let _write_guard = self.prune_gate.write();
        let _guard = self.admission_lock.lock();
        self.inner.remove(key);
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
        // 1. Hold read guard across existence check and inner measurement.
        // Pruning (retain_recent / shrink_to_fit) requires the write guard,
        // making measurement and pruning mutually exclusive and preventing key recreation races.
        let _read_guard = self.prune_gate.read();

        // Fast path for established keys: lock-free concurrent measurement across DashMap shards
        if self.inner.contains_key(key) {
            #[cfg(test)]
            if let Some(hook) = &self.test_observe_hook {
                hook(key);
            }
            return self.inner.measure_and_replace(key, f);
        }

        // New key path: synchronize admission to strictly bound capacity under concurrency
        let _admission = self.admission_lock.lock();

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
        let _write_guard = self.prune_gate.write();
        let _guard = self.admission_lock.lock();
        self.inner.retain_recent(drop_below);
    }

    fn shrink_to_fit(&self) {
        let _write_guard = self.prune_gate.write();
        let _guard = self.admission_lock.lock();
        self.inner.shrink_to_fit();
    }

    fn len(&self) -> usize {
        let _read_guard = self.prune_gate.read();
        self.inner.len()
    }

    fn is_empty(&self) -> bool {
        let _read_guard = self.prune_gate.read();
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

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::OnceLock;

type HmacSha256 = Hmac<Sha256>;

/// Minimum key length in bytes required for configured HMAC log correlation secrets.
pub const MIN_LOG_HMAC_SECRET_BYTES: usize = 32;

/// Validates the configured `ESA_LOG_HMAC_SECRET` environment variable.
///
/// Rules:
/// - If omitted or empty: valid (falls back to process-lifetime random secret with 244 bits of entropy).
/// - If configured and non-empty: must be at least 32 bytes in length. Shorter values are strictly rejected.
/// - Never logs the secret or any part of it.
/// - Length validation cannot establish entropy or randomness; operators must configure a cryptographically
///   secure random secret (e.g. `openssl rand -hex 32`).
pub fn validate_log_hmac_secret_configuration() -> Result<(), String> {
    if let Ok(val) = std::env::var("ESA_LOG_HMAC_SECRET") {
        let trimmed = val.trim();
        if !trimmed.is_empty() && trimmed.len() < MIN_LOG_HMAC_SECRET_BYTES {
            return Err(format!(
                "Configured ESA_LOG_HMAC_SECRET is too short ({} bytes). Keyed HMAC logging requires at least {} bytes for cryptographic security. Note: length validation cannot establish entropy; operators must configure a cryptographically random value.",
                trimmed.len(),
                MIN_LOG_HMAC_SECRET_BYTES
            ));
        }
    }
    Ok(())
}

static PROCESS_FALLBACK_SECRET: OnceLock<[u8; 32]> = OnceLock::new();

/// Retrieves or generates a process-lifetime fallback secret for HMAC-SHA256 logging.
///
/// Fallback Entropy & Randomness Design:
/// - Generated on first use per process lifetime via two independent OS CSPRNG UUIDv4 values.
/// - RFC 4122 reserves 6 bits per UUID for version and variant (122 random bits per UUIDv4).
/// - Two independent UUIDv4s provide exactly 244 bits of cryptographic CSPRNG entropy from the
///   underlying operating system's cryptographic random number source across the 32-byte (256-bit) buffer.
/// - 244 bits of cryptographic entropy well exceeds the industry standard 128-bit security margin for HMAC keys,
///   preventing offline dictionary and rainbow-table precomputation across the lifetime of the process.
fn get_fallback_secret() -> &'static [u8; 32] {
    PROCESS_FALLBACK_SECRET.get_or_init(|| {
        let mut secret = [0u8; 32];
        let (u1, u2) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        secret[..16].copy_from_slice(u1.as_bytes());
        secret[16..].copy_from_slice(u2.as_bytes());
        secret
    })
}

/// Generates a privacy-preserving pseudonymous identifier for structured logging without exposing
/// raw client IPs, API tokens, or merchant IDs.
///
/// Security & Privacy Design:
/// - Uses keyed HMAC-SHA256 to prevent dictionary and rainbow-table precomputation attacks against small
///   input spaces (e.g. IPv4 addresses).
/// - Truncated to 16 hex characters (64 bits of HMAC output) to provide high collision resistance while
///   maintaining readable log tokens.
/// - If `ESA_LOG_HMAC_SECRET` is configured in the environment, it is used as the HMAC key to enable stable,
///   pseudonymous log correlation across multiple cluster nodes and service restarts.
/// - If configured, `ESA_LOG_HMAC_SECRET` must be at least 32 bytes (validated via `validate_log_hmac_secret_configuration`).
/// - If `ESA_LOG_HMAC_SECRET` is omitted or empty, falls back to a 244-bit entropy process-lifetime random secret,
///   providing pseudonymous correlation within a single process run while preventing cross-restart tracking.
/// - Note: Length validation cannot establish entropy; operators must configure a cryptographically random value.
/// - Note: This produces pseudonymous identifiers for log correlation, not full anonymization.
pub fn privacy_preserving_client_id(raw: &str) -> String {
    let env_val = std::env::var("ESA_LOG_HMAC_SECRET").ok();
    let key: &[u8] = match env_val.as_deref().map(str::trim) {
        Some(trimmed) if !trimmed.is_empty() => {
            if trimmed.len() < MIN_LOG_HMAC_SECRET_BYTES {
                panic!(
                    "Configured ESA_LOG_HMAC_SECRET is too short ({} bytes). Keyed HMAC logging requires at least {} bytes.",
                    trimmed.len(),
                    MIN_LOG_HMAC_SECRET_BYTES
                );
            }
            trimmed.as_bytes()
        }
        _ => get_fallback_secret(),
    };

    let hash = if let Ok(mut mac) = HmacSha256::new_from_slice(key) {
        mac.update(raw.as_bytes());
        format!("{:x}", mac.finalize().into_bytes())
    } else {
        use sha2::Digest;
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.update(raw.as_bytes());
        format!("{:x}", hasher.finalize())
    };

    let prefix = if raw.starts_with("auth_") {
        "auth_user:"
    } else if raw.starts_with("ip:") {
        "ip_hash:"
    } else {
        "client_hash:"
    };
    format!("{}{}", prefix, &hash[..16])
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
            let mut list = Vec::new();
            for token in generic_str.split(',') {
                let clean = token.trim();
                if clean.is_empty() {
                    continue;
                }
                match IpCidr::parse(clean) {
                    Some(cidr) => list.push(cidr),
                    None => {
                        tracing::warn!(
                            token = %clean,
                            "Failed to parse TRUSTED_PROXIES entry as valid IPv4 or IPv6 CIDR; entry ignored"
                        );
                    }
                }
            }
            list
        };

        let mut cloudflare_proxies = Vec::new();
        for token in cloudflare_str.split(',') {
            let clean = token.trim();
            if clean.is_empty() {
                continue;
            }
            match IpCidr::parse(clean) {
                Some(cidr) => cloudflare_proxies.push(cidr),
                None => {
                    tracing::warn!(
                        token = %clean,
                        "Failed to parse CLOUDFLARE_PROXIES entry as valid IPv4 or IPv6 CIDR; entry ignored"
                    );
                }
            }
        }

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
    #[cfg(test)]
    client_gen_store: BoundedKeyedStateStore<String>,
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
        #[cfg(test)]
        {
            Self::new_with_bounded_capacity_and_hook(
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
                keyed_capacity,
                None,
            )
        }
        #[cfg(not(test))]
        {
            let checkout_quota = Quota::per_second(NonZeroU32::new(checkout_rps).unwrap())
                .allow_burst(NonZeroU32::new(checkout_burst).unwrap());
            let aggregate_keys_quota =
                Quota::per_second(NonZeroU32::new(aggregate_keys_rps).unwrap())
                    .allow_burst(NonZeroU32::new(aggregate_keys_burst).unwrap());
            let client_gen_quota = Quota::per_second(NonZeroU32::new(client_gen_rps).unwrap())
                .allow_burst(NonZeroU32::new(client_gen_burst).unwrap());
            let client_val_quota = Quota::per_second(NonZeroU32::new(client_val_rps).unwrap())
                .allow_burst(NonZeroU32::new(client_val_burst).unwrap());
            let demo_quota = Quota::per_second(NonZeroU32::new(demo_rps).unwrap())
                .allow_burst(NonZeroU32::new(demo_burst).unwrap());

            let clock = DefaultClock::default();
            let gen_store =
                BoundedKeyedStateStore::new(keyed_capacity, "overflow_client".to_string());
            let client_gen_limiter =
                Arc::new(GovRateLimiter::new(client_gen_quota, gen_store, &clock));
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
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_bounded_capacity_and_hook(
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
        hook: ObserveHook<String>,
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
        let mut gen_store =
            BoundedKeyedStateStore::new(keyed_capacity, "overflow_client".to_string());
        gen_store.test_observe_hook = hook;
        let client_gen_limiter = Arc::new(GovRateLimiter::new(
            client_gen_quota,
            gen_store.clone(),
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
            client_gen_store: gen_store,
            demo_limiter: Arc::new(GovRateLimiter::direct(demo_quota)),
            call_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn check_checkout(&self) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        if self.checkout_limiter.check().is_err() {
            tracing::warn!(limiter = "checkout", "Payment checkout rate limit exceeded");
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
            tracing::warn!(
                limiter = "aggregate_keys",
                "Process-level API key operation rate limit exceeded"
            );
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
            tracing::warn!(
                limiter = "generate_sandbox_aggregate",
                "Process-level aggregate API key generation rate limit exceeded"
            );
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
            let client_ref = privacy_preserving_client_id(client_key);
            tracing::warn!(
                client_ref = %client_ref,
                limiter = "generate_sandbox_per_client",
                "Client sandbox key generation rate limit exceeded"
            );
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
            tracing::warn!(
                limiter = "validate_key_aggregate",
                "Process-level aggregate API key validation rate limit exceeded"
            );
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
            let client_ref = privacy_preserving_client_id(client_key);
            tracing::warn!(
                client_ref = %client_ref,
                limiter = "validate_key_per_client",
                "Client API key validation rate limit exceeded"
            );
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
            tracing::warn!(limiter = "demo", "Scenario simulation rate limit exceeded");
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

    #[allow(dead_code)]
    pub fn retain_recent(&self) {
        self.client_gen_limiter.retain_recent();
        self.client_val_limiter.retain_recent();
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn gen_store(&self) -> &BoundedKeyedStateStore<String> {
        &self.client_gen_store
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
    use std::sync::atomic::AtomicBool;

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

    #[test]
    fn test_prune_update_race_eliminated_deterministically() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Barrier;

        let capacity = 3;
        let barrier_observed = Arc::new(Barrier::new(2));
        let barrier_resume = Arc::new(Barrier::new(2));
        let observed_flag = Arc::new(AtomicBool::new(false));

        let b1 = barrier_observed.clone();
        let b2 = barrier_resume.clone();
        let flag = observed_flag.clone();

        let hook: Arc<dyn Fn(&String) + Send + Sync> = Arc::new(move |k: &String| {
            if k == "ip:198.51.100.1" {
                flag.store(true, Ordering::SeqCst);
                b1.wait();
                b2.wait();
            }
        });

        let limiter = ApiRateLimiter::new_with_bounded_capacity_and_hook(
            10,
            10,
            100,
            100,
            10,
            10,
            10,
            10,
            10,
            10,
            capacity,
            Some(hook),
        );

        // 1. Fill store to capacity:
        // Key 1: "ip:198.51.100.1"
        // Key 2: "ip:198.51.100.2"
        // Overflow Key: "overflow_client" (pre-allocated on first overflow)
        assert!(limiter.check_generate_sandbox("ip:198.51.100.1").is_ok());
        assert!(limiter.check_generate_sandbox("ip:198.51.100.2").is_ok());
        assert!(limiter.check_generate_sandbox("ip:198.51.100.99").is_ok()); // Triggers overflow bucket
        assert_eq!(limiter.gen_len(), capacity);

        // 2. Thread 1: Established key request for "ip:198.51.100.1"
        // It observes that key exists, triggers hook, and pauses before measurement
        let lim_clone = limiter.clone();
        let handle = std::thread::spawn(move || {
            let res = lim_clone.check_generate_sandbox("ip:198.51.100.1");
            assert!(res.is_ok());
        });

        // Wait until Thread 1 has observed the established key and paused while holding prune_gate.read()
        barrier_observed.wait();
        assert!(observed_flag.load(Ordering::SeqCst));

        // 3. Thread 2 (here): Attempt to prune or write while Thread 1 is in-flight.
        // Because Thread 1 holds prune_gate.read(), write guard CANNOT be acquired!
        // This deterministically proves that retain_recent() / pruning is mutually exclusive
        // and cannot delete the key from under Thread 1!
        let write_lock_blocked = limiter.gen_store().prune_gate().try_write().is_none();
        assert!(
            write_lock_blocked,
            "Prune gate write lock MUST be blocked while measurement holds read guard"
        );

        // 4. Release Thread 1 to complete its measurement
        barrier_resume.wait();
        handle.join().unwrap();

        // 5. Thread 1 has released the read lock. Now pruning can safely remove a stale key
        limiter
            .gen_store()
            .force_remove(&"ip:198.51.100.1".to_string());
        assert_eq!(limiter.gen_len(), capacity - 1);

        // 6. Admit a different new key into the freed capacity
        assert!(limiter.check_generate_sandbox("ip:198.51.100.3").is_ok());
        assert_eq!(limiter.gen_len(), capacity);

        // 7. Request "ip:198.51.100.1" again (which was removed in step 5)
        // Since capacity is full, it routes safely to overflow_client without exceeding capacity
        assert!(limiter.check_generate_sandbox("ip:198.51.100.1").is_ok());

        // 8. Strict capacity assertion: len must remain exactly capacity!
        assert_eq!(
            limiter.gen_len(),
            capacity,
            "Store must strictly remain at capacity bound"
        );
    }

    #[test]
    fn test_stress_simultaneous_admission_and_pruning() {
        let capacity = 10;
        let limiter = ApiRateLimiter::new_with_bounded_capacity(
            10, 10, 1000, 1000, 100, 100, 100, 100, 10, 10, capacity,
        );

        let running = Arc::new(AtomicBool::new(true));

        // 10 threads continuously performing new key admissions and established key hits
        let mut workers = Vec::new();
        for worker_id in 0..10 {
            let lim = limiter.clone();
            let r = running.clone();
            workers.push(std::thread::spawn(move || {
                let mut i = 0;
                while r.load(Ordering::Relaxed) {
                    i += 1;
                    let key = if i % 2 == 0 {
                        // Established key
                        format!("ip:10.0.0.{}", worker_id)
                    } else {
                        // New key attempt
                        format!("ip:10.{}.{}", worker_id, i)
                    };
                    let _ = lim.check_generate_sandbox(&key);
                }
            }));
        }

        // 4 threads continuously pruning
        let mut pruners = Vec::new();
        for _ in 0..4 {
            let lim = limiter.clone();
            let r = running.clone();
            pruners.push(std::thread::spawn(move || {
                while r.load(Ordering::Relaxed) {
                    lim.retain_recent();
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }));
        }

        // Run stress test for 100ms
        std::thread::sleep(std::time::Duration::from_millis(100));
        running.store(false, Ordering::Relaxed);

        for w in workers {
            w.join().unwrap();
        }
        for p in pruners {
            p.join().unwrap();
        }

        // Assert store len NEVER exceeded capacity
        assert!(
            limiter.gen_len() <= capacity,
            "Stress test failed: gen_len {} exceeded capacity {}",
            limiter.gen_len(),
            capacity
        );
    }
}
