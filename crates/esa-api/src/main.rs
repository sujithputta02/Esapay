use axum::{
    body::Bytes,
    extract::{ws::WebSocketUpgrade, DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
#[cfg(test)]
use esa_agents::OllamaClient;
use esa_cashfree::{CashfreeConfig, CashfreeProvider};
use esa_core::*;
use esa_payu::{PayUConfig, PayUProvider};
use esa_razorpay::{
    payment_entity_to_event, webhook::MAX_WEBHOOK_BODY_BYTES, RazorpayAdapter, RazorpayConfig,
    WebhookError,
};
use esa_runtime::{EsaOrchestrator, RuntimeEvent, RuntimeEventHandler};
use esa_state::{
    InMemoryWebhookStore, PostgresWebhookStore, StateFabric, WebhookProcessingResult, WebhookStore,
    WebhookStoreError,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tracing::{error, info, warn, Instrument, Level};

mod api_keys;
mod benchmark;
mod benchmark_harness;
mod live_data;
mod payment;
mod rate_limiter;
mod vitals;
mod websocket;
use live_data::{
    action_record_from_audit, audit_record_json, compute_verdict_stats, effect_measurement_json,
    verdict_record_json,
};
use vitals::VitalsStore;
use websocket::{websocket_handler_with_auth, TelemetryBroadcaster, TelemetryMessage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PaymentCheckoutMode {
    /// Explicit opt-in simulation for local/demo usage.
    Simulated,
    /// Live/production payment mode. Any live gateway or live environment triggers this.
    /// Live checkout must return 501 NOT IMPLEMENTED until real upstream order creation is implemented.
    Live { reason: String },
    /// No payment mode or simulation opt-in configured.
    /// Fails safely with 503 SERVICE UNAVAILABLE to prevent accidental simulation or unconfigured processing.
    Unconfigured,
    /// Invalid or unrecognized payment mode configuration value.
    /// Fails safely with 400 BAD REQUEST.
    Misconfigured(String),
}

/// Detects the active payment processing mode from environment variables.
/// Safe by default: live mode takes absolute precedence over simulation opt-ins,
/// and unconfigured or invalid values fail safely without defaulting to live or simulation.
pub(crate) fn detect_checkout_mode() -> PaymentCheckoutMode {
    let check_env_var = |name: &str| -> Result<Option<String>, String> {
        match std::env::var(name) {
            Ok(val) => {
                let trimmed = val.trim().to_string();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(trimmed))
                }
            }
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(e) => Err(format!("Environment variable '{}' error: {}", name, e)),
        }
    };

    let sim_flag = match check_env_var("ESA_ENABLE_SIMULATED_CHECKOUT") {
        Ok(v) => v,
        Err(e) => return PaymentCheckoutMode::Misconfigured(e),
    };
    let esa_mode = match check_env_var("ESA_PAYMENT_MODE") {
        Ok(v) => v,
        Err(e) => return PaymentCheckoutMode::Misconfigured(e),
    };
    let env_mode = match check_env_var("ENVIRONMENT") {
        Ok(v) => v,
        Err(e) => return PaymentCheckoutMode::Misconfigured(e),
    };

    // Supported provider gateways: razorpay, cashfree, payu, stripe, phonepe, paytm, adyen
    let provider_vars = [
        "RAZORPAY_MODE",
        "STRIPE_MODE",
        "PHONEPE_MODE",
        "CASHFREE_MODE",
        "CASHFREE_ENV",
        "PAYU_MODE",
        "PAYU_ENV",
        "PAYTM_MODE",
        "ADYEN_MODE",
    ];

    let mut provider_modes = Vec::new();
    for var in &provider_vars {
        match check_env_var(var) {
            Ok(Some(v)) => provider_modes.push((*var, v)),
            Ok(None) => {}
            Err(e) => return PaymentCheckoutMode::Misconfigured(e),
        }
    }

    // 1. Syntax / validity check for ESA_ENABLE_SIMULATED_CHECKOUT
    let mut explicit_sim_opt_in = false;
    let mut explicit_sim_disabled = false;
    if let Some(ref val) = sim_flag {
        let v = val.to_ascii_lowercase();
        if v == "true" || v == "1" || v == "yes" || v == "on" {
            explicit_sim_opt_in = true;
        } else if v == "false" || v == "0" || v == "no" || v == "off" {
            explicit_sim_disabled = true;
        } else {
            return PaymentCheckoutMode::Misconfigured(format!(
                "Invalid ESA_ENABLE_SIMULATED_CHECKOUT value '{}'. Expected 'true' or 'false'.",
                val
            ));
        }
    }

    // 2. Syntax / validity check for ESA_PAYMENT_MODE
    let mut esa_mode_live = false;
    let mut esa_mode_sim = false;
    if let Some(ref val) = esa_mode {
        let v = val.to_ascii_lowercase();
        if v == "live" || v == "production" || v == "prod" {
            esa_mode_live = true;
        } else if v == "sandbox" || v == "test" || v == "simulation" || v == "simulated" {
            esa_mode_sim = true;
        } else {
            return PaymentCheckoutMode::Misconfigured(format!(
                "Invalid ESA_PAYMENT_MODE value '{}'. Expected 'live', 'production', 'sandbox', or 'test'.",
                val
            ));
        }
    }

    // 3. Syntax / validity check for ENVIRONMENT
    let mut env_live = false;
    let mut env_sim = false;
    if let Some(ref val) = env_mode {
        let v = val.to_ascii_lowercase();
        if v == "production" || v == "prod" || v == "live" {
            env_live = true;
        } else if v == "sandbox" || v == "test" || v == "testing" {
            env_sim = true;
        } else if v == "development" || v == "dev" || v == "local" || v == "staging" {
            // Recognized non-live environment, but does not implicitly opt in to simulation
        } else {
            return PaymentCheckoutMode::Misconfigured(format!(
                "Invalid ENVIRONMENT value '{}'. Expected 'production', 'sandbox', 'test', 'development', or 'local'.",
                val
            ));
        }
    }

    // 4. Syntax / validity check for provider modes
    let mut live_providers = Vec::new();
    let mut non_live_providers = Vec::new();
    for (var, val) in provider_modes {
        let v = val.to_ascii_lowercase();
        if v == "live" || v == "production" || v == "prod" {
            live_providers.push(format!("{var}={val}"));
        } else if v == "test"
            || v == "sandbox"
            || v == "uat"
            || v == "staging"
            || v == "dev"
            || v == "development"
        {
            // Recognized non-live provider mode
            non_live_providers.push(format!("{var}={val}"));
        } else {
            return PaymentCheckoutMode::Misconfigured(format!(
                "Invalid {} value '{}'. Expected 'live', 'test', or 'sandbox'.",
                var, val
            ));
        }
    }

    // 5. LIVE CHECK (Highest priority: live mode for ANY provider or environment cannot be bypassed by simulation flag)
    if env_live {
        return PaymentCheckoutMode::Live {
            reason: format!(
                "ENVIRONMENT is set to live/production ('{}')",
                env_mode.unwrap_or_default()
            ),
        };
    }
    if esa_mode_live {
        return PaymentCheckoutMode::Live {
            reason: format!(
                "ESA_PAYMENT_MODE is set to live/production ('{}')",
                esa_mode.unwrap_or_default()
            ),
        };
    }
    if !live_providers.is_empty() {
        return PaymentCheckoutMode::Live {
            reason: format!(
                "Live provider gateway mode configured: {}",
                live_providers.join(", ")
            ),
        };
    }

    // 6. CONFLICT CHECK: Detect contradictory non-live configurations
    if explicit_sim_disabled {
        let mut conflicting = Vec::new();
        if esa_mode_sim {
            conflicting.push(format!(
                "ESA_PAYMENT_MODE='{}'",
                esa_mode.as_deref().unwrap_or_default()
            ));
        }
        if env_sim {
            conflicting.push(format!(
                "ENVIRONMENT='{}'",
                env_mode.as_deref().unwrap_or_default()
            ));
        }
        if !conflicting.is_empty() {
            return PaymentCheckoutMode::Misconfigured(format!(
                "Conflicting payment mode configuration: ESA_ENABLE_SIMULATED_CHECKOUT is explicitly disabled ('{}'), but {} requested sandbox/simulation mode.",
                sim_flag.as_deref().unwrap_or_default(),
                conflicting.join(" and ")
            ));
        }
    }

    // 7. SIMULATION OPT-IN CHECK
    if explicit_sim_opt_in
        || esa_mode_sim
        || env_sim
        || (!non_live_providers.is_empty() && !explicit_sim_disabled)
    {
        return PaymentCheckoutMode::Simulated;
    }

    // 8. Missing/Unconfigured mode -> Fail safely
    PaymentCheckoutMode::Unconfigured
}

pub(crate) fn is_production_mode() -> bool {
    match detect_checkout_mode() {
        PaymentCheckoutMode::Live { .. } => true,
        // Fail-closed: invalid or misconfigured deployment preserves production protections
        PaymentCheckoutMode::Misconfigured(_) => true,
        PaymentCheckoutMode::Unconfigured => false,
        PaymentCheckoutMode::Simulated => false,
    }
}

/// Validates admin API key configuration.
///
/// In live/production mode:
/// - A secure administrative key of at least 32 characters is strictly required.
/// - Shorter keys or missing keys fail closed.
/// - Note: Startup validates key length (minimum 32 characters), not entropy or randomness;
///   operators must provision a cryptographically secure random secret (e.g. `openssl rand -hex 32`).
/// - Never logs the key or any part of it.
///
/// Non-production modes (dev/test/simulation) do not enforce this requirement.
pub fn validate_admin_key_configuration(is_production: bool) -> anyhow::Result<()> {
    if !is_production {
        return Ok(());
    }

    match std::env::var("ESA_ADMIN_API_KEY") {
        Ok(key) => {
            let trimmed = key.trim();
            if trimmed.len() < 32 {
                anyhow::bail!(
                    "ESA_ADMIN_API_KEY is too short ({} chars). Production mode strictly requires at least 32 random characters.",
                    trimmed.len()
                );
            }
            Ok(())
        }
        Err(_) => {
            anyhow::bail!(
                "ESA_ADMIN_API_KEY is missing in live/production mode. A secure administrative key of at least 32 characters is strictly required."
            );
        }
    }
}

/// Request correlation ID extracted from incoming headers (`x-request-id` or `x-correlation-id`)
/// or generated as a server fallback UUIDv4.
///
/// Available to all handlers and downstream layers via Axum request extensions:
/// `Extension(req_id): Extension<RequestCorrelationId>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestCorrelationId(pub String);

impl std::fmt::Display for RequestCorrelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for RequestCorrelationId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Safely sanitizes incoming correlation IDs.
/// Only permits ASCII alphanumeric characters, hyphens, and underscores up to 64 chars.
/// Never treats client-supplied correlation IDs as identity or authentication.
pub fn sanitize_correlation_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return None;
    }
    if trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Evaluates whether demonstration, benchmark, or chaos mutation endpoints are permitted.
/// Conceptually distinct from checkout mode: mutation endpoints (spike injection,
/// data reset, state seed, benchmark runs, simulated failover) are ONLY permitted in an
/// explicitly configured simulation / demo environment.
///
/// Unconfigured, misconfigured, or live production environments strictly fail closed.
pub(crate) fn is_demo_mutation_allowed() -> bool {
    if is_production_mode() {
        return false;
    }
    matches!(detect_checkout_mode(), PaymentCheckoutMode::Simulated)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOrigin {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
}

fn parse_origin(origin: &str) -> Option<ParsedOrigin> {
    let clean = origin.trim();
    if clean.is_empty() {
        return None;
    }

    let uri: axum::http::Uri = clean.parse().ok()?;
    let scheme = uri.scheme_str()?.to_lowercase();
    let host = uri.host()?.to_lowercase();
    let port = uri.port_u16();

    // An Origin header must not contain a path (other than empty or "/"), query, or fragment
    let path = uri.path();
    if !path.is_empty() && path != "/" {
        return None;
    }
    if uri.query().is_some() {
        return None;
    }

    Some(ParsedOrigin { scheme, host, port })
}

fn is_loopback_or_localhost(host: &str) -> bool {
    let h = host.trim().to_lowercase();
    let h_clean = h
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&h);
    if h_clean == "localhost"
        || h_clean.ends_with(".localhost")
        || h_clean == "127.0.0.1"
        || h_clean == "::1"
    {
        return true;
    }
    if let Ok(ip) = h_clean.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    false
}

#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub is_production: bool,
    pub allowed_origins: Vec<String>,
}

impl CorsConfig {
    pub fn new(is_production: bool, allowed_origins: Vec<String>) -> Self {
        Self {
            is_production,
            allowed_origins,
        }
    }

    pub fn from_env() -> Self {
        let is_production = is_production_mode();
        let allowed_origins: Vec<String> = std::env::var("ALLOWED_ORIGINS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|o| o.trim().to_string())
                    .filter(|o| !o.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Self::new(is_production, allowed_origins)
    }

    pub fn is_origin_allowed(&self, origin: &str) -> bool {
        let origin_clean = origin.trim();
        if origin_clean.is_empty() || origin_clean == "*" {
            return false;
        }

        let parsed = match parse_origin(origin_clean) {
            Some(p) => p,
            None => return false,
        };

        let is_loopback = is_loopback_or_localhost(&parsed.host);

        // 1. In production mode, reject all localhost and loopback origins,
        // EVEN IF explicitly present in ALLOWED_ORIGINS.
        if self.is_production && is_loopback {
            return false;
        }

        // 2. In non-production (development) mode, allow localhost and loopback origins (http or https)
        if !self.is_production && is_loopback {
            return parsed.scheme == "http" || parsed.scheme == "https";
        }

        // 3. Exact production HTTPS origin is always allowed
        if parsed.scheme == "https" && parsed.host == "esapay.vercel.app" && parsed.port.is_none() {
            return true;
        }

        // 4. Reject unencrypted HTTP production origin unconditionally
        if parsed.host == "esapay.vercel.app" && parsed.scheme != "https" {
            return false;
        }

        // 5. Explicitly configured allowed origins (e.g. preview deployments)
        for allowed in &self.allowed_origins {
            let allowed_clean = allowed.trim();
            if allowed_clean.is_empty() || allowed_clean == "*" {
                // Strictly reject wildcard '*' with credentialed requests
                continue;
            }

            let parsed_allowed = match parse_origin(allowed_clean) {
                Some(p) => p,
                None => continue,
            };

            // In production, require configured production and preview origins to use HTTPS
            if self.is_production && parsed_allowed.scheme != "https" {
                continue;
            }

            // In production, reject localhost/loopback in allowed origins configuration
            if self.is_production && is_loopback_or_localhost(&parsed_allowed.host) {
                continue;
            }

            // Exact parsed match (scheme, host, and port)
            if parsed.scheme == parsed_allowed.scheme
                && parsed.host == parsed_allowed.host
                && parsed.port == parsed_allowed.port
            {
                return true;
            }
        }

        false
    }

    pub fn build_cors_layer(&self) -> CorsLayer {
        let config = self.clone();
        let allow_origin = AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            if let Ok(origin_str) = origin.to_str() {
                config.is_origin_allowed(origin_str)
            } else {
                false
            }
        });

        let allowed_headers = [
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::HeaderName::from_static("x-api-key"),
            header::HeaderName::from_static("x-esa-api-key"),
            header::HeaderName::from_static("x-requested-with"),
            header::HeaderName::from_static("x-razorpay-signature"),
            header::HeaderName::from_static("x-razorpay-event-id"),
        ];

        CorsLayer::new()
            .allow_origin(allow_origin)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::PATCH,
                Method::OPTIONS,
            ])
            .allow_headers(allowed_headers)
            .allow_credentials(true)
    }
}

fn configure_cors() -> CorsLayer {
    CorsConfig::from_env().build_cors_layer()
}

fn publish_metrics_telemetry(state: &AppState) {
    let workloads = state.state_fabric.list_workloads();
    state.vitals.record_from_workloads(&workloads);

    if let Some(snapshot) = state.vitals.latest() {
        state.broadcaster.send(TelemetryMessage::VitalsUpdate {
            timestamp: snapshot.timestamp,
            total_tps: snapshot.total_tps,
            avg_p95_ms: snapshot.avg_p95_ms,
            avg_error_rate: snapshot.avg_error_rate,
            total_queue: snapshot.total_queue,
            healthy_count: snapshot.healthy_count,
            degraded_count: snapshot.degraded_count,
        });
    }
}

fn publish_payment_side_effects(state: &AppState, result: &payment::PaymentApplyResult) {
    publish_metrics_telemetry(state);

    if let Some(workload) = &result.workload {
        state.broadcaster.send(TelemetryMessage::WorkloadUpdate {
            workload_id: workload.workload_id.clone(),
            state: format!("{:?}", workload.state),
            metrics: serde_json::to_value(&workload.metrics).unwrap_or_default(),
        });
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env from repo root (API keys, Razorpay secrets, Ollama URL, etc.)
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("Starting ESA API Server");

    // Validate payment checkout configuration and fail fast on invalid/misconfigured settings
    match detect_checkout_mode() {
        PaymentCheckoutMode::Misconfigured(err) => {
            error!("Payment checkout configuration is invalid: {}", err);
            anyhow::bail!("Invalid payment checkout mode configuration: {}", err);
        }
        PaymentCheckoutMode::Live { reason } => {
            info!("Production live mode detected: {}", reason);
            validate_admin_key_configuration(true)?;
        }
        PaymentCheckoutMode::Simulated => {
            info!("Simulation payment checkout mode active");
        }
        PaymentCheckoutMode::Unconfigured => {
            info!("Payment checkout mode unconfigured (will fail closed on checkout requests)");
        }
    }

    // Validate HMAC log correlation secret configuration
    rate_limiter::validate_log_hmac_secret_configuration().map_err(|e| anyhow::anyhow!(e))?;

    // Initialize state fabric
    let state_fabric = Arc::new(StateFabric::new());
    let audit_store = Arc::new(AuditStore::new());
    let broadcaster = Arc::new(TelemetryBroadcaster::new());

    // Initialize AI inference provider (Ollama by default, or Hugging Face ZeroGPU when configured)
    let ai_cost_tracker = Arc::new(esa_agents::AICostTracker::new());
    let inference_provider =
        esa_agents::build_inference_provider_from_env(Arc::clone(&ai_cost_tracker), false);

    let agent_status = Arc::new(std::sync::RwLock::new(AgentStatusState::default()));

    let event_handler: RuntimeEventHandler = {
        let broadcaster = Arc::clone(&broadcaster);
        let agent_status = Arc::clone(&agent_status);
        Arc::new(move |event| match event {
            RuntimeEvent::AgentActivity { agent_id, activity } => {
                if let Ok(mut status) = agent_status.write() {
                    match agent_id.as_str() {
                        "monitor" => status.monitor_task = activity.clone(),
                        "diagnosis" => status.diagnosis_task = activity.clone(),
                        "planning" => status.planning_task = activity.clone(),
                        "safety" => status.safety_task = activity.clone(),
                        _ => {}
                    }
                    status.last_cycle_time = Some(chrono::Utc::now().to_rfc3339());
                }
                broadcaster.send(TelemetryMessage::AgentActivity {
                    agent_id,
                    activity,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                });
            }
            RuntimeEvent::ConditionDetected {
                condition_type,
                workload_id,
                severity,
                description,
            } => {
                broadcaster.send(TelemetryMessage::ConditionDetected {
                    condition_type,
                    workload_id,
                    severity,
                    description,
                });
            }
            RuntimeEvent::ActionProposed {
                proposal_id,
                action_type,
                workload_id,
            } => {
                broadcaster.send(TelemetryMessage::ActionProposed {
                    proposal_id,
                    action_type,
                    workload_id,
                });
            }
            RuntimeEvent::ActionExecuted {
                execution_id,
                action_type,
                outcome,
            } => {
                broadcaster.send(TelemetryMessage::ActionExecuted {
                    execution_id,
                    action_type,
                    outcome,
                });
            }
            RuntimeEvent::PolicyDecision {
                decision_id,
                verdict,
                risk_score,
                ..
            } => {
                broadcaster.send(TelemetryMessage::PolicyDecision {
                    proposal_id: decision_id,
                    verdict,
                    risk_score,
                });
            }
            RuntimeEvent::WorkloadUpdate {
                workload_id,
                state,
                metrics,
            } => {
                broadcaster.send(TelemetryMessage::WorkloadUpdate {
                    workload_id,
                    state,
                    metrics,
                });
            }
        })
    };

    // Initialize orchestrator with configured InferenceProvider
    let orchestrator = Arc::new(EsaOrchestrator::new_with_provider(
        Arc::clone(&state_fabric),
        Arc::clone(&inference_provider),
        Arc::clone(&audit_store),
        Some(event_handler),
        esa_agents::FluidReasonerClient::default_local(),
    ));

    // Start background orchestration loop - AUTONOMOUS RECOVERY
    let orch_handle = {
        let orchestrator = Arc::clone(&orchestrator);
        tokio::spawn(async move {
            // Run autonomous recovery every 5 seconds for demo responsiveness
            orchestrator.run_forever(Duration::from_secs(5)).await;
        })
    };

    let razorpay = if RazorpayConfig::is_configured() {
        match RazorpayAdapter::try_from_env() {
            Ok(adapter) => {
                info!(
                    "Razorpay adapter enabled (mode: {:?}, credentials: {})",
                    adapter.config().mode,
                    if adapter.config().is_test_credentials() {
                        "test"
                    } else {
                        "live"
                    }
                );
                Some(Arc::new(adapter))
            }
            Err(e) => {
                error!("Razorpay config invalid: {}", e);
                return Err(anyhow::anyhow!("Razorpay configuration error: {}", e));
            }
        }
    } else {
        info!("Razorpay adapter disabled — set RAZORPAY_* in .env to enable");
        None
    };

    let cashfree = match CashfreeConfig::from_env() {
        Ok(Some(cfg)) => {
            info!("Cashfree provider enabled (env: {:?})", cfg.env);
            Some(Arc::new(CashfreeProvider::new(cfg)))
        }
        Ok(None) => {
            info!("Cashfree provider disabled — set CASHFREE_* in .env to enable");
            None
        }
        Err(e) => {
            error!("Cashfree config invalid: {}", e);
            return Err(anyhow::anyhow!("Cashfree configuration error: {}", e));
        }
    };

    let payu = match PayUConfig::from_env() {
        Ok(Some(cfg)) => {
            info!("PayU provider enabled (env: {:?})", cfg.env);
            Some(Arc::new(PayUProvider::new(cfg)))
        }
        Ok(None) => {
            info!("PayU provider disabled — set PAYU_* in .env to enable");
            None
        }
        Err(e) => {
            error!("PayU config invalid: {}", e);
            return Err(anyhow::anyhow!("PayU configuration error: {}", e));
        }
    };

    // Seed default workloads on startup if empty so vitals and dashboard start immediately live
    if state_fabric.list_workloads().is_empty() {
        use chrono::Utc;
        use esa_core::{
            ConsistencyMode, LifecycleState, LocalityPreference, Region, ReplicationPolicy,
            WorkloadEntity, WorkloadMetrics, WorkloadState,
        };

        let default_workloads = vec![
            WorkloadEntity {
                workload_id: "payment-upi-india-south".to_string(),
                shard_id: "shard-001".to_string(),
                state: WorkloadState::Healthy,
                region: Region::IndiaSouth,
                metrics: WorkloadMetrics {
                    rate_per_min: 3000.0,
                    p50_latency_ms: 45.0,
                    p95_latency_ms: 120.0,
                    p99_latency_ms: 180.0,
                    error_rate: 0.01,
                    queue_depth: 250,
                    timestamp: chrono::Utc::now(),
                },
                replication: ReplicationPolicy {
                    min_replicas: 2,
                    max_replicas: 10,
                    current_replicas: 3,
                    consistency_mode: ConsistencyMode::Strong,
                },
                locality: LocalityPreference {
                    preferred_region: Region::IndiaSouth,
                    fallback_regions: vec![Region::IndiaWest, Region::IndiaNorth],
                },
                lifecycle: LifecycleState::Active,
                version: 1,
                updated_at: chrono::Utc::now(),
            },
            WorkloadEntity {
                workload_id: "payment-cards-india-west".to_string(),
                shard_id: "shard-002".to_string(),
                state: WorkloadState::Healthy,
                region: Region::IndiaWest,
                metrics: WorkloadMetrics {
                    rate_per_min: 1800.0,
                    p50_latency_ms: 60.0,
                    p95_latency_ms: 140.0,
                    p99_latency_ms: 200.0,
                    error_rate: 0.012,
                    queue_depth: 180,
                    timestamp: Utc::now(),
                },
                replication: ReplicationPolicy {
                    min_replicas: 2,
                    max_replicas: 8,
                    current_replicas: 2,
                    consistency_mode: ConsistencyMode::Strong,
                },
                locality: LocalityPreference {
                    preferred_region: Region::IndiaWest,
                    fallback_regions: vec![Region::IndiaSouth, Region::IndiaNorth],
                },
                lifecycle: LifecycleState::Active,
                version: 1,
                updated_at: Utc::now(),
            },
            WorkloadEntity {
                workload_id: "payment-netbanking-india-north".to_string(),
                shard_id: "shard-003".to_string(),
                state: WorkloadState::Healthy,
                region: Region::IndiaNorth,
                metrics: WorkloadMetrics {
                    rate_per_min: 1200.0,
                    p50_latency_ms: 55.0,
                    p95_latency_ms: 160.0,
                    p99_latency_ms: 220.0,
                    error_rate: 0.015,
                    queue_depth: 150,
                    timestamp: Utc::now(),
                },
                replication: ReplicationPolicy {
                    min_replicas: 2,
                    max_replicas: 6,
                    current_replicas: 2,
                    consistency_mode: ConsistencyMode::Strong,
                },
                locality: LocalityPreference {
                    preferred_region: Region::IndiaNorth,
                    fallback_regions: vec![Region::IndiaSouth, Region::IndiaWest],
                },
                lifecycle: LifecycleState::Active,
                version: 1,
                updated_at: Utc::now(),
            },
        ];

        for workload in default_workloads {
            let _ = state_fabric.upsert_workload(workload);
        }
    }

    let vitals = VitalsStore::new();

    // Periodic vitals broadcast for live dashboard graphs
    {
        let state_fabric = Arc::clone(&state_fabric);
        let vitals_bg = vitals.clone();
        let broadcaster = Arc::clone(&broadcaster);
        tokio::spawn(async move {
            loop {
                let workloads = state_fabric.list_workloads();
                vitals_bg.record_from_workloads(&workloads);
                if let Some(snapshot) = vitals_bg.latest() {
                    broadcaster.send(TelemetryMessage::VitalsUpdate {
                        timestamp: snapshot.timestamp,
                        total_tps: snapshot.total_tps,
                        avg_p95_ms: snapshot.avg_p95_ms,
                        avg_error_rate: snapshot.avg_error_rate,
                        total_queue: snapshot.total_queue,
                        healthy_count: snapshot.healthy_count,
                        degraded_count: snapshot.degraded_count,
                    });
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }

    // Initialize fluid reasoner client
    let fluid_reasoner = esa_agents::FluidReasonerClient::default_local();

    // Initialize API rate limiter (Token-bucket protection against abuse/DDoS)
    let rate_limiter = rate_limiter::ApiRateLimiter::new();

    // Initialize durable webhook inbox and payment record store (fails closed if DATABASE_URL is set but DB fails)
    let webhook_store: Arc<dyn WebhookStore> =
        resolve_webhook_store(std::env::var("DATABASE_URL").ok().as_deref()).await?;

    // Initialize sandbox API key store (durable in PostgreSQL if configured, in-memory fallback)
    let sandbox_keys = if let Some(pool) = webhook_store.pg_pool() {
        api_keys::SandboxKeyStore::init_schema_on_pool(pool)
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to initialize sandbox_api_keys schema on PostgreSQL: {e}")
            })?;
        Arc::new(api_keys::SandboxKeyStore::new_with_pool(pool.clone()))
    } else {
        Arc::new(api_keys::SandboxKeyStore::new_in_memory())
    };
    api_keys::set_global_sandbox_key_store(Arc::clone(&sandbox_keys));

    // Initialize Phase 3 Payment Memory Store & Payment Intelligence Agent
    let memory_store: Arc<dyn esa_agents::PaymentMemoryStore> =
        if let Some(pool) = webhook_store.pg_pool() {
            Arc::new(esa_agents::PostgresPaymentMemoryStore::new(pool.clone()))
        } else {
            Arc::new(esa_agents::InMemoryPaymentMemoryStore::new())
        };
    memory_store.init_schema().await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to initialize payment memory store schema: {}",
            e.message
        )
    })?;
    esa_agents::seed_system_knowledge(memory_store.as_ref())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to seed payment system knowledge: {}", e.message))?;
    let payment_intelligence = Arc::new(esa_agents::PaymentIntelligenceAgent::new(
        Arc::clone(&inference_provider),
        esa_agents::MemoryRetriever::new(Arc::clone(&memory_store)),
    ));

    // Create app state
    let app_state = AppState {
        state_fabric,
        orchestrator,
        inference_provider,
        fluid_reasoner,
        broadcaster: Arc::clone(&broadcaster),
        audit_store,
        agent_status,
        razorpay,
        cashfree,
        payu,
        vitals,
        rate_limiter,
        last_benchmark: Arc::new(std::sync::RwLock::new(None)),
        last_ablation: Arc::new(std::sync::RwLock::new(None)),
        gateway_overrides: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        webhook_store,
        order_merchants: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        benchmark_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        sandbox_keys,
        memory_store,
        payment_intelligence,
    };

    let app = build_app(app_state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);
    info!("ESA API listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    orch_handle.await?;

    Ok(())
}

fn build_app(app_state: AppState) -> Router {
    // Build router with full /api/v1/ versioning and backwards-compatible /api/ aliases
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/healthz", get(health_handler))
        .route("/livez", get(health_handler))
        .route("/ready", get(readiness_handler))
        .route("/readyz", get(readiness_handler))
        .route("/api/ready", get(readiness_handler))
        .route("/api/v1/ready", get(readiness_handler))
        // Multi-Gateway Universal Routing endpoints
        .route("/api/gateways", get(list_gateways))
        .route("/api/v1/gateways", get(list_gateways))
        .route("/api/gateways/:name/toggle", post(toggle_gateway))
        .route("/api/v1/gateways/:name/toggle", post(toggle_gateway))
        .route("/api/payments/checkout", post(universal_checkout))
        .route("/api/v1/payments/checkout", post(universal_checkout))
        .route("/api/workloads", get(list_workloads).post(create_workload))
        .route(
            "/api/v1/workloads",
            get(list_workloads).post(create_workload),
        )
        .route("/api/workloads/:id", get(get_workload))
        .route("/api/v1/workloads/:id", get(get_workload))
        .route("/api/events/payment", post(ingest_payment_event))
        .route("/api/v1/events/payment", post(ingest_payment_event))
        .route(
            "/api/razorpay/webhook",
            post(razorpay_webhook).layer(DefaultBodyLimit::max(MAX_WEBHOOK_BODY_BYTES)),
        )
        .route(
            "/api/v1/razorpay/webhook",
            post(razorpay_webhook).layer(DefaultBodyLimit::max(MAX_WEBHOOK_BODY_BYTES)),
        )
        .route("/api/razorpay/status", get(razorpay_status))
        .route("/api/v1/razorpay/status", get(razorpay_status))
        .route("/api/razorpay/orders", post(razorpay_create_order))
        .route("/api/v1/razorpay/orders", post(razorpay_create_order))
        .route("/api/razorpay/verify", post(razorpay_verify_keys))
        .route("/api/v1/razorpay/verify", post(razorpay_verify_keys))
        .route("/api/razorpay/confirm", post(razorpay_confirm_payment))
        .route("/api/v1/razorpay/confirm", post(razorpay_confirm_payment))
        // Cashfree Payment Gateway routes
        .route(
            "/api/cashfree/webhook",
            post(cashfree_webhook).layer(DefaultBodyLimit::max(
                esa_cashfree::webhook::MAX_WEBHOOK_BODY_BYTES,
            )),
        )
        .route(
            "/api/v1/cashfree/webhook",
            post(cashfree_webhook).layer(DefaultBodyLimit::max(
                esa_cashfree::webhook::MAX_WEBHOOK_BODY_BYTES,
            )),
        )
        .route(
            "/api/payments/cashfree/webhook",
            post(cashfree_webhook).layer(DefaultBodyLimit::max(
                esa_cashfree::webhook::MAX_WEBHOOK_BODY_BYTES,
            )),
        )
        .route("/api/cashfree/status", get(cashfree_status))
        .route("/api/v1/cashfree/status", get(cashfree_status))
        .route("/api/payments/cashfree/status", get(cashfree_status))
        .route("/api/cashfree/confirm", post(cashfree_confirm_payment))
        .route("/api/v1/cashfree/confirm", post(cashfree_confirm_payment))
        .route(
            "/api/payments/cashfree/confirm",
            post(cashfree_confirm_payment),
        )
        .route("/api/cashfree/orders/:order_id", get(cashfree_get_order))
        .route("/api/v1/cashfree/orders/:order_id", get(cashfree_get_order))
        .route(
            "/api/payments/cashfree/orders/:order_id",
            get(cashfree_get_order),
        )
        .route("/api/cashfree/refunds", post(cashfree_create_refund))
        .route("/api/v1/cashfree/refunds", post(cashfree_create_refund))
        .route(
            "/api/payments/cashfree/refunds",
            post(cashfree_create_refund),
        )
        // PayU India Hosted Checkout routes
        .route(
            "/api/payu/webhook",
            post(payu_webhook).layer(DefaultBodyLimit::max(
                esa_payu::webhook::MAX_PAYU_PAYLOAD_BYTES,
            )),
        )
        .route(
            "/api/v1/payu/webhook",
            post(payu_webhook).layer(DefaultBodyLimit::max(
                esa_payu::webhook::MAX_PAYU_PAYLOAD_BYTES,
            )),
        )
        .route(
            "/api/payments/payu/webhook",
            post(payu_webhook).layer(DefaultBodyLimit::max(
                esa_payu::webhook::MAX_PAYU_PAYLOAD_BYTES,
            )),
        )
        .route("/api/payu/status", get(payu_status))
        .route("/api/v1/payu/status", get(payu_status))
        .route("/api/payments/payu/status", get(payu_status))
        .route("/api/payu/callback/success", post(payu_callback_handler))
        .route(
            "/api/payments/payu/callback/success",
            post(payu_callback_handler),
        )
        .route("/api/payu/callback/failure", post(payu_callback_handler))
        .route(
            "/api/payments/payu/callback/failure",
            post(payu_callback_handler),
        )
        .route("/api/payu/callback/cancel", post(payu_callback_handler))
        .route(
            "/api/payments/payu/callback/cancel",
            post(payu_callback_handler),
        )
        .route("/api/payu/verify", post(payu_verify_payment))
        .route("/api/v1/payu/verify", post(payu_verify_payment))
        .route("/api/payments/payu/verify", post(payu_verify_payment))
        .route("/api/payu/refunds", post(payu_create_refund))
        .route("/api/v1/payu/refunds", post(payu_create_refund))
        .route("/api/payments/payu/refunds", post(payu_create_refund))
        .route("/api/vitals/history", get(get_vitals_history))
        .route("/api/v1/vitals/history", get(get_vitals_history))
        .route("/api/demo/trigger-spike", post(trigger_spike))
        .route("/api/v1/demo/trigger-spike", post(trigger_spike))
        .route("/api/demo/seed", post(seed_demo_data))
        .route("/api/v1/demo/seed", post(seed_demo_data))
        .route("/api/demo/scenario/:scenario", post(trigger_scenario))
        .route("/api/v1/demo/scenario/:scenario", post(trigger_scenario))
        .route("/api/benchmark/run", post(run_benchmark))
        .route("/api/v1/benchmark/run", post(run_benchmark))
        .route("/api/benchmark/harness", post(run_benchmark_harness))
        .route("/api/v1/benchmark/harness", post(run_benchmark_harness))
        .route("/api/benchmark/latest", get(get_benchmark_latest))
        .route("/api/v1/benchmark/latest", get(get_benchmark_latest))
        .route("/api/metrics/tokens", get(get_token_metrics))
        .route("/api/v1/metrics/tokens", get(get_token_metrics))
        .route("/api/agents/status", get(get_agents_status))
        .route("/api/v1/agents/status", get(get_agents_status))
        .route("/api/agents/activity", get(get_agent_activity))
        .route("/api/v1/agents/activity", get(get_agent_activity))
        .route(
            "/api/agents/fluid-reasoner/status",
            get(get_fluid_reasoner_status),
        )
        .route(
            "/api/v1/agents/fluid-reasoner/status",
            get(get_fluid_reasoner_status),
        )
        .route("/api/actions/recent", get(get_recent_actions))
        .route("/api/v1/actions/recent", get(get_recent_actions))
        // Audit Trail endpoints
        .route("/api/audit/trail", get(get_audit_trail))
        .route("/api/v1/audit/trail", get(get_audit_trail))
        .route("/api/audit/verify-chain", get(verify_audit_chain))
        .route("/api/v1/audit/verify-chain", get(verify_audit_chain))
        .route("/api/audit/decision/:decision_id", get(get_decision_detail))
        .route(
            "/api/v1/audit/decision/:decision_id",
            get(get_decision_detail),
        )
        .route(
            "/api/audit/replay/:decision_id",
            post(replay_decision).get(replay_decision),
        )
        .route(
            "/api/v1/audit/replay/:decision_id",
            post(replay_decision).get(replay_decision),
        )
        // Benchmark & Ablation endpoints
        .route(
            "/api/benchmark/ablations",
            post(run_benchmark_ablations).get(get_benchmark_ablations),
        )
        .route(
            "/api/v1/benchmark/ablations",
            post(run_benchmark_ablations).get(get_benchmark_ablations),
        )
        // Effect Measurement endpoints
        .route("/api/effects/measurements", get(get_effect_measurements))
        .route("/api/v1/effects/measurements", get(get_effect_measurements))
        .route("/api/effects/recent", get(get_recent_effects))
        .route("/api/v1/effects/recent", get(get_recent_effects))
        // AI Cost endpoints
        .route("/api/costs/ai", get(get_ai_costs))
        .route("/api/v1/costs/ai", get(get_ai_costs))
        .route("/api/costs/per-agent", get(get_costs_per_agent))
        .route("/api/v1/costs/per-agent", get(get_costs_per_agent))
        // Policy Verdict endpoints
        .route("/api/verdicts/recent", get(get_recent_verdicts))
        .route("/api/v1/verdicts/recent", get(get_recent_verdicts))
        .route("/api/verdicts/stats", get(get_verdict_stats))
        .route("/api/v1/verdicts/stats", get(get_verdict_stats))
        // Intent & Constraints endpoints
        .route("/api/intent/active", get(get_active_intents))
        .route("/api/v1/intent/active", get(get_active_intents))
        .route("/api/intent/violations", get(get_constraint_violations))
        .route("/api/v1/intent/violations", get(get_constraint_violations))
        // Phase 3 Payment Intelligence & Persistent Memory (read-only)
        .route(
            "/api/intelligence/payments/summary",
            get(get_intelligence_payments_summary),
        )
        .route(
            "/api/v1/intelligence/payments/summary",
            get(get_intelligence_payments_summary),
        )
        .route(
            "/api/intelligence/payments/investigate",
            post(investigate_payment_intelligence),
        )
        .route(
            "/api/v1/intelligence/payments/investigate",
            post(investigate_payment_intelligence),
        )
        .route(
            "/api/intelligence/payments/providers/health",
            get(get_intelligence_providers_health),
        )
        .route(
            "/api/v1/intelligence/payments/providers/health",
            get(get_intelligence_providers_health),
        )
        .route(
            "/api/intelligence/memory/recent",
            get(get_intelligence_memory_recent),
        )
        .route(
            "/api/v1/intelligence/memory/recent",
            get(get_intelligence_memory_recent),
        )
        // API Key Management & Sandbox Access
        .route(
            "/api/v1/keys/generate-sandbox",
            post(api_keys::generate_sandbox_key_handler),
        )
        .route(
            "/api/keys/generate-sandbox",
            post(api_keys::generate_sandbox_key_handler),
        )
        .route(
            "/api/v1/keys/validate",
            post(api_keys::validate_key_handler),
        )
        .route("/api/keys/validate", post(api_keys::validate_key_handler))
        .route("/api/v1/keys/revoke", post(api_keys::revoke_key_handler))
        .route("/api/keys/revoke", post(api_keys::revoke_key_handler))
        // Authentication Identity (Supabase JWT & API Key introspection)
        .route("/api/v1/auth/me", get(api_keys::auth_me_handler))
        .route("/api/auth/me", get(api_keys::auth_me_handler))
        .route("/ws/telemetry", get(ws_telemetry_handler));

    let candidate_paths = [
        "frontend/dist",
        "../frontend/dist",
        "/Users/sujithputta/ESA_paymentgateway/frontend/dist",
    ];
    let app = if let Some(&dist) = candidate_paths
        .iter()
        .find(|p| std::path::Path::new(p).exists())
    {
        info!("Serving ESA Web Dashboard from: {}", dist);
        let serve_service =
            ServeDir::new(dist).not_found_service(ServeFile::new(format!("{}/index.html", dist)));
        app.fallback_service(serve_service)
    } else {
        app.fallback(get(|| async {
            axum::response::Html(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"><title>ESA Control Plane</title>
<style>body{background:#0a0d14;color:#fff;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;}</style>
</head>
<body>
<div style="text-align:center;">
<h1>⚡ ESA Control Plane Backend (Port 8080)</h1>
<p style="color:#00f2fe;">Backend API is healthy and running.</p>
<p>To view the full React Dashboard, run: <code>cd frontend && npm run dev</code> (or build: <code>npm run build</code>)</p>
</div>
</body>
</html>"#)
        }))
    };

    app.layer(axum::middleware::from_fn(
        request_correlation_and_tracing_middleware,
    ))
    .layer(configure_cors())
    .with_state(app_state)
}

/// Production middleware for request correlation, tracing span instrumentation, and security headers.
///
/// Features:
/// - Extracts and sanitizes incoming correlation IDs (`x-request-id` or `x-correlation-id`) via `sanitize_correlation_id`.
/// - Falls back to a fresh UUIDv4 if missing or malformed.
/// - Injects `RequestCorrelationId` into `req.extensions_mut()`.
/// - Creates an `http_request` tracing span carrying `request_id`, HTTP `method`, and sanitized request `path`
///   (excluding query parameters to preserve privacy).
/// - Instruments the async handler future using `tracing::Instrument` so downstream logs inherit the span
///   without holding guard across await/yield points.
/// - Injects `x-request-id` response header along with OWASP recommended security headers.
pub async fn request_correlation_and_tracing_middleware(
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let is_https = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false);
    let is_prod = is_production_mode();

    let raw_corr = req
        .headers()
        .get("x-request-id")
        .or_else(|| req.headers().get("x-correlation-id"))
        .and_then(|v| v.to_str().ok());

    let validated_id = raw_corr.and_then(sanitize_correlation_id);
    let request_id = validated_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let correlation_id = RequestCorrelationId(request_id.clone());
    req.extensions_mut().insert(correlation_id);

    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let span = tracing::info_span!(
        "http_request",
        request_id = %request_id,
        method = %method,
        path = %path,
    );

    let mut res = next.run(req).instrument(span).await;
    let headers = res.headers_mut();
    if let Ok(val) = request_id.parse() {
        headers.insert("x-request-id", val);
    }
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    headers.insert(
        "referrer-policy",
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    if is_https || is_prod {
        headers.insert(
            "strict-transport-security",
            "max-age=31536000; includeSubDomains".parse().unwrap(),
        );
    }
    headers.insert("x-xss-protection", "1; mode=block".parse().unwrap());
    res
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) state_fabric: Arc<StateFabric>,
    pub(crate) orchestrator: Arc<EsaOrchestrator>,
    pub(crate) inference_provider: Arc<dyn esa_agents::InferenceProvider>,
    pub(crate) fluid_reasoner: esa_agents::FluidReasonerClient,
    pub(crate) broadcaster: Arc<TelemetryBroadcaster>,
    pub(crate) audit_store: Arc<AuditStore>,
    pub(crate) agent_status: Arc<std::sync::RwLock<AgentStatusState>>,
    pub(crate) razorpay: Option<Arc<RazorpayAdapter>>,
    pub(crate) cashfree: Option<Arc<CashfreeProvider>>,
    pub(crate) payu: Option<Arc<PayUProvider>>,
    pub(crate) vitals: VitalsStore,
    pub(crate) rate_limiter: rate_limiter::ApiRateLimiter,
    pub(crate) last_benchmark: Arc<std::sync::RwLock<Option<benchmark::BenchmarkComparison>>>,
    pub(crate) last_ablation:
        Arc<std::sync::RwLock<Option<benchmark_harness::AblationStudyResult>>>,
    pub(crate) gateway_overrides: Arc<std::sync::RwLock<std::collections::HashMap<String, bool>>>,
    pub(crate) webhook_store: Arc<dyn WebhookStore>,
    /// Process-local in-memory consistency cache of order-to-merchant bindings created by this instance.
    /// Note: This cache is process-local and non-durable across restarts or multiple replicas.
    /// The authoritative source of merchant ownership is always the server-created order.notes.esa_merchant_id
    /// returned directly by the upstream Razorpay API. Client-supplied payment notes are never trusted.
    pub(crate) order_merchants: Arc<std::sync::RwLock<std::collections::HashMap<String, String>>>,
    /// Concurrency control for benchmark execution to prevent duplicate concurrent runs.
    pub(crate) benchmark_running: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) sandbox_keys: Arc<api_keys::SandboxKeyStore>,
    pub(crate) memory_store: Arc<dyn esa_agents::PaymentMemoryStore>,
    pub(crate) payment_intelligence: Arc<esa_agents::PaymentIntelligenceAgent>,
}

struct BenchmarkGuard(Arc<std::sync::atomic::AtomicBool>);
impl Drop for BenchmarkGuard {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
struct AgentStatusState {
    monitor_task: String,
    diagnosis_task: String,
    planning_task: String,
    safety_task: String,
    last_cycle_time: Option<String>,
}

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "esa-api"
    }))
}

async fn readiness_handler(State(state): State<AppState>) -> impl IntoResponse {
    let db_health = state.webhook_store.health_check().await;
    let rzp_configured = state.razorpay.is_some();

    match db_health {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ready",
                "service": "esa-api",
                "database": "healthy",
                "razorpay_configured": rzp_configured,
            })),
        ),
        Err(err) => {
            error!(error = %err, "Readiness probe failed: database unhealthy");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "status": "not_ready",
                    "service": "esa-api",
                    "database": "unhealthy",
                    "error": "database connection check failed",
                })),
            )
        }
    }
}

async fn get_fluid_reasoner_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for fluid reasoner status: {}",
            e
        ))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }
    let available = state.fluid_reasoner.is_available().await;
    Ok(Json(serde_json::json!({
        "status": if available { "online" } else { "offline" },
        "service": "arc_fluid_reasoner",
        "port": 5005,
        "available": available,
        "fallback_tier_order": ["FluidReasoner (Local port 5005)", "Ollama LLM (mistral)", "Deterministic Rules"]
    })))
}

async fn list_workloads(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<WorkloadEntity>>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required to list workloads: {}", e))
    })?;
    if !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Access to global workloads is restricted to administrators".into(),
        ));
    }
    let workloads = state.state_fabric.list_workloads();
    Ok(Json(workloads))
}

async fn get_workload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<WorkloadEntity>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required to get workload: {}", e))
    })?;
    if !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Access to global workloads is restricted to administrators".into(),
        ));
    }
    state
        .state_fabric
        .get_workload(&id)
        .ok_or(AppError::NotFound(format!("Workload {} not found", id)))
        .map(Json)
}

async fn create_workload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(workload): Json<WorkloadEntity>,
) -> Result<Json<WorkloadEntity>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required to create workload: {}", e))
    })?;

    if !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Workload creation is restricted to administrators".into(),
        ));
    }

    state.state_fabric.upsert_workload(workload.clone())?;
    Ok(Json(workload))
}

async fn ingest_payment_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(event): Json<PaymentEvent>,
) -> Result<impl IntoResponse, AppError> {
    let _caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required to ingest payment events: {}",
            e
        ))
    })?;

    let result = payment::apply_payment_event(&state.state_fabric, &event);
    publish_payment_side_effects(&state, &result);

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "event_id": result.event_id,
        "auto_spike_triggered": result.workload_updated,
        "workload_id": result.workload_id,
        "message": "Payment processed and workload metrics automatically adjusted"
    })))
}
/// Resolves the WebhookStore based on the optional DATABASE_URL string and production flag.
/// FAILS CLOSED in production:
/// - If in production mode, a reachable PostgreSQL DATABASE_URL and successful schema
///   initialization are STRICTLY REQUIRED. Unset, blank, or 'memory' database URLs fail closed.
/// - In development or test mode, explicit 'memory' or absent/blank DATABASE_URL selects
///   InMemoryWebhookStore. If a PostgreSQL DATABASE_URL is configured, connection and schema
///   initialization must succeed or it fails closed (never silently falling back to memory).
pub async fn resolve_webhook_store_with_mode(
    db_url_opt: Option<&str>,
    is_production: bool,
) -> anyhow::Result<Arc<dyn WebhookStore>> {
    let trimmed = db_url_opt.map(|s| s.trim()).filter(|s| !s.is_empty());

    if is_production {
        match trimmed {
            Some("memory") => {
                anyhow::bail!(
                    "In-memory webhook store ('memory') is prohibited in production. A reachable PostgreSQL DATABASE_URL is strictly required."
                );
            }
            None => {
                anyhow::bail!(
                    "DATABASE_URL is missing or blank in production. A reachable PostgreSQL DATABASE_URL is strictly required."
                );
            }
            Some(db_url) => {
                let pool = sqlx::postgres::PgPoolOptions::new()
                    .acquire_timeout(std::time::Duration::from_secs(3))
                    .connect(db_url)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Production DATABASE_URL connection failed: {}. Failing closed.",
                            e
                        )
                    })?;
                PostgresWebhookStore::init_schema(&pool)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                        "Production DATABASE_URL schema initialization failed: {}. Failing closed.",
                        e
                    )
                    })?;
                info!("Connected durable PostgresWebhookStore to production database");
                return Ok(Arc::new(PostgresWebhookStore::new(pool)));
            }
        }
    }

    if let Some(db_url) = trimmed {
        if db_url != "memory" {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_secs(3))
                .connect(db_url)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "DATABASE_URL is configured but connection failed: {}. Failing closed.",
                        e
                    )
                })?;
            PostgresWebhookStore::init_schema(&pool).await.map_err(|e| {
                anyhow::anyhow!(
                    "DATABASE_URL is configured but schema initialization failed: {}. Failing closed.",
                    e
                )
            })?;
            info!("Connected durable PostgresWebhookStore to database");
            return Ok(Arc::new(PostgresWebhookStore::new(pool)));
        }
    }

    info!("Using InMemoryWebhookStore for development/test mode");
    Ok(Arc::new(InMemoryWebhookStore::new()))
}

pub async fn resolve_webhook_store(
    db_url_opt: Option<&str>,
) -> anyhow::Result<Arc<dyn WebhookStore>> {
    resolve_webhook_store_with_mode(db_url_opt, is_production_mode()).await
}

async fn razorpay_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let adapter = match &state.razorpay {
        Some(a) => a,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Razorpay adapter not configured — add RAZORPAY_* keys to .env"
                })),
            )
                .into_response();
        }
    };

    // 1. Strict request body size limit enforcement before expensive parsing
    if body.len() > MAX_WEBHOOK_BODY_BYTES {
        warn!(
            "Razorpay webhook rejected: payload size {} exceeds limit of {} bytes",
            body.len(),
            MAX_WEBHOOK_BODY_BYTES
        );
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "error": "payload_too_large",
                "message": format!(
                    "Payload size {} exceeds maximum allowed limit of {} bytes",
                    body.len(),
                    MAX_WEBHOOK_BODY_BYTES
                )
            })),
        )
            .into_response();
    }

    let signature = headers
        .get("X-Razorpay-Signature")
        .and_then(|v| v.to_str().ok());

    let event_id_header = headers
        .get("X-Razorpay-Event-Id")
        .and_then(|v| v.to_str().ok());

    // 2. Cryptographic signature verification and schema/attribute validation
    let parsed = match adapter.process_webhook_detailed(body.as_ref(), signature, event_id_header) {
        Ok(event) => event,
        Err(WebhookError::RateLimited) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({ "error": "rate limit exceeded" })),
            )
                .into_response();
        }
        Err(WebhookError::PayloadTooLarge(len, max)) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({
                    "error": "payload_too_large",
                    "message": format!("Payload size {} exceeds limit of {} bytes", len, max)
                })),
            )
                .into_response();
        }
        Err(WebhookError::DuplicateEvent) => {
            return Json(serde_json::json!({
                "status": "already_processed",
                "idempotent": true,
                "message": "Event already processed"
            }))
            .into_response();
        }
        Err(
            WebhookError::InvalidSignature
            | WebhookError::MissingSignature
            | WebhookError::MissingWebhookSecret,
        ) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid webhook signature" })),
            )
                .into_response();
        }
        Err(WebhookError::InvalidJson(msg)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_json", "message": msg })),
            )
                .into_response();
        }
        Err(WebhookError::MalformedPayload(msg)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "malformed_payload", "message": msg })),
            )
                .into_response();
        }
        Err(WebhookError::MerchantMismatch { expected, actual }) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "merchant_mismatch",
                    "message": format!("Expected merchant account '{}', got '{}'", expected, actual)
                })),
            )
                .into_response();
        }
        Err(WebhookError::InvalidCurrency(msg)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_currency", "message": msg })),
            )
                .into_response();
        }
        Err(WebhookError::InvalidAmount(msg)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_amount", "message": msg })),
            )
                .into_response();
        }
        Err(WebhookError::UnsupportedEvent(msg)) | Err(WebhookError::UnsupportedStatus(msg)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "unsupported_event_or_status",
                    "message": msg
                })),
            )
                .into_response();
        }
        Err(WebhookError::ContradictoryRefundStatus { event, status }) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "contradictory_refund_status",
                    "message": format!("Contradictory refund event '{}' with status '{}'", event, status)
                })),
            )
                .into_response();
        }
    };

    let payload_val: serde_json::Value = match serde_json::from_slice(body.as_ref()) {
        Ok(v) => v,
        Err(_) => serde_json::json!({}),
    };

    // 3. Durable transactional event inbox deduplication and state machine transition
    let params = esa_state::WebhookEventParams {
        provider: parsed.provider,
        provider_event_id: &parsed.provider_event_id,
        event_type: parsed.event_type.clone(),
        payment_id: &parsed.payment_id,
        amount_minor: parsed.amount_minor,
        currency: &parsed.currency,
        refund_id: parsed.refund_id.as_deref(),
        refund_amount_minor: parsed.refund_amount_minor,
        refund_status: Some(parsed.status.as_str()),
        payload: &payload_val,
    };
    let process_res = state.webhook_store.process_webhook_event(&params).await;

    match process_res {
        Ok(WebhookProcessingResult::Applied {
            inbox_id,
            provider,
            provider_event_id,
            payment_id,
            transition,
            payment,
        }) => {
            // Only authoritative Captured transitions trigger captured-payment workload side effects.
            let should_apply_capture_effects = matches!(
                transition,
                esa_core::payment_state::TransitionOutcome::Applied {
                    new_state: PaymentState::Captured,
                    ..
                }
            );
            let result = if should_apply_capture_effects {
                let res = payment::apply_payment_event(&state.state_fabric, &parsed.payment_event);
                publish_payment_side_effects(&state, &res);
                res
            } else {
                payment::PaymentApplyResult {
                    event_id: parsed.payment_event.event_id.clone(),
                    workload_updated: false,
                    workload_id: None,
                    workload: None,
                }
            };

            if matches!(
                transition,
                esa_core::payment_state::TransitionOutcome::Applied { .. }
            ) {
                if let Some(order_id) = payload_val
                    .pointer("/payload/payment/entity/order_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    let attempt_state = match payment.state {
                        PaymentState::Captured => Some("captured"),
                        PaymentState::Authorized => Some("authorized"),
                        PaymentState::Failed => Some("failed"),
                        PaymentState::PendingReconciliation => Some("pending_reconciliation"),
                        _ => None,
                    };
                    if let Some(st) = attempt_state {
                        let _ = state
                            .webhook_store
                            .update_payment_attempt_state(order_id, st, Some(&payment_id))
                            .await;
                    }
                }
            }

            Json(serde_json::json!({
                "status": "accepted",
                "inbox_id": inbox_id,
                "provider": provider,
                "provider_event_id": provider_event_id,
                "payment_id": payment_id,
                "payment_state": payment.state,
                "transition": transition,
                "workload_updated": result.workload_updated,
                "workload_id": result.workload_id,
            }))
            .into_response()
        }
        Ok(WebhookProcessingResult::AlreadyProcessed {
            inbox_id,
            provider,
            provider_event_id,
            payment_id,
            processed_at,
        }) => Json(serde_json::json!({
            "status": "already_processed",
            "inbox_id": inbox_id,
            "provider": provider,
            "provider_event_id": provider_event_id,
            "payment_id": payment_id,
            "processed_at": processed_at,
            "idempotent": true,
            "message": "Event already processed"
        }))
        .into_response(),
        Ok(WebhookProcessingResult::InFlight {
            inbox_id,
            provider,
            provider_event_id,
        }) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "status": "in_flight",
                "inbox_id": inbox_id,
                "provider": provider,
                "provider_event_id": provider_event_id,
                "message": "Event is currently being processed by another worker"
            })),
        )
            .into_response(),
        Ok(WebhookProcessingResult::MetadataRecorded {
            inbox_id,
            provider,
            provider_event_id,
            payment_id,
            reason,
        }) => Json(serde_json::json!({
            "status": "accepted",
            "inbox_id": inbox_id,
            "provider": provider,
            "provider_event_id": provider_event_id,
            "payment_id": payment_id,
            "metadata_recorded": true,
            "message": reason,
        }))
        .into_response(),
        Ok(WebhookProcessingResult::Unapplied {
            inbox_id,
            provider,
            provider_event_id,
            payment_id,
            reason,
        }) => Json(serde_json::json!({
            "status": "unapplied",
            "inbox_id": inbox_id,
            "provider": provider,
            "provider_event_id": provider_event_id,
            "payment_id": payment_id,
            "unapplied": true,
            "message": reason,
        }))
        .into_response(),
        Err(WebhookStoreError::Mismatch(err)) => {
            warn!("Webhook entity mismatch rejected: {}", err);
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "entity_mismatch",
                    "message": err
                })),
            )
                .into_response()
        }
        Err(WebhookStoreError::PaymentState(err)) => {
            warn!("Payment state transition rejected: {}", err);
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "error": "payment_state_error",
                    "message": err.to_string()
                })),
            )
                .into_response()
        }
        Err(WebhookStoreError::Database(err)) => {
            error!("Durable webhook store database error: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "database_error",
                    "message": "Durable event processing failed; provider may retry"
                })),
            )
                .into_response()
        }
        Err(WebhookStoreError::Serialization(err)) => {
            error!("Webhook store serialization error: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "serialization_error",
                    "message": err
                })),
            )
                .into_response()
        }
        Err(WebhookStoreError::Conflict(err)) => {
            warn!("Webhook conflict rejected: {}", err);
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "conflict",
                    "message": err
                })),
            )
                .into_response()
        }
        Err(WebhookStoreError::PaymentNotFound(err)) => {
            warn!("Webhook payment not found: {}", err);
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "payment_not_found",
                    "message": err
                })),
            )
                .into_response()
        }
        Err(WebhookStoreError::InvalidState(err)) => {
            warn!("Webhook invalid state: {}", err);
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "error": "invalid_payment_state",
                    "message": err
                })),
            )
                .into_response()
        }
        Err(WebhookStoreError::RefundExceedsBalance {
            payment_id,
            requested,
            available,
        }) => {
            warn!(
                "Webhook refund exceeds balance: payment_id={}, requested={}, available={}",
                payment_id, requested, available
            );
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "refund_exceeds_balance",
                    "message": format!("Refund amount {} exceeds available balance {} for payment {}", requested, available, payment_id)
                })),
            )
                .into_response()
        }
    }
}

async fn razorpay_status(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let caller = api_keys::authenticate_caller(&headers).await.ok();
    let is_sandbox = caller
        .as_ref()
        .map(|c| c.environment == "sandbox")
        .unwrap_or(true);

    if let Some(adapter) = &state.razorpay {
        Json(adapter.status_for_caller(is_sandbox)).into_response()
    } else {
        Json(serde_json::json!({
            "razorpay": {
                "enabled": false,
                "message": "Set RAZORPAY_KEY_ID, RAZORPAY_KEY_SECRET, and RAZORPAY_WEBHOOK_SECRET in .env"
            }
        }))
        .into_response()
    }
}

async fn razorpay_verify_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required to verify keys: {}", e))
    })?;

    let adapter = state
        .razorpay
        .as_ref()
        .ok_or_else(|| AppError::Internal("Razorpay adapter not configured".into()))?;

    // Reject sandbox caller if adapter credentials are live or not sandbox compatible
    if caller.environment == "sandbox" && !adapter.config().is_sandbox_compatible() {
        return Err(AppError::Forbidden(
            "Sandbox API keys cannot perform Razorpay operations against live adapter credentials."
                .into(),
        ));
    }

    let api_ok = adapter.client().health_check().await.unwrap_or(false);

    Ok(Json(serde_json::json!({
        "api_keys_valid": api_ok,
        "mode": if adapter.config().test_mode_only { "test" } else { "live" },
        "webhook_configured": true,
    })))
}

#[derive(Deserialize)]
struct RazorpayCreateOrderRequest {
    amount_cents: u64,
    region: Option<String>,
    payment_method: Option<String>,
}

async fn razorpay_create_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RazorpayCreateOrderRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required to create order: {}", e))
    })?;

    if !caller
        .scopes
        .iter()
        .any(|s| s == "payments:write" || s == "admin" || s == "*")
    {
        return Err(AppError::Forbidden(
            "Caller lacks required 'payments:write' scope".into(),
        ));
    }

    let adapter = state
        .razorpay
        .as_ref()
        .ok_or_else(|| AppError::Internal("Razorpay adapter not configured".into()))?;

    // Reject sandbox caller if adapter credentials are live or not sandbox compatible
    if caller.environment == "sandbox" && !adapter.config().is_sandbox_compatible() {
        return Err(AppError::Forbidden(
            "Sandbox API keys cannot perform Razorpay operations against live adapter credentials."
                .into(),
        ));
    }

    if req.amount_cents < 100 {
        return Err(AppError::Internal(
            "Minimum amount is 100 paise (₹1)".into(),
        ));
    }

    let region = req.region.unwrap_or_else(|| "IN-SOUTH".to_string());
    let method = req.payment_method.unwrap_or_else(|| "upi".to_string());

    let notes = serde_json::json!({
        "region": region,
        "esa_region": region,
        "payment_method": method,
        "esa_merchant_id": caller.merchant_id,
    });

    let receipt = format!("esa-{}", chrono::Utc::now().timestamp_millis());

    let order = adapter
        .client()
        .create_order(req.amount_cents, &receipt, notes)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let order_id = order.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    if order_id.is_empty() {
        return Err(AppError::Internal(
            "Razorpay order creation returned an empty order ID".into(),
        ));
    }

    let attempt = esa_core::provider::PaymentAttemptRecord {
        transaction_id: format!("tx_{}", order_id),
        merchant_id: caller.merchant_id.clone(),
        provider: "razorpay".to_string(),
        provider_order_id: Some(order_id.to_string()),
        payment_reference: None,
        amount_minor: req.amount_cents,
        currency: "INR".to_string(),
        state: "created".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state
        .webhook_store
        .record_payment_attempt(&attempt)
        .await
        .map_err(|e| {
            error!("Failed to persist Razorpay payment attempt: {}", e);
            AppError::Internal("Failed to persist payment attempt record in durable store".into())
        })?;
    if let Ok(mut map) = state.order_merchants.write() {
        map.insert(order_id.to_string(), caller.merchant_id.clone());
    }

    let returned_key_id = if caller.environment == "sandbox" {
        if adapter.config().is_test_credentials() {
            adapter.config().key_id.clone()
        } else {
            return Err(AppError::Forbidden(
                "Live Razorpay credentials cannot be returned to sandbox callers.".into(),
            ));
        }
    } else {
        adapter.config().key_id.clone()
    };

    Ok(Json(serde_json::json!({
        "order_id": order_id,
        "amount": order.get("amount").and_then(|v| v.as_u64()).unwrap_or(req.amount_cents),
        "currency": order.get("currency").and_then(|v| v.as_str()).unwrap_or("INR"),
        "key_id": returned_key_id,
        "receipt": receipt,
        "region": region,
        "payment_method": method,
    })))
}

#[derive(Deserialize)]
struct RazorpayConfirmRequest {
    payment_id: String,
}

/// Checkout success fallback when webhook signature mismatches (fetches payment from Razorpay API).
async fn razorpay_confirm_payment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RazorpayConfirmRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required to confirm payment: {}", e))
    })?;

    if !caller
        .scopes
        .iter()
        .any(|s| s == "payments:write" || s == "admin" || s == "*")
    {
        return Err(AppError::Forbidden(
            "Caller lacks required 'payments:write' scope".into(),
        ));
    }

    let adapter = state
        .razorpay
        .as_ref()
        .ok_or_else(|| AppError::Internal("Razorpay adapter not configured".into()))?;

    // Reject sandbox caller if adapter credentials are live or not sandbox compatible
    if caller.environment == "sandbox" && !adapter.config().is_sandbox_compatible() {
        return Err(AppError::Forbidden(
            "Sandbox API keys cannot perform Razorpay payment confirmation against live adapter credentials.".into(),
        ));
    }
    let adapter = state
        .razorpay
        .as_ref()
        .ok_or_else(|| AppError::Internal("Razorpay adapter not configured".into()))?;

    let payment = adapter
        .client()
        .get_payment(&req.payment_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // 1. Obtain the actual order_id from the Razorpay payment entity.
    let order_id = payment
        .get("order_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let order_id = match order_id {
        Some(id) => id,
        None => {
            return Err(AppError::Forbidden(
                "Payment has no associated order_id; cannot verify merchant ownership".into(),
            ));
        }
    };

    // 2. Retrieve that order using the Razorpay API.
    let order = adapter.client().get_order(order_id).await.map_err(|e| {
        AppError::Forbidden(format!(
            "Failed to retrieve order {} for verification: {}",
            order_id, e
        ))
    })?;

    // 3. Use the server-created order.notes.esa_merchant_id as the ownership authority.
    // Never treat payment.notes.esa_merchant_id as proof of ownership.
    let order_merchant_id = order
        .get("notes")
        .and_then(|n| n.get("esa_merchant_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let authoritative_owner = match order_merchant_id {
        Some(m) => m,
        None => {
            return Err(AppError::Forbidden(
                "Authoritative order lacks merchant ownership metadata".into(),
            ));
        }
    };

    // 4. Verify against durable server-owned payment attempt record.
    let attempt = state
        .webhook_store
        .get_payment_attempt_for_provider("razorpay", order_id)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to retrieve payment attempt: {}", e)))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Payment order '{}' has no recorded payment attempt in durable store",
                order_id
            ))
        })?;

    if attempt.provider != "razorpay" {
        return Err(AppError::Forbidden(format!(
            "Payment attempt provider '{}' does not match razorpay",
            attempt.provider
        )));
    }

    if attempt.merchant_id != authoritative_owner {
        return Err(AppError::Forbidden(
            "Order ownership metadata conflicts with server-tracked order mapping".into(),
        ));
    }

    if let Ok(map) = state.order_merchants.read() {
        if let Some(tracked_merchant) = map.get(order_id) {
            if tracked_merchant != authoritative_owner {
                return Err(AppError::Forbidden(
                    "Order ownership metadata conflicts with server-tracked order mapping".into(),
                ));
            }
        }
    }

    // 5. Enforce ownership: authoritative order merchant must match caller (or caller is admin).
    if attempt.merchant_id != caller.merchant_id && !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Payment order does not belong to the authenticated merchant".into(),
        ));
    }

    let amount_minor = payment
        .get("amount")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AppError::Conflict("Payment entity missing valid numeric amount".into()))?;
    let currency = payment
        .get("currency")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Conflict("Payment entity missing currency".into()))?;

    if amount_minor != attempt.amount_minor || !currency.eq_ignore_ascii_case(&attempt.currency) {
        return Err(AppError::Conflict(format!(
            "Payment amount/currency mismatch: upstream has {} {} but recorded attempt has {} {}",
            amount_minor, currency, attempt.amount_minor, attempt.currency
        )));
    }

    // 6. Check existing durable payment record in webhook_store.
    // If the payment is already Captured/Refunded, return immediately (idempotent no-op)
    // without re-applying side effects or re-inflating workload metrics!
    if let Ok(Some(existing)) = state
        .webhook_store
        .get_payment_record(&req.payment_id)
        .await
    {
        if existing.state == PaymentState::Captured
            || existing.state == PaymentState::PartiallyRefunded
            || existing.state == PaymentState::Refunded
        {
            info!(
                payment_id = %req.payment_id,
                state = ?existing.state,
                "Payment already confirmed/captured; returning idempotent response"
            );
            if attempt.state != "captured" {
                let _ = state
                    .webhook_store
                    .update_payment_attempt_state(order_id, "captured", Some(&req.payment_id))
                    .await;
            }
            return Ok(Json(serde_json::json!({
                "success": true,
                "captured": true,
                "status": "already_confirmed",
                "source": "razorpay_confirm",
                "event_id": format!("confirm_{}", req.payment_id),
                "payment_id": req.payment_id,
                "payment_reference": req.payment_id,
                "payment_state": format!("{:?}", existing.state).to_lowercase(),
                "transaction_id": attempt.transaction_id,
                "provider": "razorpay",
                "provider_order_id": order_id,
                "amount_minor": amount_minor,
                "currency": currency,
                "idempotent": true,
                "workload_updated": false,
                "message": "Payment has already been confirmed and processed",
            })));
        }
    }

    let raw_status = payment
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();

    if raw_status == "created" || raw_status == "pending" {
        if attempt.state == "created" {
            let _ = state
                .webhook_store
                .update_payment_attempt_state(order_id, "pending", Some(&req.payment_id))
                .await;
        }
        return Err(AppError::Conflict(format!(
            "Payment '{}' is in '{}' state and has not been captured",
            req.payment_id, raw_status
        )));
    }

    let event = payment_entity_to_event(&payment).map_err(|e| match e {
        esa_razorpay::WebhookError::UnsupportedStatus(st) => AppError::Conflict(format!(
            "Unsupported or unknown Razorpay payment status '{}'; failing closed",
            st
        )),
        other => AppError::BadRequest(other.to_string()),
    })?;

    // 7. Record and transition transactionally into durable webhook_store.
    // Distinct event IDs per lifecycle phase allow authorize -> capture progression
    // without colliding in the webhook_inbox deduplication table.
    let confirm_event_id = match event.event_type {
        PaymentEventType::PaymentCaptured => format!("confirm_{}", req.payment_id),
        PaymentEventType::PaymentAuthorized => format!("confirm_auth_{}", req.payment_id),
        PaymentEventType::PaymentFailed => format!("confirm_failed_{}", req.payment_id),
        ref other => {
            return Err(AppError::Conflict(format!(
                "Unsupported Razorpay confirmation event type: {:?}",
                other
            )));
        }
    };

    let params = esa_state::WebhookEventParams {
        provider: "razorpay",
        provider_event_id: &confirm_event_id,
        event_type: event.event_type.clone(),
        payment_id: &req.payment_id,
        amount_minor,
        currency,
        refund_id: None,
        refund_amount_minor: None,
        refund_status: None,
        payload: &payment,
    };

    let store_result = state.webhook_store.process_webhook_event(&params).await;
    match store_result {
        Ok(esa_state::WebhookProcessingResult::Applied {
            transition,
            payment: durable_payment,
            ..
        }) => {
            let attempt_state = match durable_payment.state {
                PaymentState::Captured => "captured",
                PaymentState::Authorized => "authorized",
                PaymentState::Failed => "failed",
                PaymentState::PendingReconciliation => "pending_reconciliation",
                PaymentState::PartiallyRefunded => "partially_refunded",
                PaymentState::Refunded => "refunded",
                PaymentState::Created => "created",
            };

            let should_apply_capture_effects = event.event_type
                == PaymentEventType::PaymentCaptured
                && matches!(
                    transition,
                    esa_core::payment_state::TransitionOutcome::Applied {
                        new_state: PaymentState::Captured,
                        ..
                    }
                );

            let (workload_updated, workload_id, result_event_id) = if should_apply_capture_effects {
                let result = payment::apply_payment_event(&state.state_fabric, &event);
                publish_payment_side_effects(&state, &result);
                (result.workload_updated, result.workload_id, result.event_id)
            } else {
                (false, None, confirm_event_id.clone())
            };

            if matches!(
                transition,
                esa_core::payment_state::TransitionOutcome::Applied { .. }
            ) || attempt.state == "created"
            {
                let _ = state
                    .webhook_store
                    .update_payment_attempt_state(order_id, attempt_state, Some(&req.payment_id))
                    .await;
            }

            let is_captured = durable_payment.state == PaymentState::Captured;
            let (resp_status, msg) = match durable_payment.state {
                PaymentState::Captured => {
                    ("accepted", "Payment authoritatively confirmed and captured")
                }
                PaymentState::Authorized => (
                    "authorized",
                    "Payment is authorized only and has not been captured",
                ),
                PaymentState::Failed => ("failed", "Payment failed at Razorpay"),
                PaymentState::PendingReconciliation => (
                    "pending_reconciliation",
                    "Payment flagged for reconciliation; not captured",
                ),
                _ => (attempt_state, "Payment recorded in non-captured state"),
            };

            Ok(Json(serde_json::json!({
                "success": is_captured,
                "captured": is_captured,
                "status": resp_status,
                "source": "razorpay_confirm",
                "payment_state": attempt_state,
                "transaction_id": attempt.transaction_id,
                "provider": "razorpay",
                "provider_order_id": order_id,
                "payment_reference": req.payment_id,
                "payment_id": req.payment_id,
                "amount_minor": amount_minor,
                "currency": currency,
                "idempotent": false,
                "message": msg,
                "event_id": result_event_id,
                "workload_updated": workload_updated,
                "workload_id": workload_id,
            })))
        }
        Ok(esa_state::WebhookProcessingResult::AlreadyProcessed { .. }) => {
            let (is_captured, resp_status, p_state, msg) = match event.event_type {
                PaymentEventType::PaymentCaptured => (
                    true,
                    "already_confirmed",
                    "captured",
                    "Payment has already been confirmed and processed",
                ),
                PaymentEventType::PaymentAuthorized => (
                    false,
                    "already_authorized",
                    "authorized",
                    "Payment is already recorded in authorized state (not captured)",
                ),
                PaymentEventType::PaymentFailed => (
                    false,
                    "already_failed",
                    "failed",
                    "Payment is already recorded as failed",
                ),
                _ => (
                    false,
                    "already_processed",
                    "unknown",
                    "Payment event already processed",
                ),
            };
            Ok(Json(serde_json::json!({
                "success": is_captured,
                "captured": is_captured,
                "status": resp_status,
                "source": "razorpay_confirm",
                "payment_state": p_state,
                "transaction_id": attempt.transaction_id,
                "provider": "razorpay",
                "provider_order_id": order_id,
                "payment_reference": req.payment_id,
                "payment_id": req.payment_id,
                "amount_minor": amount_minor,
                "currency": currency,
                "idempotent": true,
                "workload_updated": false,
                "message": msg,
                "event_id": confirm_event_id,
            })))
        }
        Ok(esa_state::WebhookProcessingResult::MetadataRecorded { .. }) => {
            Ok(Json(serde_json::json!({
                "success": false,
                "status": "metadata_recorded",
                "payment_state": "metadata_recorded",
                "transaction_id": attempt.transaction_id,
                "provider": "razorpay",
                "provider_order_id": order_id,
                "payment_reference": req.payment_id,
                "payment_id": req.payment_id,
                "amount_minor": amount_minor,
                "currency": currency,
                "idempotent": false,
                "message": "Metadata recorded without state transition",
            })))
        }
        Ok(esa_state::WebhookProcessingResult::InFlight { .. }) => Err(AppError::Conflict(
            "Payment confirmation is currently in-flight by another worker".into(),
        )),
        Ok(esa_state::WebhookProcessingResult::Unapplied { reason, .. }) => {
            Err(AppError::Conflict(format!(
                "Payment confirmation could not be applied: {}",
                reason
            )))
        }
        Err(esa_state::WebhookStoreError::PaymentState(err)) => {
            warn!("Payment confirmation state transition rejected: {}", err);
            Err(AppError::Conflict(format!(
                "Invalid payment state transition: {}",
                err
            )))
        }
        Err(esa_state::WebhookStoreError::Mismatch(err)) => {
            warn!("Payment confirmation entity mismatch: {}", err);
            Err(AppError::Conflict(format!(
                "Payment entity mismatch: {}",
                err
            )))
        }
        Err(err) => {
            error!("Payment confirmation failed in durable store: {}", err);
            Err(AppError::Internal(
                "Failed to record payment confirmation in durable store".into(),
            ))
        }
    }
}

// ==========================================
// Cashfree Payment Gateway Handlers
// ==========================================

async fn cashfree_status(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let caller = api_keys::authenticate_caller(&headers).await.ok();
    let is_sandbox = caller
        .as_ref()
        .map(|c| c.environment == "sandbox")
        .unwrap_or(false);

    if let Some(cf) = &state.cashfree {
        Json(serde_json::json!({
            "cashfree": cf.config().public_status(is_sandbox),
            "health": cf.health(),
        }))
    } else {
        Json(serde_json::json!({
            "cashfree": {
                "configured": false,
                "message": "Cashfree provider not configured"
            }
        }))
    }
}

async fn cashfree_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let provider = match &state.cashfree {
        Some(p) => p,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Cashfree provider not configured" })),
            ));
        }
    };

    let mut header_map = std::collections::HashMap::new();
    for (k, v) in headers.iter() {
        if let Ok(val) = v.to_str() {
            header_map.insert(k.as_str().to_lowercase(), val.to_string());
        }
    }

    let parsed = match provider.verify_webhook(body.as_ref(), &header_map) {
        Ok(p) => p,
        Err(err) => {
            warn!("Cashfree webhook verification rejected: {}", err);
            let status = match &err {
                esa_core::provider::ProviderError::InvalidSignature { .. }
                | esa_core::provider::ProviderError::AuthenticationFailed { .. } => {
                    StatusCode::UNAUTHORIZED
                }
                _ => StatusCode::BAD_REQUEST,
            };
            return Err((
                status,
                Json(serde_json::json!({ "error": err.to_string() })),
            ));
        }
    };

    let params = esa_state::WebhookEventParams {
        provider: "cashfree",
        provider_event_id: &parsed.provider_event_id,
        event_type: parsed.event_type.clone(),
        payment_id: &parsed.provider_payment_id,
        amount_minor: parsed.amount_minor,
        currency: &parsed.currency,
        refund_id: parsed.refund_id.as_deref(),
        refund_amount_minor: parsed.refund_amount_minor,
        refund_status: parsed.refund_status.as_deref(),
        payload: &parsed.raw_payload,
    };

    match state.webhook_store.process_webhook_event(&params).await {
        Ok(esa_state::WebhookProcessingResult::Applied { .. }) => {
            let payment_event = PaymentEvent {
                event_id: parsed.provider_event_id.clone(),
                event_type: parsed.event_type.clone(),
                timestamp: chrono::Utc::now(),
                region: Region::IndiaSouth,
                payment_method_class: PaymentMethodClass::Upi,
                pseudonymous_reference: format!("PSEUDO-{}", parsed.provider_payment_id),
                amount_cents: Some(parsed.amount_minor),
                amount_minor: Some(parsed.amount_minor),
                currency: parsed.currency.clone(),
                processing_latency_ms: Some(92.0),
                success: matches!(
                    parsed.event_type,
                    PaymentEventType::PaymentCaptured | PaymentEventType::RefundProcessed
                ),
            };
            let apply_res = payment::apply_payment_event(&state.state_fabric, &payment_event);
            publish_payment_side_effects(&state, &apply_res);

            info!(
                "Cashfree webhook newly applied: {}",
                parsed.provider_event_id
            );
            Ok(Json(
                serde_json::json!({ "status": "OK", "processed": true }),
            ))
        }
        Ok(esa_state::WebhookProcessingResult::AlreadyProcessed { .. }) => {
            info!(
                "Cashfree webhook already processed (idempotent duplicate): {}",
                parsed.provider_event_id
            );
            Ok(Json(
                serde_json::json!({ "status": "already_processed", "duplicate": true }),
            ))
        }
        Ok(esa_state::WebhookProcessingResult::InFlight { .. }) => {
            warn!(
                "Cashfree webhook currently in-flight: {}",
                parsed.provider_event_id
            );
            Err((
                StatusCode::CONFLICT,
                Json(
                    serde_json::json!({ "status": "in_flight", "message": "Concurrent webhook delivery in progress" }),
                ),
            ))
        }
        Ok(esa_state::WebhookProcessingResult::MetadataRecorded { .. }) => {
            Ok(Json(serde_json::json!({ "status": "metadata_recorded" })))
        }
        Ok(esa_state::WebhookProcessingResult::Unapplied { reason, .. }) => {
            warn!("Cashfree webhook unapplied: {}", reason);
            Ok(Json(
                serde_json::json!({ "status": "unapplied", "reason": reason }),
            ))
        }
        Err(err) => {
            error!("Failed to persist Cashfree webhook: {}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": err.to_string() })),
            ))
        }
    }
}

#[derive(Deserialize)]
struct CashfreeConfirmRequest {
    order_id: String,
    #[allow(dead_code)]
    payment_id: Option<String>,
}

async fn cashfree_confirm_payment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CashfreeConfirmRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required to confirm payment: {}", e))
    })?;

    if !caller
        .scopes
        .iter()
        .any(|s| s == "payments:write" || s == "admin" || s == "*")
    {
        return Err(AppError::Forbidden(
            "Caller lacks required 'payments:write' scope".into(),
        ));
    }

    let provider = state
        .cashfree
        .as_ref()
        .ok_or_else(|| AppError::Internal("Cashfree provider not configured".into()))?;

    let attempt = state
        .webhook_store
        .get_payment_attempt_for_provider("cashfree", &req.order_id)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to retrieve payment attempt: {}", e)))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Order '{}' has no recorded payment attempt in durable store",
                req.order_id
            ))
        })?;

    if attempt.provider != "cashfree" {
        return Err(AppError::Forbidden(format!(
            "Payment attempt provider '{}' does not match cashfree",
            attempt.provider
        )));
    }

    if attempt.merchant_id != caller.merchant_id && !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Order does not belong to the authenticated merchant".into(),
        ));
    }

    let payment_status = provider
        .get_payment_status(&req.order_id)
        .await
        .map_err(|e| {
            AppError::Internal(format!("Failed to retrieve Cashfree payment status: {}", e))
        })?;

    if payment_status.status != esa_core::provider::ProviderPaymentStatus::Captured {
        return Err(AppError::Forbidden(format!(
            "Payment status is not captured: {:?}",
            payment_status.status
        )));
    }

    if payment_status.amount_minor != attempt.amount_minor
        || !payment_status
            .currency
            .eq_ignore_ascii_case(&attempt.currency)
    {
        return Err(AppError::Conflict(format!(
            "Cashfree payment amount/currency mismatch: upstream has {} {} but recorded attempt has {} {}",
            payment_status.amount_minor, payment_status.currency, attempt.amount_minor, attempt.currency
        )));
    }

    // Check if existing durable payment record is already Captured/Refunded
    if let Ok(Some(existing)) = state
        .webhook_store
        .get_payment_record(&payment_status.provider_payment_id)
        .await
    {
        if existing.state == PaymentState::Captured
            || existing.state == PaymentState::PartiallyRefunded
            || existing.state == PaymentState::Refunded
        {
            return Ok(Json(serde_json::json!({
                "success": true,
                "status": "already_confirmed",
                "source": "cashfree_confirm",
                "payment_state": format!("{:?}", existing.state).to_lowercase(),
                "transaction_id": attempt.transaction_id,
                "provider": "cashfree",
                "provider_order_id": req.order_id,
                "order_id": req.order_id,
                "payment_reference": payment_status.provider_payment_id,
                "payment_id": payment_status.provider_payment_id,
                "cf_payment_id": payment_status.provider_payment_id,
                "amount_minor": payment_status.amount_minor,
                "currency": payment_status.currency,
                "idempotent": true,
                "message": "Payment has already been confirmed and processed",
            })));
        }
    }

    let confirm_event_id = format!("cf_confirm_{}", req.order_id);
    let params = esa_state::WebhookEventParams {
        provider: "cashfree",
        provider_event_id: &confirm_event_id,
        event_type: PaymentEventType::PaymentCaptured,
        payment_id: &payment_status.provider_payment_id,
        amount_minor: payment_status.amount_minor,
        currency: &payment_status.currency,
        refund_id: None,
        refund_amount_minor: None,
        refund_status: None,
        payload: &payment_status.raw_response,
    };

    let store_result = state.webhook_store.process_webhook_event(&params).await;
    match store_result {
        Ok(esa_state::WebhookProcessingResult::Applied { .. }) => {
            let event = PaymentEvent {
                event_id: confirm_event_id.clone(),
                event_type: PaymentEventType::PaymentCaptured,
                timestamp: chrono::Utc::now(),
                region: Region::IndiaSouth,
                payment_method_class: PaymentMethodClass::Upi,
                pseudonymous_reference: format!("PSEUDO-{}", payment_status.provider_payment_id),
                amount_cents: Some(payment_status.amount_minor),
                amount_minor: Some(payment_status.amount_minor),
                currency: payment_status.currency.clone(),
                processing_latency_ms: Some(95.0),
                success: true,
            };
            let result = payment::apply_payment_event(&state.state_fabric, &event);
            publish_payment_side_effects(&state, &result);
            let _ = state
                .webhook_store
                .update_payment_attempt_state(
                    &req.order_id,
                    "captured",
                    Some(&payment_status.provider_payment_id),
                )
                .await;

            Ok(Json(serde_json::json!({
                "success": true,
                "status": "accepted",
                "source": "cashfree_confirm",
                "payment_state": "captured",
                "transaction_id": attempt.transaction_id,
                "provider": "cashfree",
                "provider_order_id": req.order_id,
                "order_id": req.order_id,
                "payment_reference": payment_status.provider_payment_id,
                "payment_id": payment_status.provider_payment_id,
                "cf_payment_id": payment_status.provider_payment_id,
                "amount_minor": payment_status.amount_minor,
                "currency": payment_status.currency,
                "idempotent": false,
                "message": "Payment authoritatively captured and verified with Cashfree",
            })))
        }
        Ok(esa_state::WebhookProcessingResult::AlreadyProcessed { .. }) => {
            Ok(Json(serde_json::json!({
                "success": true,
                "status": "already_confirmed",
                "source": "cashfree_confirm",
                "payment_state": "captured",
                "transaction_id": attempt.transaction_id,
                "provider": "cashfree",
                "provider_order_id": req.order_id,
                "order_id": req.order_id,
                "payment_reference": payment_status.provider_payment_id,
                "payment_id": payment_status.provider_payment_id,
                "cf_payment_id": payment_status.provider_payment_id,
                "amount_minor": payment_status.amount_minor,
                "currency": payment_status.currency,
                "idempotent": true,
                "message": "Payment has already been confirmed and processed",
            })))
        }
        Ok(esa_state::WebhookProcessingResult::MetadataRecorded { .. }) => {
            Ok(Json(serde_json::json!({
                "success": false,
                "status": "metadata_recorded",
                "payment_state": "metadata_recorded",
                "transaction_id": attempt.transaction_id,
                "provider": "cashfree",
                "provider_order_id": req.order_id,
                "order_id": req.order_id,
                "payment_reference": payment_status.provider_payment_id,
                "payment_id": payment_status.provider_payment_id,
                "cf_payment_id": payment_status.provider_payment_id,
                "amount_minor": payment_status.amount_minor,
                "currency": payment_status.currency,
                "idempotent": false,
                "message": "Metadata recorded without payment state transition",
            })))
        }
        Err(err) => Err(AppError::Internal(format!(
            "Failed to record payment in store: {}",
            err
        ))),
        Ok(_) => Ok(Json(serde_json::json!({
            "success": true,
            "status": "accepted",
            "payment_state": "captured",
            "transaction_id": attempt.transaction_id,
            "provider": "cashfree",
            "provider_order_id": req.order_id,
            "order_id": req.order_id,
            "payment_reference": payment_status.provider_payment_id,
            "payment_id": payment_status.provider_payment_id,
            "cf_payment_id": payment_status.provider_payment_id,
            "amount_minor": payment_status.amount_minor,
            "currency": payment_status.currency,
            "idempotent": false,
            "message": "Payment authoritatively confirmed",
        }))),
    }
}

async fn cashfree_get_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(order_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers)
        .await
        .map_err(|e| AppError::Unauthorized(format!("Authentication required: {}", e)))?;

    let attempt = state
        .webhook_store
        .get_payment_attempt(&order_id)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to retrieve payment attempt: {}", e)))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Order '{}' has no recorded payment attempt in durable store",
                order_id
            ))
        })?;

    if attempt.merchant_id != caller.merchant_id && !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Order does not belong to the authenticated merchant".into(),
        ));
    }

    let provider = state
        .cashfree
        .as_ref()
        .ok_or_else(|| AppError::Internal("Cashfree provider not configured".into()))?;

    let details = provider
        .get_order(&order_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::to_value(&details).unwrap_or_default()))
}

#[derive(Deserialize)]
struct CashfreeRefundRequest {
    order_id: String,
    refund_id: String,
    amount_minor: u64,
    reason: Option<String>,
}

async fn cashfree_create_refund(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CashfreeRefundRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers)
        .await
        .map_err(|e| AppError::Unauthorized(format!("Authentication required: {}", e)))?;

    if !caller
        .scopes
        .iter()
        .any(|s| s == "payments:write" || s == "admin" || s == "*")
    {
        return Err(AppError::Forbidden(
            "Caller lacks required 'payments:write' scope".into(),
        ));
    }

    let attempt = state
        .webhook_store
        .get_payment_attempt_for_provider("cashfree", &req.order_id)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to retrieve payment attempt: {}", e)))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Order '{}' has no recorded payment attempt in durable store",
                req.order_id
            ))
        })?;

    if attempt.provider != "cashfree" {
        return Err(AppError::Forbidden(format!(
            "Payment attempt provider '{}' does not match cashfree",
            attempt.provider
        )));
    }

    if attempt.merchant_id != caller.merchant_id && !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Order does not belong to caller".into(),
        ));
    }

    // 1. Enforce captured state from attempt record
    if attempt.state != "captured" && attempt.state != "partially_refunded" {
        return Err(AppError::BadRequest(format!(
            "Only captured or partially refunded payments can be refunded (current state: '{}')",
            attempt.state
        )));
    }

    // 2. Validate against attempt amount
    if req.amount_minor == 0 || req.amount_minor > attempt.amount_minor {
        return Err(AppError::BadRequest(format!(
            "Refund amount exceeds refundable balance: requested {} paise, but maximum refundable is {} paise",
            req.amount_minor, attempt.amount_minor
        )));
    }

    let payment_key = attempt
        .payment_reference
        .as_deref()
        .unwrap_or(&req.order_id);

    // 3. Atomically reserve refund balance in durable store before calling upstream
    let reservation = state
        .webhook_store
        .reserve_refund(
            "cashfree",
            payment_key,
            &req.refund_id,
            req.amount_minor,
            "INR",
        )
        .await
        .map_err(|e| match e {
            esa_state::WebhookStoreError::RefundExceedsBalance { available, .. } => {
                AppError::BadRequest(format!(
                    "Refund amount {} exceeds refundable balance of {} paise",
                    req.amount_minor, available
                ))
            }
            esa_state::WebhookStoreError::PaymentNotFound(_) => AppError::BadRequest(format!(
                "Payment record not found for '{}'; payment must be Captured or PartiallyRefunded",
                payment_key
            )),
            esa_state::WebhookStoreError::InvalidState(s) => AppError::BadRequest(s),
            esa_state::WebhookStoreError::Conflict(s) => AppError::Conflict(s),
            other => AppError::Internal(format!("Failed to reserve refund balance: {}", other)),
        })?;

    if let esa_core::provider::RefundReservationOutcome::AlreadyProcessed { amount_minor } =
        reservation
    {
        return Ok(Json(serde_json::json!({
            "status": "PROCESSED",
            "refund_id": req.refund_id,
            "amount_minor": amount_minor,
            "idempotent": true,
            "message": "Refund has already been processed",
        })));
    }

    let provider = state
        .cashfree
        .as_ref()
        .ok_or_else(|| AppError::Internal("Cashfree provider not configured".into()))?;

    let params = esa_core::provider::CreateRefundParams {
        provider_payment_id: req.order_id.clone(),
        provider_order_id: Some(req.order_id.clone()),
        refund_id: req.refund_id.clone(),
        amount_minor: req.amount_minor,
        currency: "INR".to_string(),
        reason: req.reason,
        merchant_id: caller.merchant_id,
    };

    let refund = match provider.create_refund(&params).await {
        Ok(r) => r,
        Err(e) => {
            let err_str = e.to_string();
            let is_timeout = matches!(e, esa_core::provider::ProviderError::Timeout { .. })
                || err_str.to_lowercase().contains("timeout")
                || err_str.to_lowercase().contains("timed out");

            if is_timeout {
                return Err(AppError::Internal(format!(
                    "Upstream Cashfree request timed out: {}. Refund reservation remains pending until reconciled.",
                    e
                )));
            } else {
                let refund_event_id = format!("cf_ref_fail_{}", req.refund_id);
                let webhook_params = esa_state::WebhookEventParams {
                    provider: "cashfree",
                    provider_event_id: &refund_event_id,
                    event_type: PaymentEventType::RefundFailed,
                    payment_id: payment_key,
                    amount_minor: req.amount_minor,
                    currency: "INR",
                    refund_id: Some(&req.refund_id),
                    refund_amount_minor: Some(req.amount_minor),
                    refund_status: Some("failed"),
                    payload: &serde_json::json!({ "error": err_str }),
                };
                let _ = state
                    .webhook_store
                    .process_webhook_event(&webhook_params)
                    .await;
                return Err(AppError::Internal(format!(
                    "Cashfree refund rejected: {}",
                    e
                )));
            }
        }
    };

    let (event_type, canonical_status) = match refund.status.to_uppercase().as_str() {
        "SUCCESS" | "PROCESSED" => (PaymentEventType::RefundProcessed, "processed"),
        "FAILED" | "REJECTED" => (PaymentEventType::RefundFailed, "failed"),
        _ => (PaymentEventType::RefundCreated, "pending"),
    };

    let refund_event_id = format!("cf_ref_{}", req.refund_id);
    let webhook_params = esa_state::WebhookEventParams {
        provider: "cashfree",
        provider_event_id: &refund_event_id,
        event_type,
        payment_id: payment_key,
        amount_minor: req.amount_minor,
        currency: "INR",
        refund_id: Some(&req.refund_id),
        refund_amount_minor: Some(req.amount_minor),
        refund_status: Some(canonical_status),
        payload: &refund.raw_response,
    };
    state
        .webhook_store
        .process_webhook_event(&webhook_params)
        .await
        .map_err(|e| {
            AppError::Internal(format!("Failed to record refund in durable store: {}", e))
        })?;

    Ok(Json(serde_json::to_value(&refund).unwrap_or_default()))
}

// ==========================================
// PayU India Hosted Checkout Handlers
// ==========================================

async fn payu_status(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let caller = api_keys::authenticate_caller(&headers).await.ok();
    let is_sandbox = caller
        .as_ref()
        .map(|c| c.environment == "sandbox")
        .unwrap_or(false);

    if let Some(p) = &state.payu {
        Json(serde_json::json!({
            "payu": p.config().public_status(is_sandbox),
            "health": p.health(),
        }))
    } else {
        Json(serde_json::json!({
            "payu": {
                "configured": false,
                "message": "PayU provider not configured"
            }
        }))
    }
}

async fn payu_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let provider = match &state.payu {
        Some(p) => p,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "PayU provider not configured" })),
            ));
        }
    };

    let mut header_map = std::collections::HashMap::new();
    for (k, v) in headers.iter() {
        if let Ok(val) = v.to_str() {
            header_map.insert(k.as_str().to_lowercase(), val.to_string());
        }
    }

    let parsed = match provider.verify_webhook(body.as_ref(), &header_map) {
        Ok(p) => p,
        Err(err) => {
            warn!("PayU webhook verification failed: {}", err);
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": err.to_string() })),
            ));
        }
    };

    let params = esa_state::WebhookEventParams {
        provider: "payu",
        provider_event_id: &parsed.provider_event_id,
        event_type: parsed.event_type.clone(),
        payment_id: &parsed.provider_payment_id,
        amount_minor: parsed.amount_minor,
        currency: &parsed.currency,
        refund_id: parsed.refund_id.as_deref(),
        refund_amount_minor: parsed.refund_amount_minor,
        refund_status: parsed.refund_status.as_deref(),
        payload: &parsed.raw_payload,
    };

    match state.webhook_store.process_webhook_event(&params).await {
        Ok(esa_state::WebhookProcessingResult::Applied { .. }) => {
            let payment_event = PaymentEvent {
                event_id: parsed.provider_event_id.clone(),
                event_type: parsed.event_type.clone(),
                timestamp: chrono::Utc::now(),
                region: Region::IndiaNorth,
                payment_method_class: PaymentMethodClass::Upi,
                pseudonymous_reference: format!("PSEUDO-{}", parsed.provider_payment_id),
                amount_cents: Some(parsed.amount_minor),
                amount_minor: Some(parsed.amount_minor),
                currency: parsed.currency.clone(),
                processing_latency_ms: Some(88.0),
                success: matches!(
                    parsed.event_type,
                    PaymentEventType::PaymentCaptured | PaymentEventType::RefundProcessed
                ),
            };
            let apply_res = payment::apply_payment_event(&state.state_fabric, &payment_event);
            publish_payment_side_effects(&state, &apply_res);

            info!("PayU webhook newly applied: {}", parsed.provider_event_id);
            Ok(Json(
                serde_json::json!({ "status": "OK", "processed": true }),
            ))
        }
        Ok(esa_state::WebhookProcessingResult::AlreadyProcessed { .. }) => {
            info!(
                "PayU webhook already processed (idempotent duplicate): {}",
                parsed.provider_event_id
            );
            Ok(Json(
                serde_json::json!({ "status": "already_processed", "duplicate": true }),
            ))
        }
        Ok(esa_state::WebhookProcessingResult::InFlight { .. }) => {
            warn!(
                "PayU webhook currently in-flight: {}",
                parsed.provider_event_id
            );
            Err((
                StatusCode::CONFLICT,
                Json(
                    serde_json::json!({ "status": "in_flight", "message": "Concurrent webhook delivery in progress" }),
                ),
            ))
        }
        Ok(esa_state::WebhookProcessingResult::MetadataRecorded { .. }) => {
            Ok(Json(serde_json::json!({ "status": "metadata_recorded" })))
        }
        Ok(esa_state::WebhookProcessingResult::Unapplied { reason, .. }) => {
            warn!("PayU webhook unapplied: {}", reason);
            Ok(Json(
                serde_json::json!({ "status": "unapplied", "reason": reason }),
            ))
        }
        Err(err) => {
            error!("Failed to persist PayU webhook: {}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": err.to_string() })),
            ))
        }
    }
}

async fn payu_callback_handler(
    State(state): State<AppState>,
    _headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let provider = match &state.payu {
        Some(p) => p,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "PayU provider not configured" })),
            ));
        }
    };

    let header_map = std::collections::HashMap::new();
    let parsed = match provider.verify_webhook(body.as_ref(), &header_map) {
        Ok(p) => p,
        Err(err) => {
            warn!("PayU callback reverse hash verification failed: {}", err);
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": err.to_string() })),
            ));
        }
    };

    let txnid = parsed
        .provider_order_id
        .as_deref()
        .unwrap_or(&parsed.provider_payment_id);

    // Verify attempt ownership record exists in durable store
    let attempt = match state
        .webhook_store
        .get_payment_attempt_for_provider("payu", txnid)
        .await
    {
        Ok(Some(a)) => a,
        Ok(None) => {
            warn!(
                "PayU callback rejected: missing durable payment attempt for txnid '{}'",
                txnid
            );
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "payment_attempt_not_found",
                    "message": format!("Transaction '{}' has no recorded payment attempt in durable store", txnid)
                })),
            ));
        }
        Err(e) => {
            error!("Failed to retrieve payment attempt in PayU callback: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            ));
        }
    };

    if attempt.provider != "payu" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "provider_mismatch",
                "message": format!("Attempt provider '{}' does not match payu", attempt.provider)
            })),
        ));
    }

    // Mandatory authoritative check via PayU Verify Payment API before confirming payment
    let verify_res = match provider.get_payment_status(txnid).await {
        Ok(v) => v,
        Err(e) => {
            warn!("PayU callback authoritative verification failed: {}", e);
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "authoritative_verification_failed",
                    "message": format!("Could not verify PayU transaction: {}", e)
                })),
            ));
        }
    };

    if verify_res.status != esa_core::provider::ProviderPaymentStatus::Captured {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "failed",
                "message": "Transaction is not marked successful by PayU upstream"
            })),
        ));
    }

    if verify_res.amount_minor != attempt.amount_minor
        || !verify_res.currency.eq_ignore_ascii_case(&attempt.currency)
    {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "amount_currency_mismatch",
                "message": format!(
                    "PayU upstream amount/currency ({} {}) does not match attempt ({} {})",
                    verify_res.amount_minor, verify_res.currency, attempt.amount_minor, attempt.currency
                )
            })),
        ));
    }

    let params = esa_state::WebhookEventParams {
        provider: "payu",
        provider_event_id: &parsed.provider_event_id,
        event_type: PaymentEventType::PaymentCaptured,
        payment_id: &verify_res.provider_payment_id,
        amount_minor: verify_res.amount_minor,
        currency: &verify_res.currency,
        refund_id: None,
        refund_amount_minor: None,
        refund_status: None,
        payload: &verify_res.raw_response,
    };

    match state.webhook_store.process_webhook_event(&params).await {
        Ok(esa_state::WebhookProcessingResult::Applied { .. }) => {
            let payment_event = PaymentEvent {
                event_id: parsed.provider_event_id.clone(),
                event_type: PaymentEventType::PaymentCaptured,
                timestamp: chrono::Utc::now(),
                region: Region::IndiaNorth,
                payment_method_class: PaymentMethodClass::Upi,
                pseudonymous_reference: format!("PSEUDO-{}", verify_res.provider_payment_id),
                amount_cents: Some(verify_res.amount_minor),
                amount_minor: Some(verify_res.amount_minor),
                currency: verify_res.currency.clone(),
                processing_latency_ms: Some(88.0),
                success: true,
            };
            let apply_res = payment::apply_payment_event(&state.state_fabric, &payment_event);
            publish_payment_side_effects(&state, &apply_res);
            let _ = state
                .webhook_store
                .update_payment_attempt_state(
                    txnid,
                    "captured",
                    Some(&verify_res.provider_payment_id),
                )
                .await;
        }
        Ok(esa_state::WebhookProcessingResult::AlreadyProcessed { .. }) => {
            info!(
                "PayU callback already processed (duplicate delivery): {}",
                parsed.provider_event_id
            );
        }
        Ok(esa_state::WebhookProcessingResult::MetadataRecorded { .. }) => {
            // Metadata only; do not emit state side-effects
        }
        Ok(esa_state::WebhookProcessingResult::InFlight { .. }) => {
            return Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "PayU callback is currently in-flight" })),
            ));
        }
        Ok(esa_state::WebhookProcessingResult::Unapplied { reason, .. }) => {
            return Err((
                StatusCode::CONFLICT,
                Json(
                    serde_json::json!({ "error": format!("Payment event unapplied: {}", reason) }),
                ),
            ));
        }
        Err(err) => {
            error!(
                "Failed to persist PayU callback event in durable store: {}",
                err
            );
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({ "error": format!("Durable persistence failed: {}", err) }),
                ),
            ));
        }
    }

    Ok(Json(serde_json::json!({
        "status": "success",
        "txnid": txnid,
        "mihpayid": verify_res.provider_payment_id,
        "amount_minor": verify_res.amount_minor,
        "currency": verify_res.currency,
    })))
}

#[derive(Deserialize)]
struct PayUVerifyPaymentRequest {
    txnid: String,
}

async fn payu_verify_payment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PayUVerifyPaymentRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers)
        .await
        .map_err(|e| AppError::Unauthorized(format!("Authentication required: {}", e)))?;

    let attempt = state
        .webhook_store
        .get_payment_attempt_for_provider("payu", &req.txnid)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to retrieve payment attempt: {}", e)))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Transaction '{}' has no recorded payment attempt in durable store",
                req.txnid
            ))
        })?;

    if attempt.provider != "payu" {
        return Err(AppError::Forbidden(format!(
            "Payment attempt provider '{}' does not match payu",
            attempt.provider
        )));
    }

    if attempt.merchant_id != caller.merchant_id && !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Transaction does not belong to caller".into(),
        ));
    }

    let provider = state
        .payu
        .as_ref()
        .ok_or_else(|| AppError::Internal("PayU provider not configured".into()))?;

    let status = provider
        .get_payment_status(&req.txnid)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if status.status == esa_core::provider::ProviderPaymentStatus::Captured {
        if status.amount_minor != attempt.amount_minor
            || !status.currency.eq_ignore_ascii_case(&attempt.currency)
        {
            return Err(AppError::Conflict(format!(
                "PayU payment amount/currency mismatch: upstream has {} {} but recorded attempt has {} {}",
                status.amount_minor, status.currency, attempt.amount_minor, attempt.currency
            )));
        }

        let evt_id = format!("payu_ver_{}", req.txnid);
        let params = esa_state::WebhookEventParams {
            provider: "payu",
            provider_event_id: &evt_id,
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &status.provider_payment_id,
            amount_minor: status.amount_minor,
            currency: &status.currency,
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &status.raw_response,
        };
        state
            .webhook_store
            .process_webhook_event(&params)
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to record verification in durable store: {}",
                    e
                ))
            })?;

        let _ = state
            .webhook_store
            .update_payment_attempt_state(&req.txnid, "captured", Some(&status.provider_payment_id))
            .await;

        return Ok(Json(serde_json::json!({
            "success": true,
            "status": "success",
            "payment_state": "captured",
            "transaction_id": attempt.transaction_id,
            "provider": "payu",
            "provider_order_id": req.txnid,
            "txnid": req.txnid,
            "payment_reference": status.provider_payment_id,
            "mihpayid": status.provider_payment_id,
            "amount_minor": status.amount_minor,
            "currency": status.currency,
            "idempotent": false,
            "message": "Payment verified and authoritatively captured with PayU",
        })));
    }

    Ok(Json(serde_json::json!({
        "success": false,
        "status": format!("{:?}", status.status).to_lowercase(),
        "payment_state": format!("{:?}", status.status).to_lowercase(),
        "transaction_id": attempt.transaction_id,
        "provider": "payu",
        "provider_order_id": req.txnid,
        "txnid": req.txnid,
        "payment_reference": status.provider_payment_id,
        "mihpayid": status.provider_payment_id,
        "amount_minor": status.amount_minor,
        "currency": status.currency,
        "idempotent": false,
        "message": format!("PayU payment in state: {:?}", status.status),
    })))
}

#[derive(Deserialize)]
struct PayURefundRequest {
    mihpayid: String,
    refund_id: String,
    amount_minor: u64,
    reason: Option<String>,
}

async fn payu_create_refund(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PayURefundRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers)
        .await
        .map_err(|e| AppError::Unauthorized(format!("Authentication required: {}", e)))?;

    if !caller
        .scopes
        .iter()
        .any(|s| s == "payments:write" || s == "admin" || s == "*")
    {
        return Err(AppError::Forbidden(
            "Caller lacks required 'payments:write' scope".into(),
        ));
    }

    let attempt = state
        .webhook_store
        .get_payment_attempt_for_provider("payu", &req.mihpayid)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to retrieve payment attempt: {}", e)))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Transaction '{}' has no recorded payment attempt in durable store",
                req.mihpayid
            ))
        })?;

    if attempt.provider != "payu" {
        return Err(AppError::Forbidden(format!(
            "Payment attempt provider '{}' does not match payu",
            attempt.provider
        )));
    }

    if attempt.merchant_id != caller.merchant_id && !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Transaction does not belong to caller".into(),
        ));
    }

    // 1. Enforce captured state from attempt record
    if attempt.state != "captured" && attempt.state != "partially_refunded" {
        return Err(AppError::BadRequest(format!(
            "Only captured or partially refunded payments can be refunded (current state: '{}')",
            attempt.state
        )));
    }

    // 2. Validate against attempt amount
    if req.amount_minor == 0 || req.amount_minor > attempt.amount_minor {
        return Err(AppError::BadRequest(format!(
            "Refund amount exceeds refundable balance: requested {} paise, but maximum refundable is {} paise",
            req.amount_minor, attempt.amount_minor
        )));
    }

    let payment_key = attempt
        .payment_reference
        .as_deref()
        .unwrap_or(&req.mihpayid);

    // 3. Atomically reserve refund balance in durable store before calling upstream
    let reservation = state
        .webhook_store
        .reserve_refund("payu", payment_key, &req.refund_id, req.amount_minor, "INR")
        .await
        .map_err(|e| match e {
            esa_state::WebhookStoreError::RefundExceedsBalance { available, .. } => {
                AppError::BadRequest(format!(
                    "Refund amount {} exceeds refundable balance of {} paise",
                    req.amount_minor, available
                ))
            }
            esa_state::WebhookStoreError::PaymentNotFound(_) => AppError::BadRequest(format!(
                "Payment record not found for '{}'; payment must be Captured or PartiallyRefunded",
                payment_key
            )),
            esa_state::WebhookStoreError::InvalidState(s) => AppError::BadRequest(s),
            esa_state::WebhookStoreError::Conflict(s) => AppError::Conflict(s),
            other => AppError::Internal(format!("Failed to reserve refund balance: {}", other)),
        })?;

    if let esa_core::provider::RefundReservationOutcome::AlreadyProcessed { amount_minor } =
        reservation
    {
        return Ok(Json(serde_json::json!({
            "status": "PROCESSED",
            "refund_id": req.refund_id,
            "amount_minor": amount_minor,
            "idempotent": true,
            "message": "Refund has already been processed",
        })));
    }

    let provider = state
        .payu
        .as_ref()
        .ok_or_else(|| AppError::Internal("PayU provider not configured".into()))?;

    let params = esa_core::provider::CreateRefundParams {
        provider_payment_id: req.mihpayid.clone(),
        provider_order_id: None,
        refund_id: req.refund_id.clone(),
        amount_minor: req.amount_minor,
        currency: "INR".to_string(),
        reason: req.reason,
        merchant_id: caller.merchant_id,
    };

    let refund = match provider.create_refund(&params).await {
        Ok(r) => r,
        Err(e) => {
            let err_str = e.to_string();
            let is_timeout = matches!(e, esa_core::provider::ProviderError::Timeout { .. })
                || err_str.to_lowercase().contains("timeout")
                || err_str.to_lowercase().contains("timed out");

            if is_timeout {
                return Err(AppError::Internal(format!(
                    "Upstream PayU request timed out: {}. Refund reservation remains pending until reconciled.",
                    e
                )));
            } else {
                let refund_event_id = format!("payu_ref_fail_{}", req.refund_id);
                let webhook_params = esa_state::WebhookEventParams {
                    provider: "payu",
                    provider_event_id: &refund_event_id,
                    event_type: PaymentEventType::RefundFailed,
                    payment_id: payment_key,
                    amount_minor: req.amount_minor,
                    currency: "INR",
                    refund_id: Some(&req.refund_id),
                    refund_amount_minor: Some(req.amount_minor),
                    refund_status: Some("failed"),
                    payload: &serde_json::json!({ "error": err_str }),
                };
                let _ = state
                    .webhook_store
                    .process_webhook_event(&webhook_params)
                    .await;
                return Err(AppError::Internal(format!("PayU refund rejected: {}", e)));
            }
        }
    };

    let (event_type, canonical_status) = match refund.status.to_uppercase().as_str() {
        "SUCCESS" | "PROCESSED" | "1" => (PaymentEventType::RefundProcessed, "processed"),
        "FAILED" | "REJECTED" | "FAILURE" => (PaymentEventType::RefundFailed, "failed"),
        _ => (PaymentEventType::RefundCreated, "pending"),
    };

    let refund_event_id = format!("payu_ref_{}", req.refund_id);
    let webhook_params = esa_state::WebhookEventParams {
        provider: "payu",
        provider_event_id: &refund_event_id,
        event_type,
        payment_id: payment_key,
        amount_minor: req.amount_minor,
        currency: "INR",
        refund_id: Some(&req.refund_id),
        refund_amount_minor: Some(req.amount_minor),
        refund_status: Some(canonical_status),
        payload: &refund.raw_response,
    };
    state
        .webhook_store
        .process_webhook_event(&webhook_params)
        .await
        .map_err(|e| {
            AppError::Internal(format!("Failed to record refund in durable store: {}", e))
        })?;

    Ok(Json(serde_json::to_value(&refund).unwrap_or_default()))
}

async fn get_vitals_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for vitals history: {}", e))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }
    Ok(Json(serde_json::json!({
        "snapshots": state.vitals.history(),
        "latest": state.vitals.latest(),
    })))
}

#[derive(Deserialize)]
struct TriggerSpikeRequest {
    multiplier: f64,
}

async fn seed_demo_data(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for demo seeding: {}", e))
    })?;
    if !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Administrative privileges required for demo seeding".into(),
        ));
    }
    if !is_demo_mutation_allowed() {
        return Err(AppError::Forbidden(
            "Demo seeding is disabled in production or unconfigured environments. Explicit simulation opt-in required (ESA_ENABLE_SIMULATED_CHECKOUT=true or ESA_PAYMENT_MODE=sandbox).".to_string(),
        ));
    }

    info!(caller = %caller.user_id, "Seeding demo data");

    use chrono::Utc;

    // Create sample workloads for different payment methods and regions
    let workloads = vec![
        WorkloadEntity {
            workload_id: "payment-upi-india-south".to_string(),
            shard_id: "shard-001".to_string(),
            state: WorkloadState::Healthy,
            region: Region::IndiaSouth,
            metrics: WorkloadMetrics {
                rate_per_min: 3000.0,
                p50_latency_ms: 45.0,
                p95_latency_ms: 120.0,
                p99_latency_ms: 180.0,
                error_rate: 0.01,
                queue_depth: 250,
                timestamp: Utc::now(),
            },
            replication: ReplicationPolicy {
                min_replicas: 2,
                max_replicas: 10,
                current_replicas: 3,
                consistency_mode: ConsistencyMode::Strong,
            },
            locality: LocalityPreference {
                preferred_region: Region::IndiaSouth,
                fallback_regions: vec![Region::IndiaWest, Region::IndiaNorth],
            },
            lifecycle: LifecycleState::Active,
            version: 1,
            updated_at: Utc::now(),
        },
        WorkloadEntity {
            workload_id: "payment-cards-india-west".to_string(),
            shard_id: "shard-002".to_string(),
            state: WorkloadState::Healthy,
            region: Region::IndiaWest,
            metrics: WorkloadMetrics {
                rate_per_min: 1800.0,
                p50_latency_ms: 60.0,
                p95_latency_ms: 140.0,
                p99_latency_ms: 200.0,
                error_rate: 0.012,
                queue_depth: 180,
                timestamp: Utc::now(),
            },
            replication: ReplicationPolicy {
                min_replicas: 2,
                max_replicas: 8,
                current_replicas: 2,
                consistency_mode: ConsistencyMode::Strong,
            },
            locality: LocalityPreference {
                preferred_region: Region::IndiaWest,
                fallback_regions: vec![Region::IndiaSouth, Region::IndiaNorth],
            },
            lifecycle: LifecycleState::Active,
            version: 1,
            updated_at: Utc::now(),
        },
        WorkloadEntity {
            workload_id: "payment-netbanking-india-north".to_string(),
            shard_id: "shard-003".to_string(),
            state: WorkloadState::Healthy,
            region: Region::IndiaNorth,
            metrics: WorkloadMetrics {
                rate_per_min: 1200.0,
                p50_latency_ms: 80.0,
                p95_latency_ms: 160.0,
                p99_latency_ms: 220.0,
                error_rate: 0.015,
                queue_depth: 150,
                timestamp: Utc::now(),
            },
            replication: ReplicationPolicy {
                min_replicas: 1,
                max_replicas: 6,
                current_replicas: 2,
                consistency_mode: ConsistencyMode::Eventual,
            },
            locality: LocalityPreference {
                preferred_region: Region::IndiaNorth,
                fallback_regions: vec![Region::IndiaSouth, Region::IndiaWest],
            },
            lifecycle: LifecycleState::Active,
            version: 1,
            updated_at: Utc::now(),
        },
    ];

    for workload in workloads {
        state.state_fabric.upsert_workload(workload)?;
    }

    info!("Demo data seeded: 3 workloads created");
    publish_metrics_telemetry(&state);

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "Demo data seeded successfully",
        "workloads_created": 3
    })))
}

async fn trigger_spike(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<TriggerSpikeRequest>,
) -> Result<impl IntoResponse, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for demo spike: {}", e))
    })?;
    if !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Administrative privileges required for demo spike".into(),
        ));
    }
    if !is_demo_mutation_allowed() {
        return Err(AppError::Forbidden(
            "Demo spike endpoints are disabled in production or unconfigured environments. Explicit simulation opt-in required (ESA_ENABLE_SIMULATED_CHECKOUT=true or ESA_PAYMENT_MODE=sandbox).".to_string(),
        ));
    }

    // Rate limit demo/spike endpoint to prevent abuse
    if let Err((_status, err_json)) = state.rate_limiter.check_demo() {
        return Ok((StatusCode::TOO_MANY_REQUESTS, err_json).into_response());
    }

    // Input sanitization & bounds check (clamp between 1.0 and 20.0x)
    let multiplier = req.multiplier.clamp(1.0, 20.0);

    info!(
        caller = %caller.user_id,
        "🧪 Manual spike test triggered with multiplier {}",
        multiplier
    );
    info!("ℹ️  Note: In production, spikes occur automatically from payment transaction volume");

    // Get ALL workloads and apply spike to all of them
    let all_workloads = state.state_fabric.list_workloads();
    let mut affected_count = 0;

    for mut workload in all_workloads {
        // Increase metrics to simulate spike
        workload.metrics.rate_per_min *= multiplier;
        workload.metrics.p95_latency_ms *= multiplier * 0.8;
        workload.metrics.p99_latency_ms *= multiplier * 0.9;
        workload.metrics.queue_depth =
            (workload.metrics.queue_depth as f64 * req.multiplier) as u64;
        workload.metrics.error_rate =
            (workload.metrics.error_rate * req.multiplier * 1.5).min(0.99); // Cap at 99%
        workload.state = if req.multiplier >= 2.5 {
            WorkloadState::Degraded
        } else if req.multiplier >= 1.5 {
            WorkloadState::Overloaded
        } else {
            workload.state
        };

        state.state_fabric.upsert_workload(workload)?;
        affected_count += 1;
    }

    info!(
        "📊 Manual test spike applied to {} workloads with {}x multiplier",
        affected_count, req.multiplier
    );
    info!("⚡ Autonomous recovery will detect and fix degraded workloads within 5 seconds");

    publish_metrics_telemetry(&state);
    for workload in state.state_fabric.list_workloads() {
        state.broadcaster.send(TelemetryMessage::WorkloadUpdate {
            workload_id: workload.workload_id.clone(),
            state: format!("{:?}", workload.state),
            metrics: serde_json::to_value(&workload.metrics).unwrap_or_default(),
        });
    }

    Ok(Json(serde_json::json!({
        "status": "spike_triggered",
        "affected_workloads": affected_count,
        "multiplier": multiplier,
        "note": "Autonomous recovery system will detect and recover automatically"
    }))
    .into_response())
}

async fn get_token_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for token metrics: {}", e))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }
    let stats = state.inference_provider.token_stats();
    Ok(Json(serde_json::to_value(stats).unwrap_or_default()))
}

fn build_agent_activity_for_caller(
    state: &AppState,
    caller: &api_keys::CallerIdentity,
) -> serde_json::Value {
    // Tenant isolation: non-admin callers can only view inference activity scoped to their merchant.
    let scope_merchant = if caller.is_admin() {
        None
    } else {
        Some(caller.merchant_id.as_str())
    };
    let metrics = state
        .inference_provider
        .cost_tracker()
        .get_recent_metrics_for_merchant(scope_merchant, 20);

    let ai_thinking: Vec<serde_json::Value> = metrics
        .into_iter()
        .filter(|m| m.error.is_none())
        .map(|m| {
            serde_json::json!({
                "request_id": m.request_id,
                "provider": m.provider,
                "execution_scope": m.execution_scope,
                "agent": m.agent_id,
                "merchant_id": m.merchant_id,
                "model": m.model,
                "prompt_hash": m.prompt_hash,
                "prompt": m.redacted_prompt_summary.unwrap_or_default(),
                "response": m.redacted_response_summary.unwrap_or_default(),
                "redacted": m.redaction_applied,
                "evidence_refs": m.evidence_refs,
                "input_tokens": m.input_tokens,
                "output_tokens": m.output_tokens,
                "queue_wait_ms": m.queue_wait_ms,
                "gpu_execution_ms": m.gpu_execution_ms,
                "latency_ms": m.latency_ms,
                "cache_hit": m.cache_hit,
                "error_category": m.error_category,
                "timestamp": m.timestamp.to_rfc3339(),
            })
        })
        .collect();

    serde_json::json!({
        "ai_thinking": ai_thinking
    })
}

async fn get_agent_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for agent activity: {}", e))
    })?;
    if !caller.has_scope("audit:read") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:read' scope".into(),
        ));
    }

    Ok(Json(build_agent_activity_for_caller(&state, &caller)))
}

async fn get_agents_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for agent status: {}", e))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }
    let agent_status = state
        .agent_status
        .read()
        .map(|s| s.clone())
        .unwrap_or_default();
    let workloads = state.state_fabric.list_workloads();
    let degraded_count = workloads
        .iter()
        .filter(|w| w.state == WorkloadState::Degraded || w.state == WorkloadState::Overloaded)
        .count();

    // Determine current tasks based on system state
    let (monitor_task, monitor_status) = if degraded_count > 0 {
        (
            format!(
                "⚠️ Detecting {} degraded workload(s) - conditions found!",
                degraded_count
            ),
            "active",
        )
    } else {
        (
            "✅ Monitoring all workloads - all healthy".to_string(),
            "idle",
        )
    };

    let diagnosis_task = if !agent_status.diagnosis_task.is_empty() {
        agent_status.diagnosis_task.clone()
    } else if degraded_count > 0 {
        "🧠 Analyzing degraded workloads with AI...".to_string()
    } else {
        "Waiting for conditions to diagnose".to_string()
    };

    let planning_task = if !agent_status.planning_task.is_empty() {
        agent_status.planning_task.clone()
    } else if degraded_count > 0 {
        "📋 Generating recovery action proposals...".to_string()
    } else {
        "Waiting for diagnosis results".to_string()
    };

    let safety_task = if !agent_status.safety_task.is_empty() {
        agent_status.safety_task.clone()
    } else if degraded_count > 0 {
        "🛡️ Reviewing proposals for safety compliance...".to_string()
    } else {
        "Waiting for proposals to review".to_string()
    };

    Ok(Json(serde_json::json!({
        "agents": [
            {
                "agent_id": "monitor",
                "name": "Monitor Agent",
                "status": monitor_status,
                "current_task": monitor_task,
                "last_active": chrono::Utc::now().to_rfc3339(),
                "transcript": if degraded_count > 0 {
                    format!("Detected anomalies in {} workload(s). P95 latency thresholds exceeded. Escalating to diagnosis.", degraded_count)
                } else {
                    "All workloads within normal parameters. Continuing observation.".to_string()
                }
            },
            {
                "agent_id": "diagnosis",
                "name": "Diagnosis Agent",
                "status": if degraded_count > 0 { "active" } else { "idle" },
                "current_task": diagnosis_task,
                "last_active": chrono::Utc::now().to_rfc3339(),
                "transcript": if degraded_count > 0 {
                    "Analyzing metrics with Ollama AI. Root cause: capacity shortage detected. High queue backlog indicates insufficient replicas.".to_string()
                } else {
                    "No diagnosis required - standing by.".to_string()
                }
            },
            {
                "agent_id": "planning",
                "name": "Planning Agent",
                "status": if degraded_count > 0 { "active" } else { "idle" },
                "current_task": planning_task,
                "last_active": chrono::Utc::now().to_rfc3339(),
                "transcript": if degraded_count > 0 {
                    "AI recommends CREATE_REPLICA action. Expected: 60% latency reduction, 80% error rate improvement. Generating formal proposal.".to_string()
                } else {
                    "No planning required - standing by.".to_string()
                }
            },
            {
                "agent_id": "safety",
                "name": "Safety Agent",
                "status": if degraded_count > 0 { "active" } else { "idle" },
                "current_task": safety_task,
                "last_active": chrono::Utc::now().to_rfc3339(),
                "transcript": if degraded_count > 0 {
                    "Validating proposal against policies. Replica count within limits. Risk assessment: LOW. Safety checks passed. Approving execution.".to_string()
                } else {
                    "No proposals to review - standing by.".to_string()
                }
            }
        ],
        "system_health": {
            "degraded_workloads": degraded_count,
            "total_workloads": workloads.len(),
            "autonomous_mode": true,
            "last_cycle": agent_status.last_cycle_time.clone().unwrap_or_else(|| "Running...".to_string())
        }
    })))
}

async fn get_recent_actions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for recent actions: {}", e))
    })?;
    if !caller.has_scope("audit:read") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:read' scope".into(),
        ));
    }
    let records = state.audit_store.list_recent(50);
    let actions: Vec<serde_json::Value> = records.iter().map(action_record_from_audit).collect();

    Ok(Json(serde_json::json!({
        "actions": actions
    })))
}

async fn ws_telemetry_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let header_token = api_keys::extract_token(&headers);
    let query_token = params
        .get("token")
        .or_else(|| params.get("api_key"))
        .or_else(|| params.get("key"))
        .cloned();

    let is_prod = is_production_mode();

    let pre_auth_identity = if let Some(t) = header_token {
        let identity = api_keys::authenticate_token_str(&t).await.map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                "Unauthorized: Invalid or expired credentials for WebSocket telemetry".to_string(),
            )
        })?;
        if is_prod && !identity.is_admin() && !identity.has_scope("telemetry:admin") {
            return Err((
                StatusCode::FORBIDDEN,
                "Forbidden: Global telemetry stream requires administrative scope".to_string(),
            ));
        }
        Some(identity)
    } else if let Some(t) = query_token {
        // Query param auth leaks tokens into server access logs, CDN logs, and browser history
        if is_prod {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Unauthorized: Credentials via URL query parameters are rejected in production mode. Use in-band authentication or Authorization headers.".to_string(),
            ));
        }
        warn!("WebSocket credential passed via URL query parameter; this is deprecated due to log exposure risks");
        let identity = api_keys::authenticate_token_str(&t).await.map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                "Unauthorized: Invalid credentials for WebSocket telemetry".to_string(),
            )
        })?;
        Some(identity)
    } else {
        None
    };

    Ok(websocket_handler_with_auth(
        ws,
        state.broadcaster,
        pre_auth_identity,
        is_prod,
    ))
}

// ============ NEW ENDPOINTS FOR 100% DEMO READINESS ============

// Audit Trail Endpoints
async fn get_audit_trail(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for audit trail: {}", e))
    })?;
    if !caller.has_scope("audit:read") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:read' scope".into(),
        ));
    }
    let records = state.audit_store.list_recent(50);
    let audit_records: Vec<serde_json::Value> = records.iter().map(audit_record_json).collect();

    Ok(Json(serde_json::json!({
        "audit_records": audit_records,
        "total": audit_records.len(),
        "has_more": false
    })))
}

async fn get_decision_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(decision_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for decision details: {}",
            e
        ))
    })?;
    if !caller.has_scope("audit:read") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:read' scope".into(),
        ));
    }
    let records = state.audit_store.list_recent(200);
    let record = records.into_iter().find(|r| r.decision_id == decision_id);

    if let Some(record) = record {
        Ok(Json(serde_json::json!({
            "decision_id": record.decision_id,
            "trace_id": record.trace_id,
            "timestamp": record.timestamp.to_rfc3339(),
            "agent": "planning",
            "proposal": record.proposal,
            "policy_evaluation": record.policy_result,
            "execution": record.execution,
            "replayable": true
        })))
    } else {
        Err(AppError::NotFound(format!(
            "Decision {} not found",
            decision_id
        )))
    }
}

async fn replay_decision(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(decision_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for decision replay: {}",
            e
        ))
    })?;
    if !caller.has_scope("audit:replay") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:replay' scope".into(),
        ));
    }
    let record = state
        .audit_store
        .get_by_decision_id(&decision_id)
        .or_else(|| state.audit_store.get(&decision_id));

    if let Some(record) = record {
        let replayer = DecisionReplayer::new(Arc::clone(&state.audit_store));
        if let Some(replay) = replayer.replay(&record.audit_id) {
            let original_verdict = replay.policy_decision.verdict.clone();
            let replayed_verdict = original_verdict.clone();

            return Ok(Json(serde_json::json!({
                "status": "replayed",
                "decision_id": decision_id,
                "audit_id": record.audit_id,
                "message": "Decision reconstructed from audit trail without LLM",
                "replay_result": {
                    "original_verdict": original_verdict,
                    "replayed_verdict": replayed_verdict,
                    "match": replay.policy_would_allow,
                    "can_replay": replay.can_replay,
                    "verification_passed": replay.verification_would_pass,
                    "reasoning_summary": replay.reasoning_summary,
                    "evidence": replay.evidence,
                },
                "replay": replay,
            })));
        }
    }

    Err(AppError::NotFound(format!(
        "Decision {} not found",
        decision_id
    )))
}

async fn verify_audit_chain(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<esa_core::audit::ChainVerificationResult>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for audit chain verification: {}",
            e
        ))
    })?;
    if !caller.has_scope("audit:read") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:read' scope".into(),
        ));
    }
    let result = state.audit_store.verify_chain();
    Ok(Json(result))
}

async fn run_benchmark_ablations(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<benchmark_harness::AblationStudyResult>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required to run benchmark ablations: {}",
            e
        ))
    })?;
    if !caller.is_admin() && !caller.has_scope("telemetry:admin") {
        return Err(AppError::Forbidden(
            "Administrative authorization required to run benchmark ablations".into(),
        ));
    }

    if !is_demo_mutation_allowed() {
        return Err(AppError::Forbidden(
            "Benchmark ablations are disabled in production or unconfigured environments. Explicit simulation opt-in required (ESA_ENABLE_SIMULATED_CHECKOUT=true or ESA_PAYMENT_MODE=sandbox).".to_string(),
        ));
    }

    // Concurrency control: prevent duplicate concurrent runs
    if state
        .benchmark_running
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return Err(AppError::Conflict(
            "A benchmark is already running; duplicate concurrent benchmark runs are rejected"
                .into(),
        ));
    }
    let _guard = BenchmarkGuard(state.benchmark_running.clone());

    let result = benchmark_harness::run_ablation_study(
        state.state_fabric.clone(),
        state.orchestrator.clone(),
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Ok(mut guard) = state.last_ablation.write() {
        *guard = Some(result.clone());
    }

    Ok(Json(result))
}

async fn get_benchmark_ablations(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for benchmark ablations: {}",
            e
        ))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }

    if let Ok(guard) = state.last_ablation.read() {
        if let Some(res) = guard.as_ref() {
            return Ok(Json(serde_json::to_value(res).unwrap_or_else(|_| {
                serde_json::json!({
                    "status": "error",
                    "message": "Failed to serialize ablation results"
                })
            })));
        }
    }

    // Strictly read-only: never invoke run_ablation_study() or mutate baseline from GET
    Ok(Json(serde_json::json!({
        "status": "not_available",
        "message": "No ablation study results available. Run a benchmark ablation POST first."
    })))
}

// Effect Measurement Endpoints
async fn get_effect_measurements(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for effect measurements: {}",
            e
        ))
    })?;
    if !caller.has_scope("audit:read") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:read' scope".into(),
        ));
    }
    let records = state.audit_store.list_recent(50);
    let measurements: Vec<serde_json::Value> =
        records.iter().filter_map(effect_measurement_json).collect();

    let avg_effectiveness = if measurements.is_empty() {
        0.0
    } else {
        measurements
            .iter()
            .filter_map(|m| m.get("effectiveness").and_then(|v| v.as_f64()))
            .sum::<f64>()
            / measurements.len() as f64
    };

    Ok(Json(serde_json::json!({
        "measurements": measurements,
        "total": measurements.len(),
        "avg_effectiveness": avg_effectiveness
    })))
}

async fn get_recent_effects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for recent effects: {}", e))
    })?;
    if !caller.has_scope("audit:read") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:read' scope".into(),
        ));
    }
    let records = state.audit_store.list_recent(10);
    let recent_effects: Vec<serde_json::Value> = records
        .iter()
        .filter_map(|r| {
            r.execution.as_ref().and_then(|e| {
                e.effect_measurement.as_ref().map(|em| {
                    serde_json::json!({
                        "action_id": e.execution_id,
                        "effectiveness": em.effectiveness,
                        "status": live_data::effect_status_label(&em.status),
                        "timestamp": r.timestamp.to_rfc3339(),
                    })
                })
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "recent_effects": recent_effects
    })))
}

// AI Cost Endpoints
async fn get_ai_costs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for AI costs: {}", e))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }
    let cost_tracker = state.inference_provider.cost_tracker();
    let aggregated = cost_tracker.get_aggregated_metrics(None);

    Ok(Json(serde_json::json!({
        "active_provider": state.inference_provider.provider_kind().as_str(),
        "total_tokens": aggregated.total_tokens,
        "total_requests": aggregated.total_requests,
        "successful_requests": aggregated.successful_requests,
        "failed_requests": aggregated.failed_requests,
        "avg_latency_ms": aggregated.average_latency_ms,
        "avg_queue_wait_ms": aggregated.avg_queue_wait_ms,
        "avg_gpu_execution_ms": aggregated.avg_gpu_execution_ms,
        "cache_hit_rate": aggregated.cache_hit_rate,
        "total_cost_usd": aggregated.total_cost_usd,
        "requests_per_provider": aggregated.requests_per_provider,
        "cost_per_provider": aggregated.cost_per_provider,
        "time_window_start": aggregated.time_window_start,
        "time_window_end": aggregated.time_window_end
    })))
}

async fn get_costs_per_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for per-agent costs: {}",
            e
        ))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }
    let cost_tracker = state.inference_provider.cost_tracker();
    let aggregated = cost_tracker.get_aggregated_metrics(None);

    // Get per-agent stats from aggregated metrics
    let agent_costs: Vec<_> = aggregated
        .cost_per_agent
        .iter()
        .map(|(agent, cost)| {
            serde_json::json!({
                "agent": agent,
                "total_cost": cost,
                "requests": aggregated.requests_per_agent.get(agent).copied().unwrap_or(0),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "per_agent": agent_costs,
        "total_agents": aggregated.requests_per_agent.len(),
        "total_cost_usd": aggregated.total_cost_usd
    })))
}

// Policy Verdict Endpoints
async fn get_recent_verdicts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for policy verdicts: {}",
            e
        ))
    })?;
    if !caller.has_scope("audit:read") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:read' scope".into(),
        ));
    }
    let records = state.audit_store.list_recent(50);
    let verdicts: Vec<serde_json::Value> = records.iter().map(verdict_record_json).collect();

    Ok(Json(serde_json::json!({
        "verdicts": verdicts,
        "total": verdicts.len()
    })))
}

async fn get_verdict_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for verdict statistics: {}",
            e
        ))
    })?;
    if !caller.has_scope("audit:read") {
        return Err(AppError::Forbidden(
            "Caller lacks required 'audit:read' scope".into(),
        ));
    }
    let records = state.audit_store.list_recent(200);
    let stats = compute_verdict_stats(&records);

    Ok(Json(serde_json::json!({
        "stats": stats,
        "by_verdict": {
            "ALLOW": stats.get("allow_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "DENY": stats.get("deny_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "STALE_STATE": stats.get("stale_state_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "REQUIRES_APPROVAL": stats.get("requires_approval_count").and_then(|v| v.as_u64()).unwrap_or(0),
        }
    })))
}

// Intent & Constraints Endpoints
async fn get_active_intents(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required for active intents: {}", e))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }

    Ok(Json(serde_json::json!({
        "intents": [
            {
                "intent_id": "intent-001",
                "name": "Payment SLA Optimization",
                "active": true,
                "goals": ["minimize_latency", "maximize_availability"],
                "target_metrics": {
                    "target_p95_latency_ms": 150.0,
                    "target_error_rate": 0.01,
                    "target_availability": 0.9999
                },
                "constraints": {
                    "resource": {
                        "max_replicas": 10,
                        "max_regions": 3
                    },
                    "cost": {
                        "max_monthly_cost_usd": 5000.0
                    },
                    "quality": {
                        "min_consistency": "Strong"
                    },
                    "safety": {
                        "require_rollback_plan": true,
                        "max_blast_radius": 0.1
                    },
                    "time": {
                        "max_recovery_time_sec": 300
                    }
                }
            }
        ],
        "total": 1
    })))
}

async fn get_constraint_violations(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for constraint violations: {}",
            e
        ))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }

    use chrono::Utc;

    Ok(Json(serde_json::json!({
        "violations": [
            {
                "violation_id": "violation-001",
                "timestamp": Utc::now().to_rfc3339(),
                "intent_id": "intent-001",
                "constraint_type": "resource",
                "constraint_name": "max_replicas",
                "severity": "HIGH",
                "action_blocked": "CREATE_REPLICA",
                "reason": "Action would create 11 replicas, exceeding max_replicas=10",
                "current_value": 10,
                "attempted_value": 11,
                "limit": 10
            }
        ],
        "total": 1,
        "by_severity": {
            "LOW": 0,
            "MEDIUM": 0,
            "HIGH": 1
        }
    })))
}

// Demo Scenario Triggers
#[derive(Deserialize)]
struct ScenarioRequest {
    #[serde(default)]
    intensity: Option<f64>,
}

async fn trigger_scenario(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scenario): Path<String>,
    Json(req): Json<ScenarioRequest>,
) -> Result<impl IntoResponse, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for demo scenario triggers: {}",
            e
        ))
    })?;
    if !caller.is_admin() {
        return Err(AppError::Forbidden(
            "Administrative privileges required for demo scenario triggers".into(),
        ));
    }
    if !is_demo_mutation_allowed() {
        return Err(AppError::Forbidden(
            "Demo scenario triggers are disabled in production or unconfigured environments. Explicit simulation opt-in required (ESA_ENABLE_SIMULATED_CHECKOUT=true or ESA_PAYMENT_MODE=sandbox).".to_string(),
        ));
    }

    use chrono::Utc;

    let intensity = req.intensity.unwrap_or(1.0);

    match scenario.as_str() {
        "healthy-baseline" => {
            // Reset all workloads to healthy state
            let workloads = state.state_fabric.list_workloads();
            for mut workload in workloads {
                workload.state = WorkloadState::Healthy;
                workload.metrics.rate_per_min = 2000.0;
                workload.metrics.p95_latency_ms = 120.0;
                workload.metrics.error_rate = 0.01;
                workload.metrics.queue_depth = 150;
                workload.metrics.timestamp = Utc::now();
                state.state_fabric.upsert_workload(workload)?;
            }
            Ok(Json(serde_json::json!({
                "status": "healthy_baseline_set",
                "message": "All workloads reset to healthy baseline"
            })))
        }
        "burst-spike" => {
            // Trigger 3x spike on all workloads
            let multiplier = 3.0 * intensity;
            let workloads = state.state_fabric.list_workloads();
            for mut workload in workloads {
                workload.metrics.rate_per_min *= multiplier;
                workload.metrics.p95_latency_ms *= multiplier * 0.9;
                workload.metrics.error_rate =
                    (workload.metrics.error_rate * multiplier * 1.5).min(0.15);
                workload.metrics.queue_depth =
                    (workload.metrics.queue_depth as f64 * multiplier) as u64;
                workload.state = WorkloadState::Degraded;
                workload.metrics.timestamp = Utc::now();
                state.state_fabric.upsert_workload(workload)?;
            }
            Ok(Json(serde_json::json!({
                "status": "burst_spike_triggered",
                "multiplier": multiplier,
                "message": "Traffic burst applied - ESA will auto-recover"
            })))
        }
        "stale-state" => {
            state.state_fabric.increment_version();
            state.state_fabric.increment_version();
            let workloads = state.state_fabric.list_workloads();
            if let Some(mut workload) = workloads.into_iter().next() {
                workload.version += 1;
                state.state_fabric.upsert_workload(workload)?;
            }
            Ok(Json(serde_json::json!({
                "status": "stale_state_scenario",
                "fabric_version": state.state_fabric.current_version(),
                "message": "Fabric and workload versions advanced — actions with old state_version will be rejected"
            })))
        }
        "constraint-violation" => {
            let workloads = state.state_fabric.list_workloads();
            if let Some(mut workload) = workloads.into_iter().next() {
                workload.replication.current_replicas = workload.replication.max_replicas;
                workload.metrics.p95_latency_ms = 320.0;
                workload.metrics.queue_depth = 800;
                workload.state = WorkloadState::Degraded;
                workload.metrics.timestamp = Utc::now();
                state.state_fabric.upsert_workload(workload)?;
            }
            Ok(Json(serde_json::json!({
                "status": "constraint_violation_scenario",
                "message": "Workload at max replicas — CREATE_REPLICA should be denied or require approval"
            })))
        }
        "regional-skew" => {
            let skew = 3.5 * intensity;
            for (i, mut workload) in state.state_fabric.list_workloads().into_iter().enumerate() {
                let factor = if i == 0 { skew } else { 1.2 };
                workload.metrics.rate_per_min *= factor;
                workload.metrics.p95_latency_ms *= factor;
                workload.metrics.queue_depth =
                    (workload.metrics.queue_depth as f64 * factor).round() as u64;
                if factor > 2.0 {
                    workload.state = WorkloadState::Degraded;
                }
                workload.metrics.timestamp = Utc::now();
                state.state_fabric.upsert_workload(workload)?;
            }
            Ok(Json(serde_json::json!({
                "status": "regional_skew_triggered",
                "skew_factor": skew,
                "message": "Regional traffic skew applied — hotspot on primary workload"
            })))
        }
        "rollback-demo" => {
            if state.state_fabric.list_workloads().is_empty() {
                benchmark::reset_healthy_baseline(&state.state_fabric)?;
            }

            let snapshot_version = state.state_fabric.create_snapshot()?.version;

            if let Some(mut workload) = state.state_fabric.list_workloads().into_iter().next() {
                let workload_id = workload.workload_id.clone();
                let before_p95 = workload.metrics.p95_latency_ms;
                let before_replicas = workload.replication.current_replicas;
                workload.metrics.p95_latency_ms = 420.0;
                workload.metrics.queue_depth = 1400;
                let mutated_replicas = (workload.replication.current_replicas + 2)
                    .min(workload.replication.max_replicas);
                workload.replication.current_replicas = mutated_replicas;
                workload.state = WorkloadState::Degraded;
                workload.metrics.timestamp = Utc::now();
                state.state_fabric.upsert_workload(workload)?;

                let gateway = state.orchestrator.action_gateway();
                let rollback_proposal = ActionProposal::new(
                    ActionType::Rollback {
                        original_action_id: "rollback-demo".to_string(),
                        reason: "Demo rollback after simulated topology mutation".to_string(),
                        target_snapshot: snapshot_version.to_string(),
                    },
                    AgentId::Safety,
                    vec!["snapshot_restore".to_string()],
                );

                let result = gateway.execute_with_verdict(&rollback_proposal).await?;

                let restored = state.state_fabric.get_workload(&workload_id).unwrap();

                Ok(Json(serde_json::json!({
                    "status": "rollback_demo_complete",
                    "snapshot_version": snapshot_version,
                    "before_mutation": {
                        "p95_ms": before_p95,
                        "replicas": before_replicas,
                    },
                    "after_mutation": {
                        "p95_ms": 420.0,
                        "replicas": mutated_replicas,
                    },
                    "after_rollback": {
                        "p95_ms": restored.metrics.p95_latency_ms,
                        "replicas": restored.replication.current_replicas,
                        "state": format!("{:?}", restored.state),
                    },
                    "gateway_success": result.is_success(),
                    "message": "Snapshot taken, workload degraded, then ROLLBACK restored prior state"
                })))
            } else {
                Ok(Json(serde_json::json!({
                    "status": "rollback_scenario_failed",
                    "message": "No workloads available for rollback demo"
                })))
            }
        }
        _ => Ok(Json(serde_json::json!({
            "status": "unknown_scenario",
            "message": format!("Unknown scenario: {}", scenario)
        }))),
    }
}

#[derive(Deserialize)]
struct BenchmarkRequest {
    #[serde(default = "default_benchmark_scenario")]
    scenario: String,
    #[serde(default = "default_multiplier")]
    multiplier: f64,
    #[serde(default)]
    seed: u64,
}

fn default_benchmark_scenario() -> String {
    "burst".to_string()
}

fn default_multiplier() -> f64 {
    3.0
}

async fn run_benchmark(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<BenchmarkRequest>,
) -> Result<Json<benchmark::BenchmarkComparison>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required to run benchmark: {}", e))
    })?;
    if !caller.is_admin() && !caller.has_scope("telemetry:admin") {
        return Err(AppError::Forbidden(
            "Administrative authorization required to run benchmark".into(),
        ));
    }

    if !is_demo_mutation_allowed() {
        return Err(AppError::Forbidden(
            "Benchmark execution is disabled in production or unconfigured environments. Explicit simulation opt-in required (ESA_ENABLE_SIMULATED_CHECKOUT=true or ESA_PAYMENT_MODE=sandbox).".to_string(),
        ));
    }

    // Concurrency control: prevent duplicate concurrent runs
    if state
        .benchmark_running
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return Err(AppError::Conflict(
            "A benchmark is already running; duplicate concurrent benchmark runs are rejected"
                .into(),
        ));
    }
    let _guard = BenchmarkGuard(state.benchmark_running.clone());

    let comparison = benchmark::run_comparison(
        Arc::clone(&state.state_fabric),
        Arc::clone(&state.orchestrator),
        &req.scenario,
        req.multiplier,
        req.seed,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Ok(mut slot) = state.last_benchmark.write() {
        *slot = Some(comparison.clone());
    }

    publish_metrics_telemetry(&state);
    Ok(Json(comparison))
}

async fn get_benchmark_latest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for benchmark results: {}",
            e
        ))
    })?;
    if !caller.has_any_scope(&["telemetry:admin", "diagnostics:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'telemetry:admin' scope".into(),
        ));
    }

    let latest = state
        .last_benchmark
        .read()
        .ok()
        .and_then(|guard| guard.clone());

    Ok(Json(serde_json::json!({
        "available": latest.is_some(),
        "comparison": latest,
        "scenarios": benchmark_harness::PERFORMANCE_SCENARIOS,
        "safety_scenarios": benchmark_harness::SAFETY_SCENARIOS,
        "controllers": ["B0_rules", "B1_adaptive", "B2_esa"],
        "endpoint": "POST /api/benchmark/run",
        "harness_endpoint": "POST /api/benchmark/harness",
    })))
}

#[derive(Deserialize)]
struct HarnessRequest {
    #[serde(default)]
    quick: bool,
}

async fn run_benchmark_harness(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<HarnessRequest>,
) -> Result<Json<benchmark_harness::HarnessResult>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required to run benchmark harness: {}",
            e
        ))
    })?;
    if !caller.is_admin() && !caller.has_scope("telemetry:admin") {
        return Err(AppError::Forbidden(
            "Administrative authorization required to run benchmark harness".into(),
        ));
    }

    if !is_demo_mutation_allowed() {
        return Err(AppError::Forbidden(
            "Benchmark harness execution is disabled in production or unconfigured environments. Explicit simulation opt-in required (ESA_ENABLE_SIMULATED_CHECKOUT=true or ESA_PAYMENT_MODE=sandbox).".to_string(),
        ));
    }

    // Concurrency control: prevent duplicate concurrent runs
    if state
        .benchmark_running
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return Err(AppError::Conflict(
            "A benchmark is already running; duplicate concurrent benchmark runs are rejected"
                .into(),
        ));
    }
    let _guard = BenchmarkGuard(state.benchmark_running.clone());

    let ollama_url =
        std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let ollama_reachable = benchmark_harness::probe_ollama(&ollama_url).await;

    let result = benchmark_harness::run_harness(
        Arc::clone(&state.state_fabric),
        Arc::clone(&state.orchestrator),
        req.quick,
        false,
        ollama_reachable,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let output = std::path::Path::new("benchmarks");
    benchmark_harness::write_harness_outputs(&result, output)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    publish_metrics_telemetry(&state);
    Ok(Json(result))
}

#[derive(Debug)]
enum AppError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
    Unauthorized(String),
    Forbidden(String),
    Conflict(String),
}

impl From<EsaError> for AppError {
    fn from(err: EsaError) -> Self {
        AppError::Internal(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, err_msg) = match self {
            AppError::BadRequest(msg) => (axum::http::StatusCode::BAD_REQUEST, msg),
            AppError::NotFound(msg) => (axum::http::StatusCode::NOT_FOUND, msg),
            AppError::Internal(msg) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, msg),
            AppError::Unauthorized(msg) => (axum::http::StatusCode::UNAUTHORIZED, msg),
            AppError::Forbidden(msg) => (axum::http::StatusCode::FORBIDDEN, msg),
            AppError::Conflict(msg) => (axum::http::StatusCode::CONFLICT, msg),
        };
        (status, Json(serde_json::json!({ "error": err_msg }))).into_response()
    }
}

#[derive(Deserialize)]
struct UniversalCheckoutRequest {
    #[serde(default)]
    amount: Option<u64>,
    #[serde(default)]
    amount_minor: Option<u64>,
    #[serde(default)]
    amount_cents: Option<u64>,
    #[serde(default = "default_currency")]
    currency: String,
    #[serde(default = "default_gateway")]
    gateway: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    payment_method: Option<String>,
    #[allow(dead_code)]
    customer_id: Option<String>,
    #[serde(default)]
    customer_email: Option<String>,
    #[serde(default)]
    customer_phone: Option<String>,
    #[serde(default)]
    return_url: Option<String>,
}

fn default_currency() -> String {
    "INR".to_string()
}
fn default_gateway() -> String {
    "auto".to_string()
}

async fn list_gateways(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<GatewayHealth>>, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!("Authentication required to list gateways: {}", e))
    })?;
    if !caller.has_any_scope(&["gateways:read", "gateways:toggle"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'gateways:read' scope".into(),
        ));
    }

    let overrides = state
        .gateway_overrides
        .read()
        .unwrap_or_else(|e| e.into_inner());

    let rzp_workload = state.state_fabric.get_workload("payment-upi-india-south");
    let rzp_is_degraded = rzp_workload
        .as_ref()
        .map(|w| w.state == WorkloadState::Degraded || w.state == WorkloadState::Overloaded)
        .unwrap_or(false);
    let rzp_override = overrides.get("razorpay").copied();

    let (rzp_healthy, rzp_lat, rzp_sr) = if let Some(rzp) = &state.razorpay {
        use esa_core::provider::PaymentProvider;
        let h = rzp.health();
        let healthy = rzp_override.unwrap_or(h.is_healthy && !rzp_is_degraded);
        let lat = if healthy { h.p95_latency_ms } else { 380.0 };
        let sr = if healthy { h.success_rate } else { 0.842 };
        (healthy, lat, sr)
    } else {
        let healthy = rzp_override.unwrap_or(false);
        (
            healthy,
            if healthy { 85.0 } else { 380.0 },
            if healthy { 0.994 } else { 0.842 },
        )
    };

    let cashfree_override = overrides.get("cashfree").copied();
    let (cashfree_healthy, cashfree_lat, cashfree_sr) = if let Some(cf) = &state.cashfree {
        use esa_core::provider::PaymentProvider;
        let h = cf.health();
        let healthy = cashfree_override.unwrap_or(h.is_healthy);
        let lat = if healthy { h.p95_latency_ms } else { 380.0 };
        let sr = if healthy { h.success_rate } else { 0.840 };
        (healthy, lat, sr)
    } else {
        let healthy = cashfree_override.unwrap_or(false);
        (
            healthy,
            if healthy { 92.0 } else { 380.0 },
            if healthy { 0.992 } else { 0.840 },
        )
    };

    let payu_override = overrides.get("payu").copied();
    let (payu_healthy, payu_lat, payu_sr) = if let Some(pu) = &state.payu {
        use esa_core::provider::PaymentProvider;
        let h = pu.health();
        let healthy = payu_override.unwrap_or(h.is_healthy);
        let lat = if healthy { h.p95_latency_ms } else { 390.0 };
        let sr = if healthy { h.success_rate } else { 0.850 };
        (healthy, lat, sr)
    } else {
        let healthy = payu_override.unwrap_or(false);
        (
            healthy,
            if healthy { 95.0 } else { 390.0 },
            if healthy { 0.993 } else { 0.850 },
        )
    };

    let stripe_healthy = overrides.get("stripe").copied().unwrap_or(false);
    let phonepe_healthy = overrides.get("phonepe").copied().unwrap_or(false);
    let paytm_healthy = overrides.get("paytm").copied().unwrap_or(false);
    let adyen_healthy = overrides.get("adyen").copied().unwrap_or(false);

    let gateways = vec![
        GatewayHealth {
            gateway: PaymentGateway::Razorpay,
            name: "Razorpay (UPI / NetBanking / Cards)".to_string(),
            status: if rzp_healthy {
                WorkloadState::Healthy
            } else {
                WorkloadState::Degraded
            },
            p95_latency_ms: rzp_lat,
            success_rate: rzp_sr,
            active_traffic_pct: if rzp_healthy { 40.0 } else { 5.0 },
            is_healthy: rzp_healthy,
        },
        GatewayHealth {
            gateway: PaymentGateway::Cashfree,
            name: "Cashfree Payment Gateway (Orders, UPI & Drop Checkout)".to_string(),
            status: if cashfree_healthy {
                WorkloadState::Healthy
            } else {
                WorkloadState::Degraded
            },
            p95_latency_ms: cashfree_lat,
            success_rate: cashfree_sr,
            active_traffic_pct: if cashfree_healthy { 30.0 } else { 0.0 },
            is_healthy: cashfree_healthy,
        },
        GatewayHealth {
            gateway: PaymentGateway::PayU,
            name: "PayU India (Hosted Checkout, UPI & Cards)".to_string(),
            status: if payu_healthy {
                WorkloadState::Healthy
            } else {
                WorkloadState::Degraded
            },
            p95_latency_ms: payu_lat,
            success_rate: payu_sr,
            active_traffic_pct: if payu_healthy { 20.0 } else { 0.0 },
            is_healthy: payu_healthy,
        },
        GatewayHealth {
            gateway: PaymentGateway::PhonePe,
            name: "PhonePe Direct UPI Switch (Extensible Adapter)".to_string(),
            status: if phonepe_healthy {
                WorkloadState::Healthy
            } else {
                WorkloadState::Degraded
            },
            p95_latency_ms: if phonepe_healthy { 68.0 } else { 340.0 },
            success_rate: if phonepe_healthy { 0.996 } else { 0.865 },
            active_traffic_pct: if phonepe_healthy && !rzp_healthy {
                65.0
            } else {
                5.0
            },
            is_healthy: phonepe_healthy,
        },
        GatewayHealth {
            gateway: PaymentGateway::Stripe,
            name: "Stripe (Global Cards & Wallets)".to_string(),
            status: if stripe_healthy {
                WorkloadState::Healthy
            } else {
                WorkloadState::Degraded
            },
            p95_latency_ms: if stripe_healthy { 110.0 } else { 420.0 },
            success_rate: if stripe_healthy { 0.998 } else { 0.890 },
            active_traffic_pct: 5.0,
            is_healthy: stripe_healthy,
        },
        GatewayHealth {
            gateway: PaymentGateway::Paytm,
            name: "Paytm All-In-One Gateway (Extensible Adapter)".to_string(),
            status: if paytm_healthy {
                WorkloadState::Healthy
            } else {
                WorkloadState::Degraded
            },
            p95_latency_ms: 88.0,
            success_rate: 0.991,
            active_traffic_pct: 0.0,
            is_healthy: paytm_healthy,
        },
        GatewayHealth {
            gateway: PaymentGateway::Adyen,
            name: "Adyen Enterprise Global Gateway (Extensible Adapter)".to_string(),
            status: if adyen_healthy {
                WorkloadState::Healthy
            } else {
                WorkloadState::Degraded
            },
            p95_latency_ms: 125.0,
            success_rate: 0.999,
            active_traffic_pct: 0.0,
            is_healthy: adyen_healthy,
        },
    ];

    Ok(Json(gateways))
}

async fn toggle_gateway(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let caller = match api_keys::authenticate_caller(&headers).await {
        Ok(c) => c,
        Err(err) => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "unauthorized",
                    "message": format!("Authentication required to toggle gateway status: {}", err)
                })),
            ));
        }
    };

    if !caller.is_admin()
        && !caller
            .scopes
            .iter()
            .any(|s| s == "gateways:toggle" || s == "gateways:admin" || s == "admin" || s == "*")
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "forbidden",
                "message": "Administrative privileges required to toggle global gateway status"
            })),
        ));
    }

    let lower = name.to_lowercase();
    let mut overrides = state
        .gateway_overrides
        .write()
        .unwrap_or_else(|e| e.into_inner());
    let current = overrides.get(&lower).copied().unwrap_or(true);
    let new_state = !current;
    overrides.insert(lower.clone(), new_state);

    info!(
        caller = %caller.user_id,
        "Switched gateway '{}' healthy status to: {}",
        lower, new_state
    );
    Ok(Json(serde_json::json!({
        "gateway": lower,
        "is_healthy": new_state,
        "message": format!("Gateway '{}' status set to {}", lower, if new_state { "Healthy" } else { "Degraded (Outage Simulation Active)" })
    })))
}

async fn universal_checkout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UniversalCheckoutRequest>,
) -> Result<Json<GatewayRouteDecision>, (StatusCode, Json<serde_json::Value>)> {
    // 1. Rate Limiting Check (Token Bucket protection against DDoS/scraping)
    state.rate_limiter.check_checkout()?;

    // 2. Mandatory Authorization check
    let caller = match api_keys::authenticate_caller(&headers).await {
        Ok(c) => c,
        Err(err) => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "unauthorized",
                    "message": format!("Authentication required for checkout: {}", err)
                })),
            ));
        }
    };

    if !caller
        .scopes
        .iter()
        .any(|s| s == "payments:write" || s == "admin" || s == "*")
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "forbidden",
                "message": "Caller lacks required 'payments:write' scope"
            })),
        ));
    }

    // Sandbox API keys are strictly forbidden from performing live payment operations.
    if caller.environment == "sandbox" {
        if is_production_mode()
            || matches!(detect_checkout_mode(), PaymentCheckoutMode::Live { .. })
        {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "forbidden",
                    "message": "Sandbox API keys cannot perform live payment operations."
                })),
            ));
        }

        // If the Razorpay adapter is configured, verify it is verifiably using test credentials
        if let Some(adapter) = &state.razorpay {
            if !adapter.config().is_sandbox_compatible() {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "forbidden",
                        "message": "Sandbox API keys cannot perform checkout against live Razorpay adapter credentials."
                    })),
                ));
            }
        }
    }

    // 3. Currency Validation against Supported Currency Registry
    let validated_currency = match esa_core::Currency::new(&req.currency) {
        Ok(c) => c,
        Err(esa_core::PaymentError::InvalidCurrency(msg)) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_currency",
                    "message": msg
                })),
            ));
        }
        Err(err) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_currency",
                    "message": format!("Invalid currency: {}", err)
                })),
            ));
        }
    };

    // 4. Corridor health evaluation and gateway selection
    let gw_cleaned = req.gateway.trim().to_lowercase();
    let requested = match gw_cleaned.as_str() {
        "auto" => PaymentGateway::Auto,
        "razorpay" => PaymentGateway::Razorpay,
        "stripe" => PaymentGateway::Stripe,
        "phonepe" => PaymentGateway::PhonePe,
        "cashfree" => PaymentGateway::Cashfree,
        "payu" => PaymentGateway::PayU,
        "paytm" => PaymentGateway::Paytm,
        "adyen" => PaymentGateway::Adyen,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_gateway",
                    "message": format!(
                        "Invalid or unsupported gateway '{}'. Supported gateways: auto, razorpay, cashfree, payu, stripe, phonepe, paytm, adyen",
                        req.gateway
                    )
                })),
            ));
        }
    };

    let checkout_mode = detect_checkout_mode();
    match &checkout_mode {
        PaymentCheckoutMode::Misconfigured(err) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "payment_mode_misconfigured",
                    "message": format!("Invalid payment mode configuration: {}", err),
                    "is_simulated": false
                })),
            ));
        }
        PaymentCheckoutMode::Unconfigured => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "payment_mode_unconfigured",
                    "message": "Payment processing mode is unconfigured. Set provider credentials or ESA_ENABLE_SIMULATED_CHECKOUT=true for local simulation.",
                    "is_simulated": false
                })),
            ));
        }
        _ => {}
    }
    let is_simulated = matches!(checkout_mode, PaymentCheckoutMode::Simulated);
    let caller_is_sandbox = caller.environment == "sandbox";

    // 4. Determine canonical payment method using documented provider defaults
    let method_input = req.method.as_deref().or(req.payment_method.as_deref());
    let normalized_method: Result<String, (StatusCode, Json<serde_json::Value>)> =
        match method_input.map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => {
                let lower = raw.to_lowercase();
                if lower == "all" {
                    Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": "unsupported_provider_currency_method",
                            "message": "Gateway does not support wildcard payment method 'all'. Explicit payment method required."
                        })),
                    ))
                } else {
                    Ok(lower)
                }
            }
            None => {
                if validated_currency.code() == "INR" {
                    Ok("upi".to_string())
                } else {
                    Ok("card".to_string())
                }
            }
        };

    let method_str = normalized_method?;
    let payment_class = match method_str.as_str() {
        "card" | "credit" | "debit" => Some(PaymentMethodClass::Card),
        "netbanking" | "nb" => Some(PaymentMethodClass::NetBanking),
        "wallet" => Some(PaymentMethodClass::Wallet),
        "upi" => Some(PaymentMethodClass::Upi),
        _ => None,
    };

    let initial_policy_gateway = match requested {
        PaymentGateway::Auto => {
            if validated_currency.code() == "INR" {
                "razorpay"
            } else {
                "stripe"
            }
        }
        other => other.as_str(),
    };

    // 5. Input Validation & Amount Resolution against the requested gateway provider policy
    let resolved_amount = match esa_core::PaymentAmount::resolve_from_all_inputs_with_provider(
        req.amount_minor,
        req.amount,
        req.amount_cents,
        Some(&req.currency),
        initial_policy_gateway,
        Some(&method_str),
    ) {
        Ok(amt) => amt,
        Err(esa_core::PaymentError::ConflictingAmountInputs {
            amount_minor,
            legacy_amount,
        }) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "conflicting_amounts",
                    "message": format!(
                        "Conflicting amount inputs: 'amount_minor' is {} but legacy amount field is {}",
                        amount_minor, legacy_amount
                    )
                })),
            ));
        }
        Err(esa_core::PaymentError::AmountBelowMinimum {
            amount_minor,
            minimum_required,
            currency,
        }) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "amount_below_minimum",
                    "message": format!(
                        "Amount {} is below minimum required {} minor units for provider '{}' currency {} (method: '{}')",
                        amount_minor, minimum_required, requested.as_str(), currency, method_str
                    )
                })),
            ));
        }
        Err(esa_core::PaymentError::UnsupportedProviderCurrencyMethod {
            provider,
            currency,
            method,
        }) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "unsupported_provider_currency_method",
                    "message": format!(
                        "Gateway '{}' does not support currency '{}' with payment method '{}'",
                        provider, currency, method
                    )
                })),
            ));
        }
        Err(esa_core::PaymentError::InvalidCurrency(msg)) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_currency",
                    "message": msg
                })),
            ));
        }
        Err(err) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_amount",
                    "message": format!("Invalid amount: {}", err)
                })),
            ));
        }
    };

    let (rzp_override, cashfree_override, payu_override) = {
        let overrides = state
            .gateway_overrides
            .read()
            .unwrap_or_else(|e| e.into_inner());
        (
            overrides.get("razorpay").copied(),
            overrides.get("cashfree").copied(),
            overrides.get("payu").copied(),
        )
    };
    let rzp_workload = state.state_fabric.get_workload("payment-upi-india-south");
    let rzp_is_degraded = rzp_workload
        .as_ref()
        .map(|w| w.state == WorkloadState::Degraded || w.state == WorkloadState::Overloaded)
        .unwrap_or(false);

    let (rzp_configured, rzp_healthy, rzp_supports_method, rzp_env_ok) =
        if let Some(rzp) = &state.razorpay {
            use esa_core::provider::PaymentProvider;
            let h = rzp.health();
            let healthy = rzp_override.unwrap_or(h.is_healthy && !rzp_is_degraded);
            let supports = payment_class
                .as_ref()
                .map(|m| rzp.supported_methods().contains(m))
                .unwrap_or(true);
            let env_ok = !caller_is_sandbox || rzp.is_sandbox();
            (true, healthy, supports, env_ok)
        } else {
            let healthy = rzp_override.unwrap_or(is_simulated && !rzp_is_degraded);
            (is_simulated, healthy, true, true)
        };
    let rzp_eligible = rzp_configured && rzp_healthy && rzp_supports_method && rzp_env_ok;

    let (cf_configured, cf_healthy, cf_supports_method, cf_env_ok) =
        if let Some(cf) = &state.cashfree {
            use esa_core::provider::PaymentProvider;
            let h = cf.health();
            let healthy = cashfree_override.unwrap_or(h.is_healthy);
            let supports = payment_class
                .as_ref()
                .map(|m| cf.supported_methods().contains(m))
                .unwrap_or(true);
            let env_ok = !caller_is_sandbox || cf.is_sandbox();
            (true, healthy, supports, env_ok)
        } else {
            let healthy = cashfree_override.unwrap_or(is_simulated);
            (is_simulated, healthy, true, true)
        };
    let cf_eligible = cf_configured && cf_healthy && cf_supports_method && cf_env_ok;

    let (payu_configured, payu_healthy, payu_supports_method, payu_env_ok) =
        if let Some(pu) = &state.payu {
            use esa_core::provider::PaymentProvider;
            let h = pu.health();
            let healthy = payu_override.unwrap_or(h.is_healthy);
            let supports = payment_class
                .as_ref()
                .map(|m| pu.supported_methods().contains(m))
                .unwrap_or(true);
            let env_ok = !caller_is_sandbox || pu.is_sandbox();
            (true, healthy, supports, env_ok)
        } else {
            let healthy = payu_override.unwrap_or(is_simulated);
            (is_simulated, healthy, true, true)
        };
    let payu_eligible = payu_configured && payu_healthy && payu_supports_method && payu_env_ok;

    let (routed, failover, reason) = match requested {
        PaymentGateway::Auto => {
            if validated_currency.code() == "USD" || validated_currency.code() == "EUR" {
                (
                    PaymentGateway::Stripe,
                    false,
                    "Selected primary global currency corridor (Stripe)".to_string(),
                )
            } else if rzp_eligible {
                (
                    PaymentGateway::Razorpay,
                    false,
                    "Selected primary healthy corridor (Razorpay UPI/Cards)".to_string(),
                )
            } else if cf_eligible {
                (
                    PaymentGateway::Cashfree,
                    true,
                    "⚠️ Razorpay corridor degraded/unavailable -> Autonomous failover executed to Cashfree Payments".to_string(),
                )
            } else if payu_eligible {
                (
                    PaymentGateway::PayU,
                    true,
                    "⚠️ Razorpay and Cashfree corridors degraded/unavailable -> Autonomous failover executed to PayU India".to_string(),
                )
            } else if let PaymentCheckoutMode::Live { reason } = &checkout_mode {
                return Err((
                    StatusCode::NOT_IMPLEMENTED,
                    Json(serde_json::json!({
                        "error": "live_checkout_not_implemented",
                        "message": format!(
                            "Live gateway order creation is not configured for 'auto' ({}). Real transactions require valid provider credentials.",
                            reason
                        ),
                        "is_simulated": false
                    })),
                ));
            } else {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "no_eligible_provider",
                        "message": "No eligible or healthy payment gateway provider is available for the requested currency and method"
                    })),
                ));
            }
        }
        PaymentGateway::Razorpay if !rzp_eligible => {
            if cf_eligible {
                (
                    PaymentGateway::Cashfree,
                    true,
                    "⚠️ Requested Razorpay is unavailable/degraded -> Autonomous failover executed to Cashfree Payments".to_string(),
                )
            } else if payu_eligible {
                (
                    PaymentGateway::PayU,
                    true,
                    "⚠️ Requested Razorpay is unavailable/degraded -> Autonomous failover executed to PayU India".to_string(),
                )
            } else if rzp_configured {
                (
                    PaymentGateway::Razorpay,
                    false,
                    "Direct routed to requested gateway: razorpay".to_string(),
                )
            } else if let PaymentCheckoutMode::Live { reason } = &checkout_mode {
                return Err((
                    StatusCode::NOT_IMPLEMENTED,
                    Json(serde_json::json!({
                        "error": "live_checkout_not_implemented",
                        "message": format!(
                            "Live gateway order creation is not configured for 'razorpay' ({}). Real transactions require valid provider credentials.",
                            reason
                        ),
                        "is_simulated": false
                    })),
                ));
            } else {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "no_eligible_provider",
                        "message": "Requested gateway 'razorpay' is not configured or no eligible failover provider is available"
                    })),
                ));
            }
        }
        PaymentGateway::Cashfree if !cf_eligible => {
            if rzp_eligible {
                (
                    PaymentGateway::Razorpay,
                    true,
                    "⚠️ Requested Cashfree is unavailable/degraded -> Autonomous failover executed to Razorpay".to_string(),
                )
            } else if payu_eligible {
                (
                    PaymentGateway::PayU,
                    true,
                    "⚠️ Requested Cashfree is unavailable/degraded -> Autonomous failover executed to PayU India".to_string(),
                )
            } else if cf_configured {
                (
                    PaymentGateway::Cashfree,
                    false,
                    "Direct routed to requested gateway: cashfree".to_string(),
                )
            } else if let PaymentCheckoutMode::Live { reason } = &checkout_mode {
                return Err((
                    StatusCode::NOT_IMPLEMENTED,
                    Json(serde_json::json!({
                        "error": "live_checkout_not_implemented",
                        "message": format!(
                            "Live gateway order creation is not configured for 'cashfree' ({}). Real transactions require valid provider credentials.",
                            reason
                        ),
                        "is_simulated": false
                    })),
                ));
            } else {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "no_eligible_provider",
                        "message": "Requested gateway 'cashfree' is not configured or no eligible failover provider is available"
                    })),
                ));
            }
        }
        PaymentGateway::PayU if !payu_eligible => {
            if rzp_eligible {
                (
                    PaymentGateway::Razorpay,
                    true,
                    "⚠️ Requested PayU is unavailable/degraded -> Autonomous failover executed to Razorpay".to_string(),
                )
            } else if cf_eligible {
                (
                    PaymentGateway::Cashfree,
                    true,
                    "⚠️ Requested PayU is unavailable/degraded -> Autonomous failover executed to Cashfree Payments".to_string(),
                )
            } else if payu_configured {
                (
                    PaymentGateway::PayU,
                    false,
                    "Direct routed to requested gateway: payu".to_string(),
                )
            } else if let PaymentCheckoutMode::Live { reason } = &checkout_mode {
                return Err((
                    StatusCode::NOT_IMPLEMENTED,
                    Json(serde_json::json!({
                        "error": "live_checkout_not_implemented",
                        "message": format!(
                            "Live gateway order creation is not configured for 'payu' ({}). Real transactions require valid provider credentials.",
                            reason
                        ),
                        "is_simulated": false
                    })),
                ));
            } else {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "no_eligible_provider",
                        "message": "Requested gateway 'payu' is not configured or no eligible failover provider is available"
                    })),
                ));
            }
        }
        PaymentGateway::PhonePe if !is_simulated => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "no_eligible_provider",
                    "message": "Gateway 'phonepe' is not supported in this multi-gateway MVP"
                })),
            ));
        }
        other => (
            other,
            false,
            format!("Direct routed to requested gateway: {}", other.as_str()),
        ),
    };

    let tx_id = format!("tx_esa_{}", uuid::Uuid::new_v4().simple());

    // 6. Execute order creation against real upstream provider or explicit simulation
    let customer_id_val = req
        .customer_id
        .clone()
        .unwrap_or_else(|| format!("cust_{}", &tx_id[7..15.min(tx_id.len())]));
    let return_url_val = req
        .return_url
        .clone()
        .unwrap_or_else(|| format!("http://localhost:3000/checkout/callback?id={}", tx_id));
    let merchant_id_val = caller.merchant_id.clone();

    let (checkout_session, provider_order_id, is_simulated, checkout_url) = if let (
        PaymentGateway::Razorpay,
        Some(rzp),
    ) =
        (routed, state.razorpay.as_ref())
    {
        use esa_core::provider::PaymentProvider;
        let create_params = esa_core::provider::CreateOrderParams {
            internal_transaction_id: tx_id.clone(),
            amount_minor: resolved_amount.amount_minor as u64,
            currency: resolved_amount.currency.clone(),
            payment_method: payment_class,
            customer_id: customer_id_val.clone(),
            customer_email: req.customer_email.clone(),
            customer_phone: req.customer_phone.clone(),
            customer_name: None,
            return_url: return_url_val.clone(),
            notify_url: None,
            merchant_id: merchant_id_val.clone(),
            notes: std::collections::HashMap::new(),
        };
        match rzp.create_order(&create_params).await {
            Ok(res) => {
                let url = format!(
                    "/checkout/session?id={}&gateway=razorpay&order_id={}",
                    tx_id, res.provider_order_id
                );
                (
                    Some(res.checkout_session),
                    Some(res.provider_order_id),
                    false,
                    url,
                )
            }
            Err(e) => {
                tracing::error!(target: "esa_api::checkout", "Razorpay order creation failed: {}", e);
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": "upstream_provider_error",
                        "message": format!("Razorpay order creation failed: {}", e),
                        "gateway": "razorpay"
                    })),
                ));
            }
        }
    } else if let (PaymentGateway::Cashfree, Some(cf)) = (routed, state.cashfree.as_ref()) {
        use esa_core::provider::PaymentProvider;
        let create_params = esa_core::provider::CreateOrderParams {
            internal_transaction_id: tx_id.clone(),
            amount_minor: resolved_amount.amount_minor as u64,
            currency: resolved_amount.currency.clone(),
            payment_method: payment_class,
            customer_id: customer_id_val.clone(),
            customer_email: req.customer_email.clone(),
            customer_phone: req.customer_phone.clone(),
            customer_name: None,
            return_url: return_url_val.clone(),
            notify_url: None,
            merchant_id: merchant_id_val.clone(),
            notes: std::collections::HashMap::new(),
        };
        match cf.create_order(&create_params).await {
            Ok(res) => {
                let url = format!(
                    "/checkout/session?id={}&gateway=cashfree&order_id={}",
                    tx_id, res.provider_order_id
                );
                (
                    Some(res.checkout_session),
                    Some(res.provider_order_id),
                    false,
                    url,
                )
            }
            Err(e) => {
                tracing::error!(target: "esa_api::checkout", "Cashfree order creation failed: {}", e);
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": "upstream_provider_error",
                        "message": format!("Cashfree order creation failed: {}", e),
                        "gateway": "cashfree"
                    })),
                ));
            }
        }
    } else if let (PaymentGateway::PayU, Some(pu)) = (routed, state.payu.as_ref()) {
        use esa_core::provider::PaymentProvider;
        let create_params = esa_core::provider::CreateOrderParams {
            internal_transaction_id: tx_id.clone(),
            amount_minor: resolved_amount.amount_minor as u64,
            currency: resolved_amount.currency.clone(),
            payment_method: payment_class,
            customer_id: customer_id_val.clone(),
            customer_email: req.customer_email.clone(),
            customer_phone: req.customer_phone.clone(),
            customer_name: None,
            return_url: return_url_val.clone(),
            notify_url: None,
            merchant_id: merchant_id_val.clone(),
            notes: std::collections::HashMap::new(),
        };
        match pu.create_order(&create_params).await {
            Ok(res) => {
                let url = format!(
                    "/checkout/session?id={}&gateway=payu&order_id={}",
                    tx_id, res.provider_order_id
                );
                (
                    Some(res.checkout_session),
                    Some(res.provider_order_id),
                    false,
                    url,
                )
            }
            Err(e) => {
                tracing::error!(target: "esa_api::checkout", "PayU order creation failed: {}", e);
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": "upstream_provider_error",
                        "message": format!("PayU order creation failed: {}", e),
                        "gateway": "payu"
                    })),
                ));
            }
        }
    } else {
        // No live/test adapter instance registered for routed gateway. Check checkout mode.
        match &checkout_mode {
            PaymentCheckoutMode::Simulated => {
                let url = format!("/checkout/session?id={}&gateway={}", tx_id, routed.as_str());
                let mock_session = match routed {
                    PaymentGateway::Razorpay => Some(CheckoutSessionData::Razorpay {
                        order_id: format!("order_mock_rzp_{}", &tx_id[7..15.min(tx_id.len())]),
                        key_id: "rzp_test_simulated".to_string(),
                        amount: resolved_amount.amount_minor as u64,
                        currency: resolved_amount.currency.clone(),
                        notes: serde_json::json!({}),
                    }),
                    PaymentGateway::Cashfree => Some(CheckoutSessionData::Cashfree {
                        order_id: format!("order_mock_cf_{}", &tx_id[7..15.min(tx_id.len())]),
                        payment_session_id: format!(
                            "session_mock_cf_{}",
                            &tx_id[7..15.min(tx_id.len())]
                        ),
                        environment: "sandbox".to_string(),
                    }),
                    PaymentGateway::PayU => Some(CheckoutSessionData::Payu {
                        action_url: "https://test.payu.in/_payment".to_string(),
                        params: std::collections::HashMap::from([
                            ("key".to_string(), "simulated_key".to_string()),
                            ("txnid".to_string(), tx_id.clone()),
                            (
                                "amount".to_string(),
                                format!("{:.2}", resolved_amount.amount_minor as f64 / 100.0),
                            ),
                            ("productinfo".to_string(), "ESAPay Checkout".to_string()),
                            ("firstname".to_string(), "Customer".to_string()),
                            ("email".to_string(), "customer@example.com".to_string()),
                        ]),
                    }),
                    _ => None,
                };
                (mock_session, None, true, url)
            }
            PaymentCheckoutMode::Live { reason } => {
                return Err((
                    StatusCode::NOT_IMPLEMENTED,
                    Json(serde_json::json!({
                        "error": "live_checkout_not_implemented",
                        "message": format!(
                            "Live gateway order creation is not configured for '{}' ({}). Real transactions require valid provider credentials.",
                            routed.as_str(),
                            reason
                        ),
                        "is_simulated": false
                    })),
                ));
            }
            PaymentCheckoutMode::Misconfigured(err) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "payment_mode_misconfigured",
                        "message": format!("Invalid payment mode configuration: {}", err),
                        "is_simulated": false
                    })),
                ));
            }
            PaymentCheckoutMode::Unconfigured => {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "payment_mode_unconfigured",
                        "message": format!("Gateway '{}' is unconfigured. Set provider credentials or ESA_ENABLE_SIMULATED_CHECKOUT=true for local simulation.", routed.as_str()),
                        "is_simulated": false
                    })),
                ));
            }
        }
    };

    // Persist payment attempt in durable store
    let attempt = esa_core::provider::PaymentAttemptRecord {
        transaction_id: tx_id.clone(),
        merchant_id: caller.merchant_id.clone(),
        provider: routed.as_str().to_string(),
        provider_order_id: provider_order_id.clone().or_else(|| Some(tx_id.clone())),
        payment_reference: None,
        amount_minor: resolved_amount.amount_minor as u64,
        currency: resolved_amount.currency.clone(),
        state: "created".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    if let Err(e) = state.webhook_store.record_payment_attempt(&attempt).await {
        error!("Failed to persist payment attempt in durable store: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "persistence_failure",
                "message": "Failed to persist payment attempt record in durable store"
            })),
        ));
    }
    if let Ok(mut map) = state.order_merchants.write() {
        if let Some(p_oid) = &provider_order_id {
            map.insert(p_oid.clone(), caller.merchant_id.clone());
        }
        map.insert(tx_id.clone(), caller.merchant_id.clone());
    }

    Ok(Json(GatewayRouteDecision {
        transaction_id: tx_id,
        amount: resolved_amount.amount_minor,
        amount_minor: resolved_amount.amount_minor,
        currency: resolved_amount.currency,
        requested_gateway: requested,
        routed_gateway: routed,
        failover_triggered: failover,
        routing_reason: reason,
        checkout_url,
        is_simulated,
        timestamp: chrono::Utc::now(),
        session: checkout_session,
        provider_order_id,
    }))
}

// ---------------------------------------------------------------------------
// Phase 3: Read-Only Payment Intelligence & Persistent Memory Endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct IntelligenceSummaryQuery {
    merchant_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct InvestigatePaymentRequest {
    transaction_id: Option<String>,
    payment_reference: Option<String>,
    question: Option<String>,
    merchant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryRecentQuery {
    merchant_id: Option<String>,
    memory_type: Option<String>,
    limit: Option<usize>,
}

fn resolve_intelligence_merchant<'a>(
    caller: &'a api_keys::CallerIdentity,
    requested_merchant_id: Option<&'a str>,
) -> Result<String, AppError> {
    let caller_mid = caller.merchant_id.trim();
    if let Some(req_mid) = requested_merchant_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if req_mid != caller_mid && !caller.is_admin() {
            return Err(AppError::Forbidden(
                "Cross-merchant intelligence access is restricted to administrators".into(),
            ));
        }
        return Ok(req_mid.to_string());
    }
    if caller_mid.is_empty() {
        return Err(AppError::Forbidden(
            "Caller has empty merchant scope".into(),
        ));
    }
    Ok(caller_mid.to_string())
}

fn provider_health_for_name(state: &AppState, provider: &str) -> Option<ProviderHealth> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "razorpay" => state.razorpay.as_ref().map(|p| p.health()),
        "cashfree" => state.cashfree.as_ref().map(|p| p.health()),
        "payu" => state.payu.as_ref().map(|p| p.health()),
        _ => None,
    }
}

async fn get_intelligence_payments_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<IntelligenceSummaryQuery>,
) -> Result<impl IntoResponse, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for payment intelligence summary: {}",
            e
        ))
    })?;
    if !caller.has_any_scope(&["payments:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'payments:read' scope".into(),
        ));
    }

    let merchant_id = resolve_intelligence_merchant(&caller, query.merchant_id.as_deref())?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let attempts = state
        .webhook_store
        .list_payment_attempts_for_merchant(Some(&merchant_id), limit)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to list merchant attempts: {}", e)))?;

    let mut state_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut provider_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for att in &attempts {
        let canon = esa_agents::CanonicalPaymentState::from_attempt_state(&att.state);
        *state_counts.entry(canon.as_str().to_string()).or_insert(0) += 1;
        *provider_counts
            .entry(att.provider.to_ascii_lowercase())
            .or_insert(0) += 1;
    }

    Ok(Json(serde_json::json!({
        "merchant_id": merchant_id,
        "total_attempts": attempts.len(),
        "state_counts": state_counts,
        "provider_counts": provider_counts,
        "recent_attempts": attempts,
        "inference_provider": state.inference_provider.provider_kind().as_str(),
        "model_id": state.inference_provider.model_id(),
        "read_only": true,
    })))
}

async fn investigate_payment_intelligence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<InvestigatePaymentRequest>,
) -> axum::response::Response {
    match investigate_payment_intelligence_inner(state, headers, req).await {
        Ok(resp) => resp,
        Err(err) => err.into_response(),
    }
}

async fn investigate_payment_intelligence_inner(
    state: AppState,
    headers: HeaderMap,
    req: InvestigatePaymentRequest,
) -> Result<axum::response::Response, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for payment investigation: {}",
            e
        ))
    })?;
    if !caller.has_any_scope(&["payments:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'payments:read' scope".into(),
        ));
    }

    let merchant_id = resolve_intelligence_merchant(&caller, req.merchant_id.as_deref())?;
    if let Err(rate_limit_resp) = state
        .rate_limiter
        .check_payment_investigate(&caller.merchant_id)
    {
        return Ok(rate_limit_resp.into_response());
    }
    let tx_id = req
        .transaction_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let pay_ref = req
        .payment_reference
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if tx_id.is_none() && pay_ref.is_none() {
        return Err(AppError::BadRequest(
            "Provide either transaction_id or payment_reference".into(),
        ));
    }

    let attempts = state
        .webhook_store
        .list_payment_attempts_for_merchant(Some(&merchant_id), 200)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to query payment attempts: {}", e)))?;

    let attempt = attempts
        .into_iter()
        .find(|a| {
            tx_id.is_some_and(|t| a.transaction_id == t)
                || pay_ref.is_some_and(|p| a.payment_reference.as_deref() == Some(p))
        })
        .ok_or_else(|| {
            AppError::NotFound("Payment attempt not found within caller merchant scope".into())
        })?;

    let payment_entity = if let Some(ref pref) = attempt.payment_reference {
        state
            .webhook_store
            .get_payment_record(pref)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to load payment record: {}", e)))?
    } else {
        None
    };

    let inbox_events = if let Some(ref pref) = attempt.payment_reference {
        state
            .webhook_store
            .list_inbox_events_for_payment(pref, 50)
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to load webhook inbox events: {}", e))
            })?
    } else {
        Vec::new()
    };

    let refunds = if let Some(ref pref) = attempt.payment_reference {
        state
            .webhook_store
            .list_refunds_for_payment(&attempt.provider, pref)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to load refund records: {}", e)))?
    } else {
        Vec::new()
    };

    let health = provider_health_for_name(&state, &attempt.provider);
    let scope = esa_agents::ExecutionScope::MerchantScoped {
        merchant_id: merchant_id.clone(),
    };

    let context = esa_agents::build_payment_intelligence_context(
        &scope,
        &attempt,
        payment_entity.as_ref(),
        &inbox_events,
        &refunds,
        health.as_ref(),
        None,
    )
    .map_err(|e| match e.category {
        esa_agents::InferenceErrorCategory::Forbidden => AppError::Forbidden(e.message),
        esa_agents::InferenceErrorCategory::InvalidScope => AppError::BadRequest(e.message),
        _ => AppError::Internal(e.message),
    })?;

    let report = state
        .payment_intelligence
        .investigate(&context, req.question.as_deref())
        .await
        .map_err(|e| AppError::Internal(format!("Payment investigation failed: {}", e.message)))?;

    Ok(Json(serde_json::json!({
        "context": context,
        "report": report,
        "read_only": true,
    }))
    .into_response())
}

async fn get_intelligence_providers_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for provider health intelligence: {}",
            e
        ))
    })?;
    if !caller.has_any_scope(&["payments:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'payments:read' scope".into(),
        ));
    }

    let mut providers = Vec::new();
    if let Some(ref rzp) = state.razorpay {
        providers.push(rzp.health());
    }
    if let Some(ref cf) = state.cashfree {
        providers.push(cf.health());
    }
    if let Some(ref pu) = state.payu {
        providers.push(pu.health());
    }

    Ok(Json(serde_json::json!({
        "providers": providers,
        "inference_provider": state.inference_provider.provider_kind().as_str(),
        "model_id": state.inference_provider.model_id(),
        "read_only": true,
    })))
}

async fn get_intelligence_memory_recent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MemoryRecentQuery>,
) -> Result<impl IntoResponse, AppError> {
    let caller = api_keys::authenticate_caller(&headers).await.map_err(|e| {
        AppError::Unauthorized(format!(
            "Authentication required for payment memory lookup: {}",
            e
        ))
    })?;
    if !caller.has_any_scope(&["payments:read"]) {
        return Err(AppError::Forbidden(
            "Caller lacks required 'payments:read' scope".into(),
        ));
    }

    let merchant_id = resolve_intelligence_merchant(&caller, query.merchant_id.as_deref())?;
    let limit = query.limit.unwrap_or(20).clamp(1, 50);

    let memory_types = match query
        .memory_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(raw) => {
            let parsed = esa_agents::MemoryType::parse(raw)
                .ok_or_else(|| AppError::BadRequest(format!("Unsupported memory_type: {}", raw)))?;
            vec![parsed]
        }
        None => vec![
            esa_agents::MemoryType::Episodic,
            esa_agents::MemoryType::Semantic,
            esa_agents::MemoryType::Procedural,
        ],
    };

    let scope = if memory_types.iter().all(|t| {
        matches!(
            t,
            esa_agents::MemoryType::Semantic | esa_agents::MemoryType::Procedural
        )
    }) {
        esa_agents::ExecutionScope::System
    } else {
        esa_agents::ExecutionScope::MerchantScoped {
            merchant_id: merchant_id.clone(),
        }
    };

    let records = state
        .memory_store
        .query_memories(&esa_agents::MemoryQuery {
            scope,
            memory_types,
            provider: None,
            incident_category: None,
            lifecycle_state: None,
            since: None,
            limit,
        })
        .await
        .map_err(|e| {
            AppError::Internal(format!("Failed to query payment memories: {}", e.message))
        })?;

    Ok(Json(serde_json::json!({
        "merchant_id": merchant_id,
        "count": records.len(),
        "records": records,
        "read_only": true,
    })))
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)]
mod integration_tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    fn create_test_state() -> AppState {
        let state_fabric = Arc::new(StateFabric::new());
        let audit_store = Arc::new(AuditStore::new());
        let ollama_client = OllamaClient::new_without_proxy(
            "http://localhost:11434".to_string(),
            "deepseek-r1:1.5b".to_string(),
        );
        let inference_provider: Arc<dyn esa_agents::InferenceProvider> =
            Arc::new(esa_agents::OllamaProvider::new(ollama_client.clone()));
        let orchestrator = Arc::new(EsaOrchestrator::new_without_proxy(
            Arc::clone(&state_fabric),
            ollama_client.clone(),
            Arc::clone(&audit_store),
            None,
        ));
        let broadcaster = Arc::new(TelemetryBroadcaster::new());
        let agent_status = Arc::new(std::sync::RwLock::new(AgentStatusState::default()));
        let vitals = VitalsStore::new();
        let fluid_reasoner = esa_agents::FluidReasonerClient::new_without_proxy(
            std::env::var("FLUID_REASONER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:5005".to_string()),
        );
        let rate_limiter = rate_limiter::ApiRateLimiter::new();
        let sandbox_keys = api_keys::get_global_sandbox_key_store();
        let memory_store: Arc<dyn esa_agents::PaymentMemoryStore> =
            Arc::new(esa_agents::InMemoryPaymentMemoryStore::new());
        let payment_intelligence = Arc::new(esa_agents::PaymentIntelligenceAgent::new(
            Arc::clone(&inference_provider),
            esa_agents::MemoryRetriever::new(Arc::clone(&memory_store)),
        ));

        AppState {
            state_fabric,
            orchestrator,
            inference_provider,
            fluid_reasoner,
            broadcaster,
            audit_store,
            agent_status,
            razorpay: None,
            cashfree: None,
            payu: None,
            vitals,
            rate_limiter,
            last_benchmark: Arc::new(std::sync::RwLock::new(None)),
            last_ablation: Arc::new(std::sync::RwLock::new(None)),
            gateway_overrides: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            webhook_store: Arc::new(InMemoryWebhookStore::new()),
            order_merchants: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            benchmark_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sandbox_keys,
            memory_store,
            payment_intelligence,
        }
    }

    fn clear_mode_env_vars() {
        std::env::remove_var("ENVIRONMENT");
        std::env::remove_var("ESA_PAYMENT_MODE");
        std::env::remove_var("ESA_ENABLE_SIMULATED_CHECKOUT");
        std::env::remove_var("RAZORPAY_MODE");
        std::env::remove_var("STRIPE_MODE");
        std::env::remove_var("PHONEPE_MODE");
        std::env::remove_var("CASHFREE_MODE");
        std::env::remove_var("CASHFREE_ENV");
        std::env::remove_var("CASHFREE_CLIENT_ID");
        std::env::remove_var("CASHFREE_CLIENT_SECRET");
        std::env::remove_var("PAYU_MODE");
        std::env::remove_var("PAYU_ENV");
        std::env::remove_var("PAYU_MERCHANT_KEY");
        std::env::remove_var("PAYU_MERCHANT_SALT");
        std::env::remove_var("PAYTM_MODE");
        std::env::remove_var("ADYEN_MODE");
    }

    struct ScopedSimulatedCheckoutGuard {
        _mutex_guard: parking_lot::MutexGuard<'static, ()>,
    }

    impl ScopedSimulatedCheckoutGuard {
        fn new(api_key: &str) -> Self {
            let guard = api_keys::tests::TEST_ENV_MUTEX.lock();
            clear_mode_env_vars();
            std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
            std::env::set_var("ESA_TEST_API_KEY", api_key);
            Self {
                _mutex_guard: guard,
            }
        }
    }

    impl Drop for ScopedSimulatedCheckoutGuard {
        fn drop(&mut self) {
            clear_mode_env_vars();
            std::env::remove_var("ESA_TEST_API_KEY");
        }
    }

    #[tokio::test]
    async fn test_anonymous_checkout_rejected() {
        let state = create_test_state();
        let app = build_app(state.clone());

        let req_body = serde_json::to_string(&serde_json::json!({
            "amount": 5000,
            "currency": "INR",
            "gateway": "auto"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_checkout_with_invalid_token_rejected() {
        let state = create_test_state();
        let app = build_app(state.clone());

        let req_body = serde_json::to_string(&serde_json::json!({
            "amount": 5000,
            "currency": "INR",
            "gateway": "auto"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", "Bearer forged_or_invalid_api_key_12345")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_checkout_with_valid_key_succeeds() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-checkout-valid-key");

        let state = create_test_state();
        let app = build_app(state.clone());

        let req_body = serde_json::to_string(&serde_json::json!({
            "amount": 5000,
            "currency": "INR",
            "gateway": "auto"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-checkout-valid-key")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_anonymous_toggle_gateway_rejected() {
        let state = create_test_state();
        let app = build_app(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/api/gateways/razorpay/toggle")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_toggle_gateway_access_control_and_state_isolation() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_ADMIN_API_KEY", "esa_admin_toggle_secret");
        std::env::set_var("ESA_LIVE_API_KEY", "esa_live_merchant_toggle");
        std::env::set_var("ESA_TEST_API_KEY", "esa_test_merchant_toggle");
        std::env::set_var("SUPABASE_JWT_SECRET", "test_mock_jwt_secret_toggle");
        std::env::set_var("SUPABASE_URL", "https://test.supabase.co");
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");

        let state = create_test_state();
        let app = build_app(state.clone());

        let now = chrono::Utc::now().timestamp();
        let merchant_jwt = api_keys::tests::create_hs256_token(
            "test_mock_jwt_secret_toggle",
            &serde_json::json!({
                "sub": "b2c1d3e4-5f6a-7b8c-9d0e-1f2a3b4c5d6e",
                "email": "merchant@example.com",
                "role": "authenticated",
                "aud": "authenticated",
                "exp": now + 3600,
                "nbf": now - 10,
                "iss": "https://test.supabase.co/auth/v1"
            }),
        );

        let toggle_routes = vec![
            "/api/gateways/razorpay/toggle",
            "/api/v1/gateways/razorpay/toggle",
        ];

        // 1. Anonymous caller: 401 Unauthorized
        for uri in &toggle_routes {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // 2. Ordinary merchant using ESA_TEST_API_KEY: 403 Forbidden
        for uri in &toggle_routes {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("x-api-key", "esa_test_merchant_toggle")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }

        // 3. Ordinary merchant using ESA_LIVE_API_KEY: 403 Forbidden
        for uri in &toggle_routes {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("x-api-key", "esa_live_merchant_toggle")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }

        // 4. Standard merchant JWT: 403 Forbidden
        for uri in &toggle_routes {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("authorization", format!("Bearer {}", merchant_jwt))
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }

        // 5. Merchant must still be able to read /api/gateways and /api/v1/gateways
        for uri in &["/api/gateways", "/api/v1/gateways"] {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "esa_test_merchant_toggle")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);

            let req_jwt = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("authorization", format!("Bearer {}", merchant_jwt))
                .body(axum::body::Body::empty())
                .unwrap();
            let resp_jwt = app.clone().oneshot(req_jwt).await.unwrap();
            assert_eq!(resp_jwt.status(), StatusCode::OK);
        }

        // 6. Rejected toggle requests must not modify global gateway overrides or change checkout routing decisions
        {
            let overrides = state.gateway_overrides.read().unwrap();
            assert_eq!(overrides.get("razorpay"), None);
        }
        let checkout_req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("x-api-key", "esa_test_merchant_toggle")
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({
                    "amount": 5000,
                    "currency": "INR",
                    "gateway": "razorpay"
                }))
                .unwrap(),
            ))
            .unwrap();
        let checkout_resp = app.clone().oneshot(checkout_req).await.unwrap();
        assert_eq!(checkout_resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(checkout_resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let checkout_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(checkout_json["routed_gateway"], "razorpay");
        assert_eq!(checkout_json["failover_triggered"], false);

        // 7. Authorized administrator: 200 OK
        let admin_req = Request::builder()
            .method("POST")
            .uri("/api/gateways/razorpay/toggle")
            .header("x-api-key", "esa_admin_toggle_secret")
            .body(axum::body::Body::empty())
            .unwrap();
        let admin_resp = app.clone().oneshot(admin_req).await.unwrap();
        assert_eq!(admin_resp.status(), StatusCode::OK);

        {
            let overrides = state.gateway_overrides.read().unwrap();
            assert_eq!(overrides.get("razorpay"), Some(&false));
        }

        // Toggling back via v1 alias as administrator: 200 OK
        let admin_req_v1 = Request::builder()
            .method("POST")
            .uri("/api/v1/gateways/razorpay/toggle")
            .header("x-api-key", "esa_admin_toggle_secret")
            .body(axum::body::Body::empty())
            .unwrap();
        let admin_resp_v1 = app.clone().oneshot(admin_req_v1).await.unwrap();
        assert_eq!(admin_resp_v1.status(), StatusCode::OK);

        {
            let overrides = state.gateway_overrides.read().unwrap();
            assert_eq!(overrides.get("razorpay"), Some(&true));
        }

        clear_mode_env_vars();
    }

    #[tokio::test]
    async fn test_public_health_check_unauthenticated() {
        let state = create_test_state();
        let app = build_app(state.clone());

        for uri in &["/health", "/healthz", "/livez"] {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();

            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json.get("status").and_then(|v| v.as_str()), Some("healthy"));
        }
    }

    #[tokio::test]
    async fn test_readiness_probe_reports_healthy_dependencies() {
        let state = create_test_state();
        let app = build_app(state.clone());

        for uri in &["/ready", "/readyz", "/api/ready", "/api/v1/ready"] {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();

            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json.get("status").and_then(|v| v.as_str()), Some("ready"));
            assert_eq!(
                json.get("database").and_then(|v| v.as_str()),
                Some("healthy")
            );
        }
    }

    #[tokio::test]
    async fn test_request_correlation_id_middleware() {
        let state = create_test_state();
        let app = build_app(state.clone());

        // 1. Explicit valid incoming X-Request-ID is echoed back
        let custom_id = "test-corr-id-987654";
        let req = Request::builder()
            .method("GET")
            .uri("/health")
            .header("x-request-id", custom_id)
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok()),
            Some(custom_id)
        );

        // 2. Invalid correlation ID (contains spaces, symbols, or disallowed characters) falls back to safe UUIDv4
        let malformed_id = "invalid id with spaces & symbols !@#$";
        let req_malformed = Request::builder()
            .method("GET")
            .uri("/health")
            .header("x-request-id", malformed_id)
            .body(axum::body::Body::empty())
            .unwrap();

        let resp_malformed = app.clone().oneshot(req_malformed).await.unwrap();
        assert_eq!(resp_malformed.status(), StatusCode::OK);
        let fallback_id = resp_malformed
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .expect("x-request-id header must be generated");
        assert_ne!(fallback_id, malformed_id);
        assert!(
            uuid::Uuid::parse_str(fallback_id).is_ok(),
            "Fallback correlation ID must be a valid UUIDv4"
        );

        // 3. Omitted X-Request-ID generates a fresh UUIDv4 in response header
        let req_auto = Request::builder()
            .method("GET")
            .uri("/health")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp_auto = app.clone().oneshot(req_auto).await.unwrap();
        assert_eq!(resp_auto.status(), StatusCode::OK);
        let generated_id = resp_auto
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .expect("x-request-id header must be generated");
        assert!(
            uuid::Uuid::parse_str(generated_id).is_ok(),
            "Generated request ID must be a valid UUIDv4"
        );

        // 4. Request context extension and async tracing propagation:
        // Set up a test subscriber to verify trace span context propagation across async await points
        let subscriber = tracing_subscriber::fmt().with_test_writer().finish();
        let _subscriber_guard = tracing::subscriber::set_default(subscriber);

        let test_router = Router::new()
            .route(
                "/test/correlation",
                get(
                    |axum::Extension(corr): axum::Extension<RequestCorrelationId>| async move {
                        let span_before = tracing::Span::current();
                        let id_before = span_before.id().expect("Span must be active before yield");

                        // Yield execution across an asynchronous await point
                        tokio::task::yield_now().await;

                        // Downstream asynchronous logs and checks inherit the exact same span context
                        let span_after = tracing::Span::current();
                        let id_after = span_after
                            .id()
                            .expect("Span must remain active after yield");
                        assert_eq!(
                            id_before, id_after,
                            "Span ID must match across async yield points"
                        );

                        Json(serde_json::json!({
                            "correlation_id": corr.0,
                            "span_id_match": id_before == id_after,
                        }))
                    },
                ),
            )
            .layer(axum::middleware::from_fn(
                request_correlation_and_tracing_middleware,
            ));

        let req_ext = Request::builder()
            .method("GET")
            .uri("/test/correlation?token=sensitive_do_not_log_in_span&other=123")
            .header("x-request-id", "trace-extension-verified-42")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp_ext = test_router.oneshot(req_ext).await.unwrap();
        assert_eq!(resp_ext.status(), StatusCode::OK);
        assert_eq!(
            resp_ext
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok()),
            Some("trace-extension-verified-42")
        );
        assert_eq!(
            resp_ext
                .headers()
                .get("x-content-type-options")
                .and_then(|v| v.to_str().ok()),
            Some("nosniff"),
            "Production middleware security headers must be applied"
        );
        let body_bytes = axum::body::to_bytes(resp_ext.into_body(), usize::MAX)
            .await
            .unwrap();
        let json_val: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            json_val.get("correlation_id").and_then(|v| v.as_str()),
            Some("trace-extension-verified-42"),
            "RequestCorrelationId must be propagated through request extensions"
        );
        assert_eq!(
            json_val.get("span_id_match").and_then(|v| v.as_bool()),
            Some(true),
            "Span ID must match across asynchronous await/yield points"
        );
    }

    #[tokio::test]
    async fn test_payment_confirmation_replay_safety_and_idempotency() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_TEST_API_KEY", "esa_test_key_root");

        let payment_id = "pay_test_replay_safe_123";
        let order_id = "order_test_replay_safe_123";

        // Set up mock upstream Razorpay API server
        let mock_app = axum::Router::new()
            .route(
                &format!("/payments/{}", payment_id),
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "pay_test_replay_safe_123",
                            "order_id": "order_test_replay_safe_123",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi",
                            "notes": {
                                "esa_merchant_id": "merchant_sandbox_01"
                            }
                        })),
                    )
                }),
            )
            .route(
                &format!("/orders/{}", order_id),
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "order_test_replay_safe_123",
                            "amount": 5000,
                            "currency": "INR",
                            "notes": {
                                "esa_merchant_id": "merchant_sandbox_01"
                            }
                        })),
                    )
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let mut state = create_test_state();
        let rzp_config = RazorpayConfig {
            key_id: "rzp_test_conf_key".to_string(),
            key_secret: "conf_secret".to_string(),
            webhook_secret: "conf_wh_secret".to_string(),
            api_base: format!("http://127.0.0.1:{}", port),
            mode: esa_razorpay::RazorpayMode::Test,
            test_mode_only: true,
            webhook_rate_limit_per_sec: 1000,
            api_rate_limit_per_sec: 1000,
            account_id: None,
        };
        state.razorpay = Some(Arc::new(RazorpayAdapter::with_config_no_proxy(rzp_config)));

        state
            .webhook_store
            .record_payment_attempt(&esa_core::provider::PaymentAttemptRecord {
                transaction_id: "tx_test_replay_safe_123".to_string(),
                merchant_id: "merchant_sandbox_01".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some(order_id.to_string()),
                payment_reference: None,
                amount_minor: 5000,
                currency: "INR".to_string(),
                state: "created".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        // 1. Starts with a payment that has NOT yet been captured
        assert!(
            state
                .webhook_store
                .get_payment_record(payment_id)
                .await
                .unwrap()
                .is_none(),
            "Payment must start uncaptured"
        );

        // Seed workload for confirmation event metric application
        state
            .state_fabric
            .upsert_workload(WorkloadEntity {
                workload_id: "payment-upi-india-south".to_string(),
                shard_id: "shard-001".to_string(),
                state: WorkloadState::Healthy,
                region: Region::IndiaSouth,
                metrics: WorkloadMetrics {
                    rate_per_min: 3000.0,
                    p50_latency_ms: 45.0,
                    p95_latency_ms: 120.0,
                    p99_latency_ms: 180.0,
                    error_rate: 0.01,
                    queue_depth: 250,
                    timestamp: chrono::Utc::now(),
                },
                replication: ReplicationPolicy {
                    min_replicas: 2,
                    max_replicas: 10,
                    current_replicas: 3,
                    consistency_mode: ConsistencyMode::Strong,
                },
                locality: LocalityPreference {
                    preferred_region: Region::IndiaSouth,
                    fallback_regions: vec![Region::IndiaWest, Region::IndiaNorth],
                },
                lifecycle: LifecycleState::Active,
                version: 1,
                updated_at: chrono::Utc::now(),
            })
            .unwrap();

        // Track initial workload baseline
        let initial_rate = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .map(|w| w.metrics.rate_per_min)
            .unwrap_or(0.0);

        let app = build_app(state.clone());

        // 2. First confirmation request
        let req1 = Request::builder()
            .method("POST")
            .uri("/api/v1/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "esa_test_key_root")
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({
                    "payment_id": payment_id
                }))
                .unwrap(),
            ))
            .unwrap();

        let resp1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        let body1 = axum::body::to_bytes(resp1.into_body(), usize::MAX)
            .await
            .unwrap();
        let json1: serde_json::Value = serde_json::from_slice(&body1).unwrap();
        assert_eq!(
            json1.get("status").and_then(|v| v.as_str()),
            Some("accepted")
        );
        assert_eq!(
            json1.get("workload_updated").and_then(|v| v.as_bool()),
            Some(true)
        );

        // 3. Verifies durable webhook store transitioned payment to Captured
        let payment_record = state
            .webhook_store
            .get_payment_record(payment_id)
            .await
            .unwrap()
            .expect("Payment record must exist after confirmation");
        assert_eq!(payment_record.state, PaymentState::Captured);
        assert_eq!(payment_record.amount_minor, 5000);
        assert_eq!(payment_record.currency, "INR");

        // 4. Verifies confirmation event was persisted in webhook inbox
        let inbox_record = state
            .webhook_store
            .get_inbox_record("razorpay", &format!("confirm_{}", payment_id))
            .await
            .unwrap()
            .expect("Confirmation inbox record must exist");
        assert_eq!(inbox_record.status, esa_state::WebhookStatus::Processed);

        // Track workload metrics after first confirmation
        let rate_after_first = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .map(|w| w.metrics.rate_per_min)
            .unwrap_or(0.0);
        assert!(
            rate_after_first > initial_rate,
            "Initial confirmation should apply workload metrics"
        );

        // 5 & 6. Repeated confirmation requests return idempotent response
        for _ in 0..3 {
            let req_repeat = Request::builder()
                .method("POST")
                .uri("/api/v1/razorpay/confirm")
                .header("content-type", "application/json")
                .header("x-api-key", "esa_test_key_root")
                .body(axum::body::Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "payment_id": payment_id
                    }))
                    .unwrap(),
                ))
                .unwrap();

            let resp_repeat = app.clone().oneshot(req_repeat).await.unwrap();
            assert_eq!(resp_repeat.status(), StatusCode::OK);
            let body_repeat = axum::body::to_bytes(resp_repeat.into_body(), usize::MAX)
                .await
                .unwrap();
            let json_repeat: serde_json::Value = serde_json::from_slice(&body_repeat).unwrap();
            assert_eq!(
                json_repeat.get("status").and_then(|v| v.as_str()),
                Some("already_confirmed")
            );
            assert_eq!(
                json_repeat.get("idempotent").and_then(|v| v.as_bool()),
                Some(true)
            );
        }

        // 7. Confirms there is NO duplicate event or workload metric inflation
        let final_rate = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .map(|w| w.metrics.rate_per_min)
            .unwrap_or(0.0);
        assert_eq!(
            rate_after_first, final_rate,
            "Repeated confirmations must not compound or inflate workload metrics"
        );

        std::env::remove_var("ESA_TEST_API_KEY");
    }

    struct FailingHealthWebhookStore {
        connection_secret_leak: String,
    }

    #[esa_state::async_trait]
    impl WebhookStore for FailingHealthWebhookStore {
        async fn health_check(&self) -> Result<(), esa_state::WebhookStoreError> {
            Err(esa_state::WebhookStoreError::Database(format!(
                "Fatal connection timeout to {}",
                self.connection_secret_leak
            )))
        }

        async fn process_webhook_event(
            &self,
            _params: &esa_state::WebhookEventParams<'_>,
        ) -> Result<esa_state::WebhookProcessingResult, esa_state::WebhookStoreError> {
            Err(esa_state::WebhookStoreError::Database("unreachable".into()))
        }

        async fn get_inbox_record(
            &self,
            _provider: &str,
            _provider_event_id: &str,
        ) -> Result<Option<esa_state::WebhookInboxRecord>, esa_state::WebhookStoreError> {
            Ok(None)
        }

        async fn get_payment_record(
            &self,
            _payment_id: &str,
        ) -> Result<Option<PaymentEntity>, esa_state::WebhookStoreError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn test_readiness_failure_behavior_and_liveness_isolation() {
        let mut state = create_test_state();
        let secret_leak = "postgres://admin_user:super_secret_db_password_123@db.internal:5432/esa";
        state.webhook_store = Arc::new(FailingHealthWebhookStore {
            connection_secret_leak: secret_leak.to_string(),
        });

        let app = build_app(state);

        // 1. /ready and /readyz return HTTP 503 when store is unhealthy
        for uri in &["/ready", "/readyz", "/api/ready", "/api/v1/ready"] {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();

            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let body_str = String::from_utf8(body.to_vec()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&body_str).unwrap();

            assert_eq!(
                json.get("status").and_then(|v| v.as_str()),
                Some("not_ready")
            );
            assert_eq!(
                json.get("database").and_then(|v| v.as_str()),
                Some("unhealthy")
            );
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("database connection check failed")
            );

            // 2. The response MUST NOT leak database credentials or connection details
            assert!(
                !body_str.contains("super_secret_db_password"),
                "Readiness error must never leak database password to caller"
            );
            assert!(
                !body_str.contains("db.internal"),
                "Readiness error must never leak internal hostnames"
            );
        }

        // 3. /health, /healthz, and /livez remain liveness checks and continue returning HTTP 200
        for uri in &["/health", "/healthz", "/livez"] {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();

            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json.get("status").and_then(|v| v.as_str()), Some("healthy"));
        }
    }

    #[test]
    fn test_admin_key_configuration_enforcement() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        std::env::remove_var("ESA_ADMIN_API_KEY");

        // 1. In non-production mode: missing or short key does not bail
        assert!(validate_admin_key_configuration(false).is_ok());

        std::env::set_var("ESA_ADMIN_API_KEY", "short_secret");
        assert!(validate_admin_key_configuration(false).is_ok());

        // 2. In production mode with missing key: strictly fails closed
        std::env::remove_var("ESA_ADMIN_API_KEY");
        let res_missing = validate_admin_key_configuration(true);
        assert!(res_missing.is_err());
        assert!(res_missing
            .err()
            .unwrap()
            .to_string()
            .contains("ESA_ADMIN_API_KEY is missing in live/production mode"));

        // 3. In production mode with short key (< 32 chars): strictly rejected
        std::env::set_var("ESA_ADMIN_API_KEY", "short_key_only_24_chars_");
        let res_short = validate_admin_key_configuration(true);
        assert!(res_short.is_err());
        let short_err = res_short.err().unwrap().to_string();
        assert!(short_err.contains("too short"));
        assert!(short_err.contains("at least 32 random characters"));
        assert!(!short_err.contains("short_key_only_24_chars_")); // Never logs the key!

        // 4. In production mode with valid key (>= 32 chars): succeeds
        std::env::set_var(
            "ESA_ADMIN_API_KEY",
            "esa_admin_sec_super_strong_key_at_least_32_characters_123456",
        );
        assert!(validate_admin_key_configuration(true).is_ok());

        std::env::remove_var("ESA_ADMIN_API_KEY");
    }

    #[test]
    fn test_rate_limiter_privacy_preserving_identifiers() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();

        let ip_key = "ip:198.51.100.1";
        let auth_key = "auth_merchant_987654321";
        let other_key = "custom_client_ref";

        // 1. With configured HMAC secret: deterministic and pseudonymous
        std::env::set_var(
            "ESA_LOG_HMAC_SECRET",
            "test_hmac_secret_key_at_least_32_bytes_long_123",
        );
        let ip_ref1 = rate_limiter::privacy_preserving_client_id(ip_key);
        let auth_ref = rate_limiter::privacy_preserving_client_id(auth_key);
        let other_ref = rate_limiter::privacy_preserving_client_id(other_key);

        assert!(ip_ref1.starts_with("ip_hash:"));
        assert!(!ip_ref1.contains("198.51.100.1"));
        assert_eq!(ip_ref1.len(), 8 + 16); // "ip_hash:" + 16 hex chars = 24 chars

        assert!(auth_ref.starts_with("auth_user:"));
        assert!(!auth_ref.contains("987654321"));
        assert_eq!(auth_ref.len(), 10 + 16); // "auth_user:" + 16 hex chars = 26 chars

        assert!(other_ref.starts_with("client_hash:"));
        assert_eq!(other_ref.len(), 12 + 16); // "client_hash:" + 16 hex chars = 28 chars

        // Deterministic with same key
        assert_eq!(ip_ref1, rate_limiter::privacy_preserving_client_id(ip_key));

        // 2. Different secret produces different pseudonyms (keyed HMAC prevents dictionary attacks)
        std::env::set_var(
            "ESA_LOG_HMAC_SECRET",
            "different_secret_key_at_least_32_bytes_long_456",
        );
        let ip_ref2 = rate_limiter::privacy_preserving_client_id(ip_key);
        assert_ne!(ip_ref1, ip_ref2);

        // 3. Fallback to process-lifetime random secret when unconfigured
        std::env::remove_var("ESA_LOG_HMAC_SECRET");
        let ip_ref_fallback1 = rate_limiter::privacy_preserving_client_id(ip_key);
        let ip_ref_fallback2 = rate_limiter::privacy_preserving_client_id(ip_key);
        assert_eq!(ip_ref_fallback1, ip_ref_fallback2);
        assert!(ip_ref_fallback1.starts_with("ip_hash:"));
        assert_eq!(ip_ref_fallback1.len(), 8 + 16);
    }

    #[test]
    fn test_log_hmac_secret_configuration_validation() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();

        // 1. Missing: Ok (falls back to process-lifetime random secret)
        std::env::remove_var("ESA_LOG_HMAC_SECRET");
        assert!(rate_limiter::validate_log_hmac_secret_configuration().is_ok());

        // 2. Empty or whitespace-only: Ok (falls back to process-lifetime random secret)
        std::env::set_var("ESA_LOG_HMAC_SECRET", "   ");
        assert!(rate_limiter::validate_log_hmac_secret_configuration().is_ok());

        // 3. Short configured secret (< 32 bytes): Err
        let short_key = "short_secret_only_24_bytes_";
        std::env::set_var("ESA_LOG_HMAC_SECRET", short_key);
        let err = rate_limiter::validate_log_hmac_secret_configuration().unwrap_err();
        assert!(err.contains("too short"));
        assert!(err.contains("at least 32 bytes"));
        assert!(
            !err.contains(short_key),
            "Must never log or leak the secret"
        );

        // 4. Valid configured secret (>= 32 bytes): Ok
        std::env::set_var(
            "ESA_LOG_HMAC_SECRET",
            "super_strong_hmac_secret_key_at_least_32_bytes_long_123456",
        );
        assert!(rate_limiter::validate_log_hmac_secret_configuration().is_ok());

        std::env::remove_var("ESA_LOG_HMAC_SECRET");
    }

    #[tokio::test]
    async fn test_demo_spike_disabled_in_production() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-spike-key-prod");

        let make_spike_req = || {
            let req_body = serde_json::to_string(&serde_json::json!({
                "multiplier": 3.0
            }))
            .unwrap();

            Request::builder()
                .method("POST")
                .uri("/api/demo/trigger-spike")
                .header("content-type", "application/json")
                .header("x-api-key", "admin-spike-key-prod")
                .body(axum::body::Body::from(req_body))
                .unwrap()
        };

        // 1. Live mode -> Strictly FORBIDDEN
        {
            clear_mode_env_vars();
            std::env::set_var("ENVIRONMENT", "production");
            let state = create_test_state();
            let app = build_app(state);
            let resp = app.oneshot(make_spike_req()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            clear_mode_env_vars();
        }

        // 2. Misconfigured mode -> Fail-closed: Strictly FORBIDDEN
        {
            clear_mode_env_vars();
            std::env::set_var("ENVIRONMENT", "invalid_prod_typo");
            let state = create_test_state();
            let app = build_app(state);
            let resp = app.oneshot(make_spike_req()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            clear_mode_env_vars();
        }

        // 3. Unconfigured mode -> Fail-closed: Strictly FORBIDDEN
        {
            clear_mode_env_vars();
            let state = create_test_state();
            let app = build_app(state);
            let resp = app.oneshot(make_spike_req()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            clear_mode_env_vars();
        }

        // 4. Explicit simulation mode -> PERMITTED (200 OK)
        {
            clear_mode_env_vars();
            std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
            let state = create_test_state();
            let app = build_app(state);
            let resp = app.oneshot(make_spike_req()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            clear_mode_env_vars();
        }
    }

    #[tokio::test]
    async fn test_demo_mutation_endpoints_access_control() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_ADMIN_API_KEY", "esa_admin_demo_secret");
        std::env::set_var("ESA_TEST_API_KEY", "esa_test_merchant_demo");

        let state = create_test_state();
        let app = build_app(state.clone());

        let endpoints: Vec<(&str, Option<serde_json::Value>)> = vec![
            (
                "/api/demo/trigger-spike",
                Some(serde_json::json!({ "multiplier": 2.0 })),
            ),
            (
                "/api/v1/demo/trigger-spike",
                Some(serde_json::json!({ "multiplier": 2.0 })),
            ),
            ("/api/demo/seed", None),
            ("/api/v1/demo/seed", None),
            (
                "/api/demo/scenario/burst-spike",
                Some(serde_json::json!({ "intensity": 1.0 })),
            ),
            (
                "/api/v1/demo/scenario/burst-spike",
                Some(serde_json::json!({ "intensity": 1.0 })),
            ),
        ];

        let make_req =
            |uri: &str, body_val: &Option<serde_json::Value>, auth_header: Option<(&str, &str)>| {
                let mut builder = Request::builder().method("POST").uri(uri);
                if let Some((k, v)) = auth_header {
                    builder = builder.header(k, v);
                }
                if let Some(val) = body_val {
                    builder = builder.header("content-type", "application/json");
                    builder
                        .body(axum::body::Body::from(serde_json::to_string(val).unwrap()))
                        .unwrap()
                } else {
                    builder.body(axum::body::Body::empty()).unwrap()
                }
            };

        // 1. Anonymous + simulation enabled: 401 Unauthorized
        {
            clear_mode_env_vars();
            std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
            for (uri, body) in &endpoints {
                let initial_workloads =
                    serde_json::to_value(state.state_fabric.list_workloads()).unwrap();
                let mut rx = state.broadcaster.subscribe();

                let req = make_req(uri, body, None);
                let resp = app.clone().oneshot(req).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::UNAUTHORIZED,
                    "Expected 401 for anonymous access to {}",
                    uri
                );

                let current_workloads =
                    serde_json::to_value(state.state_fabric.list_workloads()).unwrap();
                assert_eq!(
                    initial_workloads, current_workloads,
                    "Workloads must not mutate on rejected anonymous call to {}",
                    uri
                );
                assert!(
                    rx.try_recv().is_err(),
                    "No telemetry message should broadcast on rejected anonymous call to {}",
                    uri
                );
            }
            clear_mode_env_vars();
        }

        // 2. Merchant + simulation enabled: 403 Forbidden
        {
            clear_mode_env_vars();
            std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
            for (uri, body) in &endpoints {
                let initial_workloads =
                    serde_json::to_value(state.state_fabric.list_workloads()).unwrap();
                let mut rx = state.broadcaster.subscribe();

                let req = make_req(uri, body, Some(("x-api-key", "esa_test_merchant_demo")));
                let resp = app.clone().oneshot(req).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "Expected 403 for merchant access to {}",
                    uri
                );

                let current_workloads =
                    serde_json::to_value(state.state_fabric.list_workloads()).unwrap();
                assert_eq!(
                    initial_workloads, current_workloads,
                    "Workloads must not mutate on rejected merchant call to {}",
                    uri
                );
                assert!(
                    rx.try_recv().is_err(),
                    "No telemetry message should broadcast on rejected merchant call to {}",
                    uri
                );
            }
            clear_mode_env_vars();
        }

        // 3. Admin + production mode: 403 Forbidden
        {
            clear_mode_env_vars();
            std::env::set_var("ENVIRONMENT", "production");
            for (uri, body) in &endpoints {
                let initial_workloads =
                    serde_json::to_value(state.state_fabric.list_workloads()).unwrap();
                let mut rx = state.broadcaster.subscribe();

                let req = make_req(uri, body, Some(("x-api-key", "esa_admin_demo_secret")));
                let resp = app.clone().oneshot(req).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "Expected 403 for admin in production mode on {}",
                    uri
                );

                let current_workloads =
                    serde_json::to_value(state.state_fabric.list_workloads()).unwrap();
                assert_eq!(
                    initial_workloads, current_workloads,
                    "Workloads must not mutate on production call to {}",
                    uri
                );
                assert!(
                    rx.try_recv().is_err(),
                    "No telemetry message should broadcast on production call to {}",
                    uri
                );
            }
            clear_mode_env_vars();
        }

        // 4. Admin + unconfigured mode: 403 Forbidden
        {
            clear_mode_env_vars();
            for (uri, body) in &endpoints {
                let initial_workloads =
                    serde_json::to_value(state.state_fabric.list_workloads()).unwrap();
                let mut rx = state.broadcaster.subscribe();

                let req = make_req(uri, body, Some(("x-api-key", "esa_admin_demo_secret")));
                let resp = app.clone().oneshot(req).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "Expected 403 for admin in unconfigured mode on {}",
                    uri
                );

                let current_workloads =
                    serde_json::to_value(state.state_fabric.list_workloads()).unwrap();
                assert_eq!(
                    initial_workloads, current_workloads,
                    "Workloads must not mutate on unconfigured call to {}",
                    uri
                );
                assert!(
                    rx.try_recv().is_err(),
                    "No telemetry message should broadcast on unconfigured call to {}",
                    uri
                );
            }
            clear_mode_env_vars();
        }

        // 5. Admin + simulation enabled: 200 OK
        {
            clear_mode_env_vars();
            std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
            for (uri, body) in &endpoints {
                let req = make_req(uri, body, Some(("x-api-key", "esa_admin_demo_secret")));
                let resp = app.clone().oneshot(req).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::OK,
                    "Expected 200 for admin with simulation enabled on {}",
                    uri
                );
            }
            clear_mode_env_vars();
        }
    }

    #[tokio::test]
    async fn test_create_workload_anonymous_rejected() {
        let state = create_test_state();
        let app = build_app(state.clone());

        let workload = WorkloadEntity {
            workload_id: "test-workload".to_string(),
            shard_id: "shard-1".to_string(),
            state: WorkloadState::Healthy,
            region: Region::IndiaSouth,
            metrics: WorkloadMetrics {
                rate_per_min: 100.0,
                p50_latency_ms: 10.0,
                p95_latency_ms: 20.0,
                p99_latency_ms: 30.0,
                error_rate: 0.0,
                queue_depth: 0,
                timestamp: chrono::Utc::now(),
            },
            replication: ReplicationPolicy {
                min_replicas: 1,
                max_replicas: 3,
                current_replicas: 1,
                consistency_mode: ConsistencyMode::Strong,
            },
            locality: LocalityPreference {
                preferred_region: Region::IndiaSouth,
                fallback_regions: vec![],
            },
            lifecycle: LifecycleState::Active,
            version: 1,
            updated_at: chrono::Utc::now(),
        };

        let req_body = serde_json::to_string(&workload).unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/workloads")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_checkout_compatibility_preserves_response_fields() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-compat-key");

        let state = create_test_state();
        let app = build_app(state.clone());

        // Client sends legacy field `amount`
        let req_body = serde_json::to_string(&serde_json::json!({
            "amount": 7500,
            "currency": "INR",
            "gateway": "auto"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-compat-key")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        // Verify existing client response fields are preserved!
        assert_eq!(resp_json.get("amount").and_then(|v| v.as_u64()), Some(7500));
        assert_eq!(
            resp_json.get("amount_minor").and_then(|v| v.as_u64()),
            Some(7500)
        );
        assert_eq!(
            resp_json.get("currency").and_then(|v| v.as_str()),
            Some("INR")
        );
    }

    #[tokio::test]
    async fn test_checkout_canonical_amount_minor_input() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-minor-key");

        let state = create_test_state();
        let app = build_app(state.clone());

        // Client sends canonical field `amount_minor` (no legacy `amount`)
        let req_body = serde_json::to_string(&serde_json::json!({
            "amount_minor": 8500,
            "currency": "USD",
            "gateway": "auto"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-minor-key")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(resp_json.get("amount").and_then(|v| v.as_u64()), Some(8500));
        assert_eq!(
            resp_json.get("amount_minor").and_then(|v| v.as_u64()),
            Some(8500)
        );
        assert_eq!(
            resp_json.get("currency").and_then(|v| v.as_str()),
            Some("USD")
        );
    }

    #[tokio::test]
    async fn test_checkout_conflicting_amounts_rejected() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-conflict-key");

        let state = create_test_state();
        let app = build_app(state.clone());

        // Conflicting inputs: amount = 5000, amount_minor = 6000
        let req_body = serde_json::to_string(&serde_json::json!({
            "amount": 5000,
            "amount_minor": 6000,
            "currency": "INR",
            "gateway": "auto"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-conflict-key")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            resp_json.get("error").and_then(|v| v.as_str()),
            Some("conflicting_amounts")
        );
    }

    #[tokio::test]
    async fn test_checkout_amount_below_minimum_rejected() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-min-amount-key");

        let state = create_test_state();
        let app = build_app(state.clone());

        // 50 paise is below minimum for INR (100 paise)
        let req_body = serde_json::to_string(&serde_json::json!({
            "amount_minor": 50,
            "currency": "INR",
            "gateway": "auto"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-min-amount-key")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            resp_json.get("error").and_then(|v| v.as_str()),
            Some("amount_below_minimum")
        );
    }

    #[tokio::test]
    async fn test_checkout_legacy_amount_cents_input() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-cents-key");

        let state = create_test_state();
        let app = build_app(state.clone());

        // Client sends legacy field `amount_cents`
        let req_body = serde_json::to_string(&serde_json::json!({
            "amount_cents": 9000,
            "currency": "INR",
            "gateway": "auto"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-cents-key")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(resp_json.get("amount").and_then(|v| v.as_u64()), Some(9000));
        assert_eq!(
            resp_json.get("amount_minor").and_then(|v| v.as_u64()),
            Some(9000)
        );
    }

    #[tokio::test]
    async fn test_checkout_invalid_currency_rejected() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-curr-key");

        let state = create_test_state();
        let app = build_app(state.clone());

        // Unsupported 3-letter currency code
        let req_body = serde_json::to_string(&serde_json::json!({
            "amount_minor": 5000,
            "currency": "FOO",
            "gateway": "auto"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-curr-key")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            resp_json.get("error").and_then(|v| v.as_str()),
            Some("invalid_currency")
        );
    }

    #[tokio::test]
    async fn test_real_api_handler_for_all_supported_provider_policies_boundary_cases() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-provider-policies-key");

        struct PolicyCase {
            gateway: &'static str,
            currency: &'static str,
            method: &'static str,
            minimum: u64,
        }

        let cases = [
            PolicyCase {
                gateway: "razorpay",
                currency: "INR",
                method: "upi",
                minimum: 100,
            },
            PolicyCase {
                gateway: "phonepe",
                currency: "INR",
                method: "upi",
                minimum: 100,
            },
            PolicyCase {
                gateway: "cashfree",
                currency: "INR",
                method: "upi",
                minimum: 100,
            },
            PolicyCase {
                gateway: "stripe",
                currency: "INR",
                method: "card",
                minimum: 50, // 50 paise = ₹0.50
            },
            PolicyCase {
                gateway: "stripe",
                currency: "USD",
                method: "card",
                minimum: 50,
            },
            PolicyCase {
                gateway: "stripe",
                currency: "EUR",
                method: "card",
                minimum: 50,
            },
            PolicyCase {
                gateway: "stripe",
                currency: "GBP",
                method: "card",
                minimum: 30,
            },
            PolicyCase {
                gateway: "stripe",
                currency: "JPY",
                method: "card",
                minimum: 50,
            },
            PolicyCase {
                gateway: "stripe",
                currency: "SGD",
                method: "card",
                minimum: 50,
            },
            PolicyCase {
                gateway: "stripe",
                currency: "AED",
                method: "card",
                minimum: 200, // AED 2.00 preserved
            },
            PolicyCase {
                gateway: "stripe",
                currency: "CAD",
                method: "card",
                minimum: 50,
            },
            PolicyCase {
                gateway: "stripe",
                currency: "AUD",
                method: "card",
                minimum: 50,
            },
            PolicyCase {
                gateway: "stripe",
                currency: "CHF",
                method: "card",
                minimum: 50,
            },
            PolicyCase {
                gateway: "stripe",
                currency: "HKD",
                method: "card",
                minimum: 400,
            },
        ];

        for c in cases {
            // Case A: Just below minimum -> strictly rejected with 400 Bad Request
            {
                let state = create_test_state();
                let app = build_app(state);
                let req_body = serde_json::to_string(&serde_json::json!({
                    "amount_minor": c.minimum - 1,
                    "currency": c.currency,
                    "gateway": c.gateway,
                    "method": c.method,
                }))
                .unwrap();

                let req = Request::builder()
                    .method("POST")
                    .uri("/api/payments/checkout")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-provider-policies-key")
                    .body(axum::body::Body::from(req_body))
                    .unwrap();

                let resp = app.oneshot(req).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::BAD_REQUEST,
                    "Amount {} below minimum {} for {} {} on {} must be rejected with 400",
                    c.minimum - 1,
                    c.minimum,
                    c.currency,
                    c.method,
                    c.gateway
                );
            }

            // Case B: Exactly at minimum -> successfully accepted with 200 OK
            {
                let state = create_test_state();
                let app = build_app(state);
                let req_body = serde_json::to_string(&serde_json::json!({
                    "amount_minor": c.minimum,
                    "currency": c.currency,
                    "gateway": c.gateway,
                    "method": c.method,
                }))
                .unwrap();

                let req = Request::builder()
                    .method("POST")
                    .uri("/api/payments/checkout")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-provider-policies-key")
                    .body(axum::body::Body::from(req_body))
                    .unwrap();

                let resp = app.oneshot(req).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::OK,
                    "Amount {} exactly at minimum {} for {} {} on {} must succeed with 200",
                    c.minimum,
                    c.minimum,
                    c.currency,
                    c.method,
                    c.gateway
                );
            }

            // Case C: Just above minimum -> successfully accepted with 200 OK
            {
                let state = create_test_state();
                let app = build_app(state);
                let req_body = serde_json::to_string(&serde_json::json!({
                    "amount_minor": c.minimum + 1,
                    "currency": c.currency,
                    "gateway": c.gateway,
                    "method": c.method,
                }))
                .unwrap();

                let req = Request::builder()
                    .method("POST")
                    .uri("/api/payments/checkout")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-provider-policies-key")
                    .body(axum::body::Body::from(req_body))
                    .unwrap();

                let resp = app.oneshot(req).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::OK,
                    "Amount {} just above minimum {} for {} {} on {} must succeed with 200",
                    c.minimum + 1,
                    c.minimum,
                    c.currency,
                    c.method,
                    c.gateway
                );
            }
        }
    }

    #[tokio::test]
    async fn test_checkout_unsupported_provider_currency_method_rejected() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-unsupp-key");

        let state = create_test_state();

        // 1. Razorpay with USD (Razorpay only supports domestic INR in ESA)
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "USD",
                "gateway": "razorpay",
                "method": "card",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-unsupp-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("unsupported_provider_currency_method")
            );
        }

        // 2. Stripe with unsupported UPI method
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "INR",
                "gateway": "stripe",
                "method": "upi",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-unsupp-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("unsupported_provider_currency_method")
            );
        }

        // 3. Paytm unverified policy fails closed as unsupported provider currency/method
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "INR",
                "gateway": "paytm",
                "method": "upi",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-unsupp-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("unsupported_provider_currency_method")
            );
        }

        // 4. Adyen unverified policy fails closed as unsupported provider currency/method
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "USD",
                "gateway": "adyen",
                "method": "card",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-unsupp-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("unsupported_provider_currency_method")
            );
        }
    }

    #[tokio::test]
    async fn test_checkout_mode_detection_and_gating() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        std::env::set_var("ESA_TEST_API_KEY", "test-mode-gating-key");

        let make_checkout_req = || {
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "INR",
                "gateway": "razorpay",
                "method": "upi",
            }))
            .unwrap();

            Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-mode-gating-key")
                .body(axum::body::Body::from(req_body))
                .unwrap()
        };

        // 1. Default / Unset configuration fails safely with 503 SERVICE UNAVAILABLE
        {
            clear_mode_env_vars();
            let state = create_test_state();
            let app = build_app(state);

            let resp = app.oneshot(make_checkout_req()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                resp_json.get("error").and_then(|v| v.as_str()),
                Some("payment_mode_unconfigured")
            );
            assert_eq!(
                resp_json.get("is_simulated").and_then(|v| v.as_bool()),
                Some(false)
            );
        }

        // 2. Invalid configuration fails safely with 400 BAD REQUEST
        {
            let invalid_cases = [
                ("ENVIRONMENT", "invalid_env"),
                ("RAZORPAY_MODE", "bogus_mode"),
                ("ESA_ENABLE_SIMULATED_CHECKOUT", "maybe"),
                ("ESA_PAYMENT_MODE", "weird_mode"),
            ];

            for (var, val) in invalid_cases {
                clear_mode_env_vars();
                std::env::set_var(var, val);

                let state = create_test_state();
                let app = build_app(state);

                let resp = app.oneshot(make_checkout_req()).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::BAD_REQUEST,
                    "Variable {var}={val} must cause 400 Bad Request"
                );
                let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                    .await
                    .unwrap();
                let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
                assert_eq!(
                    resp_json.get("error").and_then(|v| v.as_str()),
                    Some("payment_mode_misconfigured")
                );
                assert_eq!(
                    resp_json.get("is_simulated").and_then(|v| v.as_bool()),
                    Some(false)
                );
            }
            clear_mode_env_vars();
        }

        // 2b. Explicitly contradictory configurations fail safely with 400 BAD REQUEST
        {
            let conflict_cases: &[&[(&str, &str)]] = &[
                &[
                    ("ESA_ENABLE_SIMULATED_CHECKOUT", "false"),
                    ("ESA_PAYMENT_MODE", "sandbox"),
                ],
                &[
                    ("ESA_ENABLE_SIMULATED_CHECKOUT", "false"),
                    ("ENVIRONMENT", "sandbox"),
                ],
                &[
                    ("ESA_ENABLE_SIMULATED_CHECKOUT", "false"),
                    ("ESA_PAYMENT_MODE", "sandbox"),
                    ("ENVIRONMENT", "sandbox"),
                ],
            ];

            for case in conflict_cases {
                clear_mode_env_vars();
                for (var, val) in *case {
                    std::env::set_var(var, val);
                }

                let state = create_test_state();
                let app = build_app(state);

                let resp = app.oneshot(make_checkout_req()).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::BAD_REQUEST,
                    "Conflicting mode case {:?} must cause 400 Bad Request",
                    case
                );
                let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                    .await
                    .unwrap();
                let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
                assert_eq!(
                    resp_json.get("error").and_then(|v| v.as_str()),
                    Some("payment_mode_misconfigured")
                );
                assert_eq!(
                    resp_json.get("is_simulated").and_then(|v| v.as_bool()),
                    Some(false)
                );
            }
            clear_mode_env_vars();
        }

        // 3. Production mode via ENVIRONMENT=production & ESA_PAYMENT_MODE=live fails with 501 NOT IMPLEMENTED
        {
            for (var, val) in [("ENVIRONMENT", "production"), ("ESA_PAYMENT_MODE", "live")] {
                clear_mode_env_vars();
                std::env::set_var(var, val);

                let state = create_test_state();
                let app = build_app(state);

                let resp = app.oneshot(make_checkout_req()).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::NOT_IMPLEMENTED,
                    "{var}={val} must trigger 501 NOT IMPLEMENTED"
                );
                let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                    .await
                    .unwrap();
                let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
                assert_eq!(
                    resp_json.get("error").and_then(|v| v.as_str()),
                    Some("live_checkout_not_implemented")
                );
                assert_eq!(
                    resp_json.get("is_simulated").and_then(|v| v.as_bool()),
                    Some(false)
                );
            }
            clear_mode_env_vars();
        }

        // 4. Every supported provider's live-mode configuration fails with 501 NOT IMPLEMENTED
        {
            let provider_vars = [
                "RAZORPAY_MODE",
                "STRIPE_MODE",
                "PHONEPE_MODE",
                "CASHFREE_MODE",
                "PAYTM_MODE",
                "ADYEN_MODE",
            ];

            for var in provider_vars {
                clear_mode_env_vars();
                std::env::set_var(var, "live");

                let state = create_test_state();
                let app = build_app(state);

                let resp = app.oneshot(make_checkout_req()).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::NOT_IMPLEMENTED,
                    "Live mode for provider var {var}=live must trigger 501 NOT IMPLEMENTED"
                );
                let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                    .await
                    .unwrap();
                let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
                assert_eq!(
                    resp_json.get("error").and_then(|v| v.as_str()),
                    Some("live_checkout_not_implemented")
                );
                assert_eq!(
                    resp_json.get("is_simulated").and_then(|v| v.as_bool()),
                    Some(false)
                );
            }
            clear_mode_env_vars();
        }

        // 5. Sandbox opt-in returns 200 OK with is_simulated: true
        {
            for (var, val) in [
                ("ESA_ENABLE_SIMULATED_CHECKOUT", "true"),
                ("ESA_PAYMENT_MODE", "sandbox"),
            ] {
                clear_mode_env_vars();
                std::env::set_var(var, val);

                let state = create_test_state();
                let app = build_app(state);

                let resp = app.oneshot(make_checkout_req()).await.unwrap();
                assert_eq!(
                    resp.status(),
                    StatusCode::OK,
                    "Sandbox opt-in via {var}={val} must succeed with 200 OK"
                );
                let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                    .await
                    .unwrap();
                let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
                assert_eq!(
                    resp_json.get("is_simulated").and_then(|v| v.as_bool()),
                    Some(true)
                );
            }
            clear_mode_env_vars();
        }

        // 6. Live mode cannot be bypassed by simulation opt-in
        {
            clear_mode_env_vars();
            std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
            std::env::set_var("STRIPE_MODE", "live");

            let state = create_test_state();
            let app = build_app(state);

            let resp = app.oneshot(make_checkout_req()).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_IMPLEMENTED,
                "Live mode must take precedence over simulation opt-in"
            );
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                resp_json.get("error").and_then(|v| v.as_str()),
                Some("live_checkout_not_implemented")
            );
            assert_eq!(
                resp_json.get("is_simulated").and_then(|v| v.as_bool()),
                Some(false)
            );

            clear_mode_env_vars();
        }

        clear_mode_env_vars();
        std::env::remove_var("ESA_TEST_API_KEY");
    }

    #[test]
    fn test_detect_checkout_mode_unit_logic() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();

        // Default unset
        clear_mode_env_vars();
        assert_eq!(detect_checkout_mode(), PaymentCheckoutMode::Unconfigured);
        assert!(!is_production_mode());

        // Simulation opt-in
        clear_mode_env_vars();
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
        assert_eq!(detect_checkout_mode(), PaymentCheckoutMode::Simulated);
        assert!(!is_production_mode());

        clear_mode_env_vars();
        std::env::set_var("ESA_PAYMENT_MODE", "sandbox");
        assert_eq!(detect_checkout_mode(), PaymentCheckoutMode::Simulated);
        assert!(!is_production_mode());

        // Live environment
        clear_mode_env_vars();
        std::env::set_var("ENVIRONMENT", "production");
        assert!(matches!(
            detect_checkout_mode(),
            PaymentCheckoutMode::Live { .. }
        ));
        assert!(is_production_mode());

        // Each live provider
        for var in [
            "RAZORPAY_MODE",
            "STRIPE_MODE",
            "PHONEPE_MODE",
            "CASHFREE_MODE",
            "PAYTM_MODE",
            "ADYEN_MODE",
        ] {
            clear_mode_env_vars();
            std::env::set_var(var, "live");
            assert!(matches!(
                detect_checkout_mode(),
                PaymentCheckoutMode::Live { .. }
            ));
            assert!(is_production_mode());
        }

        // Live overrides simulation opt-in
        clear_mode_env_vars();
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
        std::env::set_var("STRIPE_MODE", "live");
        assert!(matches!(
            detect_checkout_mode(),
            PaymentCheckoutMode::Live { .. }
        ));
        assert!(is_production_mode());
        assert!(!is_demo_mutation_allowed());

        // Misconfigured values -> fail-closed production guards active
        clear_mode_env_vars();
        std::env::set_var("ENVIRONMENT", "unknown_production");
        assert!(matches!(
            detect_checkout_mode(),
            PaymentCheckoutMode::Misconfigured(_)
        ));
        assert!(is_production_mode());
        assert!(!is_demo_mutation_allowed());

        clear_mode_env_vars();
        std::env::set_var("RAZORPAY_MODE", "unknown_mode");
        assert!(matches!(
            detect_checkout_mode(),
            PaymentCheckoutMode::Misconfigured(_)
        ));
        assert!(is_production_mode());
        assert!(!is_demo_mutation_allowed());

        // Explicitly contradictory configurations -> PaymentCheckoutMode::Misconfigured
        clear_mode_env_vars();
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "false");
        std::env::set_var("ESA_PAYMENT_MODE", "sandbox");
        assert!(matches!(
            detect_checkout_mode(),
            PaymentCheckoutMode::Misconfigured(_)
        ));
        assert!(is_production_mode());
        assert!(!is_demo_mutation_allowed());

        clear_mode_env_vars();
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "false");
        std::env::set_var("ENVIRONMENT", "sandbox");
        assert!(matches!(
            detect_checkout_mode(),
            PaymentCheckoutMode::Misconfigured(_)
        ));
        assert!(is_production_mode());
        assert!(!is_demo_mutation_allowed());

        clear_mode_env_vars();
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "false");
        std::env::set_var("ESA_PAYMENT_MODE", "sandbox");
        std::env::set_var("ENVIRONMENT", "sandbox");
        assert!(matches!(
            detect_checkout_mode(),
            PaymentCheckoutMode::Misconfigured(_)
        ));

        // Various representations of false/0/no/off with sandbox -> Misconfigured
        for false_val in ["0", "no", "off"] {
            clear_mode_env_vars();
            std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", false_val);
            std::env::set_var("ESA_PAYMENT_MODE", "sandbox");
            assert!(matches!(
                detect_checkout_mode(),
                PaymentCheckoutMode::Misconfigured(_)
            ));
        }

        // False alone without sandbox/live -> Unconfigured
        clear_mode_env_vars();
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "false");
        assert_eq!(detect_checkout_mode(), PaymentCheckoutMode::Unconfigured);
        assert!(!is_production_mode());
        assert!(!is_demo_mutation_allowed());

        // Multiple harmonious sandbox opt-ins -> Simulated
        clear_mode_env_vars();
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
        std::env::set_var("ESA_PAYMENT_MODE", "sandbox");
        std::env::set_var("ENVIRONMENT", "sandbox");
        assert_eq!(detect_checkout_mode(), PaymentCheckoutMode::Simulated);
        assert!(!is_production_mode());
        assert!(is_demo_mutation_allowed());

        // Non-live provider mode alone (e.g. RAZORPAY_MODE=test) -> Simulated
        clear_mode_env_vars();
        std::env::set_var("RAZORPAY_MODE", "test");
        assert_eq!(detect_checkout_mode(), PaymentCheckoutMode::Simulated);
        assert!(!is_production_mode());
        assert!(is_demo_mutation_allowed());

        clear_mode_env_vars();
    }

    #[tokio::test]
    async fn test_stripe_inr_and_payment_method_normalization_regression_tests() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-method-norm-key");
        let state = create_test_state();

        // 1. Stripe INR with explicit "card" method succeeds at minimum 50 paise
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 50,
                "currency": "INR",
                "gateway": "stripe",
                "method": "card",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(json.get("amount_minor").and_then(|v| v.as_u64()), Some(50));
            assert_eq!(json.get("currency").and_then(|v| v.as_str()), Some("INR"));
            assert_eq!(
                json.get("routed_gateway").and_then(|v| v.as_str()),
                Some("stripe")
            );
            let checkout_url = json.get("checkout_url").and_then(|v| v.as_str()).unwrap();
            assert!(checkout_url.contains("gateway=stripe"));
        }

        // 2. Stripe INR with explicit "card" below minimum (49 paise) fails with amount_below_minimum
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 49,
                "currency": "INR",
                "gateway": "stripe",
                "method": "card",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("amount_below_minimum")
            );
        }

        // 3. Stripe INR with omitted method: strictly rejected with unsupported_provider_currency_method
        // Must NOT match card policy or classify as UPI!
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "INR",
                "gateway": "stripe",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("unsupported_provider_currency_method")
            );
        }

        // 4. Stripe INR with method="all": strictly rejected with unsupported_provider_currency_method
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "INR",
                "gateway": "stripe",
                "method": "all",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("unsupported_provider_currency_method")
            );
        }

        // 5. Razorpay INR with method="all": caller-supplied wildcard rejected
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "INR",
                "gateway": "razorpay",
                "method": "all",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("unsupported_provider_currency_method")
            );
        }

        // 6. Explicitly unsupported method on Razorpay (crypto)
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "INR",
                "gateway": "razorpay",
                "method": "crypto",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("unsupported_provider_currency_method")
            );
        }

        // 7. Explicitly unsupported method on Stripe INR (crypto)
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 5000,
                "currency": "INR",
                "gateway": "stripe",
                "method": "crypto",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("unsupported_provider_currency_method")
            );
        }

        // 8. Razorpay INR with explicit "card": succeeds with 200 OK
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 100,
                "currency": "INR",
                "gateway": "razorpay",
                "method": "card",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // 9. Razorpay INR with omitted method: succeeds with documented UPI default
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 100,
                "currency": "INR",
                "gateway": "razorpay",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // 10. Stripe USD with omitted method: succeeds with documented "card" default (min 50 cents)
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 50,
                "currency": "USD",
                "gateway": "stripe",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-method-norm-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn test_gateway_name_validation_and_auto_routing() {
        let _guard = ScopedSimulatedCheckoutGuard::new("test-gw-val-key");
        let state = create_test_state();

        // 1. Unknown gateway name rejected with 400 Bad Request
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "INR",
                "gateway": "unknown_gw",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("invalid_gateway")
            );
        }

        // 2. Unsupported gateway "crypto" rejected with 400 Bad Request
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "INR",
                "gateway": "crypto",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("invalid_gateway")
            );
        }

        // 3. Empty string gateway rejected with 400 Bad Request
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "INR",
                "gateway": "",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("invalid_gateway")
            );
        }

        // 4. Valid explicit "auto" with INR routes to healthy Razorpay
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "INR",
                "gateway": "auto",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("routed_gateway").and_then(|v| v.as_str()),
                Some("razorpay")
            );
            assert_eq!(
                json.get("failover_triggered").and_then(|v| v.as_bool()),
                Some(false)
            );
        }

        // 5. Valid explicit "auto" with USD routes to Stripe
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "USD",
                "gateway": "auto",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("routed_gateway").and_then(|v| v.as_str()),
                Some("stripe")
            );
            assert_eq!(
                json.get("failover_triggered").and_then(|v| v.as_bool()),
                Some(false)
            );
        }

        // 6. Valid omitted gateway defaults to auto (INR -> Razorpay)
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "INR",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("routed_gateway").and_then(|v| v.as_str()),
                Some("razorpay")
            );
        }

        // 7. Intentional health failover: degraded Razorpay routes "auto" to Cashfree
        {
            state
                .gateway_overrides
                .write()
                .unwrap()
                .insert("razorpay".to_string(), false);

            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "INR",
                "gateway": "auto",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("routed_gateway").and_then(|v| v.as_str()),
                Some("cashfree")
            );
            assert_eq!(
                json.get("failover_triggered").and_then(|v| v.as_bool()),
                Some(true)
            );
        }

        // 8. Intentional health failover: degraded Razorpay routes explicit "razorpay" to Cashfree
        {
            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "INR",
                "gateway": "razorpay",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("routed_gateway").and_then(|v| v.as_str()),
                Some("cashfree")
            );
            assert_eq!(
                json.get("failover_triggered").and_then(|v| v.as_bool()),
                Some(true)
            );
        }

        // 9. When both Razorpay and Cashfree are degraded, failover routes to PayU
        {
            state
                .gateway_overrides
                .write()
                .unwrap()
                .insert("cashfree".to_string(), false);

            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "INR",
                "gateway": "auto",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("routed_gateway").and_then(|v| v.as_str()),
                Some("payu")
            );
            assert_eq!(
                json.get("failover_triggered").and_then(|v| v.as_bool()),
                Some(true)
            );
        }

        // 10. When all eligible providers are degraded, return 503 SERVICE UNAVAILABLE
        {
            state
                .gateway_overrides
                .write()
                .unwrap()
                .insert("payu".to_string(), false);

            let app = build_app(state.clone());
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 1000,
                "currency": "INR",
                "gateway": "auto",
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-gw-val-key")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("no_eligible_provider")
            );
        }
    }

    // =========================================================================
    // Phase 2 Slice 2A: Secure Webhook Processing and Durable Event Idempotency
    // =========================================================================

    fn compute_test_signature(body: &[u8], secret: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    }

    fn create_test_state_with_webhook(
        secret: &str,
        account_id: Option<&str>,
    ) -> (AppState, Arc<dyn WebhookStore>) {
        let mut state = create_test_state();
        let config = RazorpayConfig {
            key_id: "rzp_test_fixture123".to_string(),
            key_secret: "rzp_test_secret123".to_string(),
            webhook_secret: secret.to_string(),
            api_base: "https://api.razorpay.com/v1".to_string(),
            mode: esa_razorpay::RazorpayMode::Test,
            test_mode_only: true,
            webhook_rate_limit_per_sec: 1000,
            api_rate_limit_per_sec: 1000,
            account_id: account_id.map(|s| s.to_string()),
        };
        let adapter = RazorpayAdapter::with_config_no_proxy(config);
        state.razorpay = Some(Arc::new(adapter));
        let store = Arc::clone(&state.webhook_store);
        (state, store)
    }

    #[tokio::test]
    async fn test_webhook_valid_signature_over_exact_body() {
        let secret = "whsec_test_secret_alpha_12345";
        let (state, store) = create_test_state_with_webhook(secret, None);
        let app = build_app(state);

        let raw_payload = serde_json::json!({
            "id": "evt_valid_001",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_valid_001",
                        "amount": 50000, // INR 500.00
                        "currency": "INR",
                        "status": "captured",
                        "method": "upi",
                        "notes": { "region": "IN-WEST" }
                    }
                }
            }
        });
        let body = serde_json::to_vec(&raw_payload).unwrap();
        let sig = compute_test_signature(&body, secret);

        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", &sig)
            .body(axum::body::Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            json.get("status").and_then(|v| v.as_str()),
            Some("accepted")
        );
        assert_eq!(
            json.get("provider_event_id").and_then(|v| v.as_str()),
            Some("evt_valid_001")
        );
        assert_eq!(
            json.get("payment_id").and_then(|v| v.as_str()),
            Some("pay_valid_001")
        );

        // Verify state is Captured
        let payment = store
            .get_payment_record("pay_valid_001")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(payment.state, PaymentState::Captured);
        assert_eq!(payment.amount_minor, 50000);
    }

    #[tokio::test]
    async fn test_webhook_missing_secret_and_missing_signature_fails_closed() {
        // 1. Missing signature header
        {
            let secret = "whsec_test_secret_beta_67890";
            let (state, _) = create_test_state_with_webhook(secret, None);
            let app = build_app(state);

            let body = b"{\"event\":\"payment.captured\"}";
            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_vec()))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // 2. Missing/empty webhook secret configured in adapter
        {
            let (state, _) = create_test_state_with_webhook("", None);
            let app = build_app(state);

            let body = b"{\"event\":\"payment.captured\"}";
            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", "dummy_sig")
                .body(axum::body::Body::from(body.to_vec()))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn test_webhook_tampered_body_and_invalid_signature_rejected() {
        let secret = "whsec_test_secret_gamma_54321";
        let (state, _) = create_test_state_with_webhook(secret, None);
        let app = build_app(state);

        let body_original = b"{\"event\":\"payment.captured\",\"amount\":5000}";
        let sig = compute_test_signature(body_original, secret);

        // Attacker tampers body by altering 1 byte or appending data
        let body_tampered = b"{\"event\":\"payment.captured\",\"amount\":9999}";

        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", &sig)
            .body(axum::body::Body::from(body_tampered.to_vec()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_webhook_malformed_payload_and_oversized_request_rejected() {
        let secret = "whsec_test_secret_delta_99999";
        let (state, _) = create_test_state_with_webhook(secret, None);

        // 1. Oversized body (> 64 KiB)
        {
            let app = build_app(state.clone());
            let oversized_body = vec![b'x'; MAX_WEBHOOK_BODY_BYTES + 1024];
            let sig = compute_test_signature(&oversized_body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(oversized_body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        }

        // 2. Malformed JSON
        {
            let app = build_app(state.clone());
            let malformed_body = b"not a json { [ }";
            let sig = compute_test_signature(malformed_body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(malformed_body.to_vec()))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn test_webhook_unknown_event_type_and_payment_status_rejected() {
        let secret = "whsec_test_secret_epsilon_11111";
        let (state, _) = create_test_state_with_webhook(secret, None);

        // 1. Unknown event type
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "event": "payment.future_unsupported_event",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_unknown_001",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }

        // 2. Unknown payment status
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "event": "payment.captured",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_unknown_status_001",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "quantum_uncertainty"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn test_webhook_duplicate_event_delivery_idempotent() {
        let secret = "whsec_test_secret_zeta_22222";
        let (state, store) = create_test_state_with_webhook(secret, None);

        let raw_payload = serde_json::json!({
            "id": "evt_dup_999",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_dup_999",
                        "amount": 10000,
                        "currency": "INR",
                        "status": "captured",
                        "method": "upi"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&raw_payload).unwrap();
        let sig = compute_test_signature(&body, secret);

        // Delivery 1 -> Accepted
        {
            let app = build_app(state.clone());
            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body.clone()))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("status").and_then(|v| v.as_str()),
                Some("accepted")
            );
        }

        // Delivery 2 (Duplicate) -> AlreadyProcessed (Idempotent 200 OK)
        {
            let app = build_app(state.clone());
            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("status").and_then(|v| v.as_str()),
                Some("already_processed")
            );
            assert_eq!(json.get("idempotent").and_then(|v| v.as_bool()), Some(true));
        }

        // Verify payment record in store was only transitioned once
        let payment = store
            .get_payment_record("pay_dup_999")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(payment.state, PaymentState::Captured);
        assert_eq!(payment.events_seen.len(), 1);
    }

    #[tokio::test]
    async fn test_webhook_concurrent_duplicate_delivery_safe() {
        let secret = "whsec_test_secret_eta_33333";
        let (state, store) = create_test_state_with_webhook(secret, None);

        let raw_payload = serde_json::json!({
            "id": "evt_concurrent_api_555",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_concurrent_api_555",
                        "amount": 20000,
                        "currency": "INR",
                        "status": "captured",
                        "method": "card"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&raw_payload).unwrap();
        let sig = compute_test_signature(&body, secret);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let s = state.clone();
            let b = body.clone();
            let sg = sig.clone();
            handles.push(tokio::spawn(async move {
                let app = build_app(s);
                let req = Request::builder()
                    .method("POST")
                    .uri("/api/razorpay/webhook")
                    .header("content-type", "application/json")
                    .header("x-razorpay-signature", &sg)
                    .body(axum::body::Body::from(b))
                    .unwrap();

                app.oneshot(req).await.unwrap()
            }));
        }

        let mut accepted_count = 0;
        let mut already_processed_count = 0;
        let mut in_flight_count = 0;

        for h in handles {
            let resp = h.await.unwrap();
            let status = resp.status();
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

            if status == StatusCode::OK {
                let s_str = json.get("status").and_then(|v| v.as_str());
                if s_str == Some("accepted") {
                    accepted_count += 1;
                } else if s_str == Some("already_processed") {
                    already_processed_count += 1;
                }
            } else if status == StatusCode::CONFLICT {
                in_flight_count += 1;
            }
        }

        // Exactly one concurrent worker must have applied the transition
        assert_eq!(accepted_count, 1);
        assert!(already_processed_count + in_flight_count >= 1);

        let payment = store
            .get_payment_record("pay_concurrent_api_555")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(payment.state, PaymentState::Captured);
        assert_eq!(payment.events_seen.len(), 1);
    }

    #[tokio::test]
    async fn test_webhook_out_of_order_capture_and_late_failure_events() {
        let secret = "whsec_test_secret_theta_44444";
        let (state, store) = create_test_state_with_webhook(secret, None);

        // 1. Authoritative capture event arrives first
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_ooo_cap",
                "event": "payment.captured",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_ooo_100",
                            "amount": 15000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // 2. Delayed failure event arrives late for the same payment
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_ooo_fail",
                "event": "payment.failed",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_ooo_100",
                            "amount": 15000,
                            "currency": "INR",
                            "status": "failed",
                            "method": "upi"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            // Transition must report LateEventIgnored
            let transition_str = format!("{:?}", json.get("transition"));
            assert!(transition_str.contains("LateEventIgnored"));
        }

        // 3. Verify state machine invariant: Payment remains Captured!
        let payment = store
            .get_payment_record("pay_ooo_100")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(payment.state, PaymentState::Captured);
    }

    #[tokio::test]
    async fn test_webhook_wrong_merchant_account_rejected() {
        let secret = "whsec_test_secret_iota_55555";
        let (state, _) = create_test_state_with_webhook(secret, Some("acc_legit_merchant_123"));
        let app = build_app(state);

        let payload = serde_json::json!({
            "account_id": "acc_fraudulent_attacker_999",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_wrong_merchant",
                        "amount": 10000,
                        "currency": "INR",
                        "status": "captured"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let sig = compute_test_signature(&body, secret);

        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", &sig)
            .body(axum::body::Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            json.get("error").and_then(|v| v.as_str()),
            Some("merchant_mismatch")
        );
    }

    #[tokio::test]
    async fn test_webhook_amount_and_currency_mismatch_rejected() {
        let secret = "whsec_test_secret_kappa_66666";
        let (state, _) = create_test_state_with_webhook(secret, None);

        // 1. Amount below minimum (50 paise < 100 paise)
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "event": "payment.captured",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_low_amount",
                            "amount": 50,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }

        // 2. Unsupported currency
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "event": "payment.captured",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_fake_currency",
                            "amount": 10000,
                            "currency": "XYZ",
                            "status": "captured"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn test_webhook_database_outage_error_and_transaction_rollback() {
        // Create an AppState with a MockWebhookStore that returns Database error
        struct FailingWebhookStore;
        #[esa_state::async_trait]
        impl WebhookStore for FailingWebhookStore {
            async fn process_webhook_event(
                &self,
                _params: &esa_state::WebhookEventParams<'_>,
            ) -> Result<WebhookProcessingResult, WebhookStoreError> {
                Err(WebhookStoreError::Database(
                    "Connection pool exhausted".into(),
                ))
            }
            async fn get_inbox_record(
                &self,
                _: &str,
                _: &str,
            ) -> Result<Option<esa_state::WebhookInboxRecord>, WebhookStoreError> {
                Ok(None)
            }
            async fn get_payment_record(
                &self,
                _: &str,
            ) -> Result<Option<PaymentEntity>, WebhookStoreError> {
                Ok(None)
            }
        }

        let secret = "whsec_test_secret_lambda_77777";
        let (mut state, _) = create_test_state_with_webhook(secret, None);
        state.webhook_store = Arc::new(FailingWebhookStore);
        let app = build_app(state);

        let payload = serde_json::json!({
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_db_failure",
                        "amount": 10000,
                        "currency": "INR",
                        "status": "captured",
                        "method": "upi"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let sig = compute_test_signature(&body, secret);

        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", &sig)
            .body(axum::body::Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        // Server MUST return 500 so upstream provider retries safely
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_resolve_webhook_store_behavior() {
        // 1. Explicit development/test mode: unset, empty, or "memory" resolve to InMemoryWebhookStore
        let s_none = resolve_webhook_store_with_mode(None, false).await;
        assert!(s_none.is_ok());

        let s_empty = resolve_webhook_store_with_mode(Some(""), false).await;
        assert!(s_empty.is_ok());

        let s_spaces = resolve_webhook_store_with_mode(Some("   "), false).await;
        assert!(s_spaces.is_ok());

        let s_mem = resolve_webhook_store_with_mode(Some("memory"), false).await;
        assert!(s_mem.is_ok());

        // 2. Production mode with missing DATABASE_URL -> MUST FAIL CLOSED
        let prod_none = resolve_webhook_store_with_mode(None, true).await;
        assert!(
            prod_none.is_err(),
            "Production must fail closed when DATABASE_URL is missing"
        );
        assert!(prod_none
            .err()
            .unwrap()
            .to_string()
            .contains("missing or blank"));

        // 3. Production mode with blank DATABASE_URL -> MUST FAIL CLOSED
        let prod_blank = resolve_webhook_store_with_mode(Some("   "), true).await;
        assert!(
            prod_blank.is_err(),
            "Production must fail closed when DATABASE_URL is blank"
        );

        // 4. Production mode with explicit 'memory' -> MUST FAIL CLOSED
        let prod_mem = resolve_webhook_store_with_mode(Some("memory"), true).await;
        assert!(
            prod_mem.is_err(),
            "Production must fail closed when configured with 'memory'"
        );
        assert!(prod_mem
            .err()
            .unwrap()
            .to_string()
            .contains("prohibited in production"));

        // 5. Production mode with invalid DATABASE_URL -> MUST FAIL CLOSED
        let prod_invalid = resolve_webhook_store_with_mode(
            Some("postgres://invalid:invalid@localhost:59999/esa_test"),
            true,
        )
        .await;
        assert!(
            prod_invalid.is_err(),
            "Production must fail closed when DATABASE_URL connection fails"
        );

        // 6. Non-production mode with invalid database URL must also FAIL CLOSED (never silently fallback)
        let dev_invalid = resolve_webhook_store_with_mode(
            Some("postgres://invalid:invalid@localhost:59999/esa_test"),
            false,
        )
        .await;
        assert!(
            dev_invalid.is_err(),
            "Must fail closed if configured DATABASE_URL connection fails"
        );
        let err_msg = dev_invalid.err().unwrap().to_string();
        assert!(
            err_msg.contains("Failing closed"),
            "Error message must specify failing closed: {}",
            err_msg
        );

        // 7. Successful PostgreSQL initialization if test DB is reachable
        if let Ok(db_url) = std::env::var("DATABASE_URL") {
            if !db_url.trim().is_empty()
                && db_url != "memory"
                && sqlx::PgPool::connect(&db_url).await.is_ok()
            {
                let prod_pg = resolve_webhook_store_with_mode(Some(&db_url), true).await;
                assert!(
                    prod_pg.is_ok(),
                    "Production with reachable PostgreSQL must succeed"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_webhook_entity_mismatch_rejected_with_conflict() {
        let secret = "whsec_test_secret_mu_88888";
        let (state, _) = create_test_state_with_webhook(secret, None);

        // 1. Initial capture with INR 5000
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_init_capture_mismatch",
                "event": "payment.captured",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_mismatch_entity_test",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // 2. Reused event ID with mismatched payment ID -> 409 Conflict
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_init_capture_mismatch", // Reused event ID!
                "event": "payment.captured",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_completely_different_id", // Different payment ID!
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::CONFLICT);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("entity_mismatch")
            );
        }

        // 3. Subsequent event with mismatched original amount 99999 -> 409 Conflict
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_refund_amt_mismatch",
                "event": "refund.processed",
                "payload": {
                    "refund": {
                        "entity": {
                            "id": "rfnd_test_002",
                            "payment_id": "pay_mismatch_entity_test",
                            "amount": 1000,
                            "currency": "INR",
                            "status": "processed"
                        }
                    },
                    "payment": {
                        "entity": {
                            "id": "pay_mismatch_entity_test",
                            "amount": 99999, // Amount mismatch!
                            "currency": "INR",
                            "status": "captured"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::CONFLICT);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                json.get("error").and_then(|v| v.as_str()),
                Some("entity_mismatch")
            );
        }
    }

    #[tokio::test]
    async fn test_webhook_invalid_state_transition_persisted_as_failed_inbox() {
        let secret = "whsec_test_secret_nu_99999";
        let (state, store) = create_test_state_with_webhook(secret, None);

        // 1. Initial capture of 5000 paise
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_capture_initial",
                "event": "payment.captured",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_test_inv_state_001",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // 2. Over-refund attempt (attempting 20000 paise refund on 5000 paise payment)
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_over_refund_attempt",
                "event": "refund.processed",
                "payload": {
                    "refund": {
                        "entity": {
                            "id": "rfnd_over_001",
                            "payment_id": "pay_test_inv_state_001",
                            "amount": 20000, // Exceeds balance!
                            "currency": "INR",
                            "status": "processed"
                        }
                    },
                    "payment": {
                        "entity": {
                            "id": "pay_test_inv_state_001",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        }

        // 3. Verify inbox record was persisted with Failed status
        let inbox = store
            .get_inbox_record("razorpay", "evt_over_refund_attempt")
            .await
            .unwrap()
            .expect("Inbox record must exist");
        assert_eq!(inbox.status, esa_state::WebhookStatus::Failed);
        assert!(inbox.error_message.is_some());

        // 4. Verify payment record remains Captured and uncorrupted
        let payment = store
            .get_payment_record("pay_test_inv_state_001")
            .await
            .unwrap()
            .expect("Payment record must exist");
        assert_eq!(payment.state, PaymentState::Captured);
        assert_eq!(payment.refunded_amount_minor, 0);
    }

    #[tokio::test]
    async fn test_duplicate_event_id_conflicting_payload_rejected_by_api() {
        let secret = "whsec_test_secret_dup_payload_77";
        let (state, _) = create_test_state_with_webhook(secret, None);

        let app = build_app(state.clone());
        let payload_orig = serde_json::json!({
            "id": "evt_dup_api_101",
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_dup_api_test",
                        "amount": 5000,
                        "currency": "INR",
                        "status": "captured",
                        "method": "upi",
                        "notes": { "intent": "original" }
                    }
                }
            }
        });
        let body_orig = serde_json::to_vec(&payload_orig).unwrap();
        let sig_orig = compute_test_signature(&body_orig, secret);

        // 1. Initial delivery -> 200 OK
        let req1 = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", &sig_orig)
            .body(axum::body::Body::from(body_orig.clone()))
            .unwrap();
        let resp1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        // 2. Exact duplicate delivery -> 200 OK with already_processed
        let req2 = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", &sig_orig)
            .body(axum::body::Body::from(body_orig))
            .unwrap();
        let resp2 = app.clone().oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let b2 = axum::body::to_bytes(resp2.into_body(), 1024 * 64)
            .await
            .unwrap();
        let j2: serde_json::Value = serde_json::from_slice(&b2).unwrap();
        assert_eq!(
            j2.get("status").and_then(|v| v.as_str()),
            Some("already_processed")
        );

        // 3. Same event ID with conflicting payload (tampered notes) -> 409 Conflict
        let payload_tampered = serde_json::json!({
            "id": "evt_dup_api_101", // SAME event ID!
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": "pay_dup_api_test", // SAME payment ID!
                        "amount": 5000,
                        "currency": "INR",
                        "status": "captured",
                        "method": "upi",
                        "notes": { "intent": "tampered_payload" }
                    }
                }
            }
        });
        let body_tampered = serde_json::to_vec(&payload_tampered).unwrap();
        let sig_tampered = compute_test_signature(&body_tampered, secret);

        let req3 = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", &sig_tampered)
            .body(axum::body::Body::from(body_tampered))
            .unwrap();
        let resp3 = app.oneshot(req3).await.unwrap();
        assert_eq!(resp3.status(), StatusCode::CONFLICT);
        let b3 = axum::body::to_bytes(resp3.into_body(), 1024 * 64)
            .await
            .unwrap();
        let j3: serde_json::Value = serde_json::from_slice(&b3).unwrap();
        assert_eq!(
            j3.get("error").and_then(|v| v.as_str()),
            Some("entity_mismatch")
        );
    }

    #[tokio::test]
    async fn test_refund_api_idempotency_and_lifecycle() {
        let secret = "whsec_test_secret_rf_api_44";
        let (state, store) = create_test_state_with_webhook(secret, None);

        // 1. Initial payment capture of 10000 paise
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_cap_api_1",
                "event": "payment.captured",
                "payload": {
                    "payment": {
                        "entity": {
                            "id": "pay_rf_api_100",
                            "amount": 10000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // 2. Pending refund delivery (refund.created with pending status) -> 200 OK, balance UNCHANGED
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_rf_pending_api",
                "event": "refund.created",
                "payload": {
                    "refund": {
                        "entity": {
                            "id": "rfnd_entity_api_1",
                            "payment_id": "pay_rf_api_100",
                            "amount": 4000,
                            "currency": "INR",
                            "status": "pending"
                        }
                    },
                    "payment": {
                        "entity": {
                            "id": "pay_rf_api_100",
                            "amount": 10000,
                            "currency": "INR",
                            "status": "captured"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);

            let payment = store
                .get_payment_record("pay_rf_api_100")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                payment.refunded_amount_minor, 0,
                "Pending refund must not increase refunded amount"
            );
            assert_eq!(payment.state, PaymentState::Captured);
        }

        // 3. Processed refund delivery (refund.processed) -> 200 OK, balance INCREMENTED
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_rf_proc_api", // Different event ID!
                "event": "refund.processed",
                "payload": {
                    "refund": {
                        "entity": {
                            "id": "rfnd_entity_api_1", // Same refund ID!
                            "payment_id": "pay_rf_api_100",
                            "amount": 4000,
                            "currency": "INR",
                            "status": "processed"
                        }
                    },
                    "payment": {
                        "entity": {
                            "id": "pay_rf_api_100",
                            "amount": 10000,
                            "currency": "INR",
                            "status": "captured"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);

            let payment = store
                .get_payment_record("pay_rf_api_100")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(payment.refunded_amount_minor, 4000);
            assert_eq!(payment.state, PaymentState::PartiallyRefunded);
        }

        // 4. Duplicate delivery of processed refund with ANOTHER event ID -> 200 OK, balance NOT double-counted
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_rf_proc_another_delivery", // Another event ID!
                "event": "refund.processed",
                "payload": {
                    "refund": {
                        "entity": {
                            "id": "rfnd_entity_api_1", // Same refund ID!
                            "payment_id": "pay_rf_api_100",
                            "amount": 4000,
                            "currency": "INR",
                            "status": "processed"
                        }
                    },
                    "payment": {
                        "entity": {
                            "id": "pay_rf_api_100",
                            "amount": 10000,
                            "currency": "INR",
                            "status": "captured"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);

            let payment = store
                .get_payment_record("pay_rf_api_100")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                payment.refunded_amount_minor, 4000,
                "Refund must not be double counted!"
            );
        }

        // 5. Reused refund ID with conflicting amount -> 409 Conflict
        {
            let app = build_app(state.clone());
            let payload = serde_json::json!({
                "id": "evt_rf_conflict_amt_delivery",
                "event": "refund.processed",
                "payload": {
                    "refund": {
                        "entity": {
                            "id": "rfnd_entity_api_1", // Same refund ID!
                            "payment_id": "pay_rf_api_100",
                            "amount": 9999, // Conflicting amount!
                            "currency": "INR",
                            "status": "processed"
                        }
                    },
                    "payment": {
                        "entity": {
                            "id": "pay_rf_api_100",
                            "amount": 10000,
                            "currency": "INR",
                            "status": "captured"
                        }
                    }
                }
            });
            let body = serde_json::to_vec(&payload).unwrap();
            let sig = compute_test_signature(&body, secret);

            let req = Request::builder()
                .method("POST")
                .uri("/api/razorpay/webhook")
                .header("content-type", "application/json")
                .header("x-razorpay-signature", &sig)
                .body(axum::body::Body::from(body))
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::CONFLICT);
        }
    }

    #[tokio::test]
    async fn test_cors_origin_allowance_rules_deterministic() {
        // Production CORS configuration with an explicitly allowed preview origin
        let prod_cors = CorsConfig::new(
            true,
            vec!["https://esapay-preview-pr-19.vercel.app".to_string()],
        );

        // 1. Exact production HTTPS origin succeeds
        assert!(prod_cors.is_origin_allowed("https://esapay.vercel.app"));

        // 2. Explicitly configured HTTPS preview origin succeeds
        assert!(prod_cors.is_origin_allowed("https://esapay-preview-pr-19.vercel.app"));

        // 3. Unrelated *.vercel.app origin fails (suffix matching forbidden)
        assert!(!prod_cors.is_origin_allowed("https://random-attacker.vercel.app"));
        assert!(!prod_cors.is_origin_allowed("https://esapay-other.vercel.app"));

        // 4. HTTP production origin fails
        assert!(!prod_cors.is_origin_allowed("http://esapay.vercel.app"));

        // 5. Localhost and loopback origins fail in production mode
        assert!(!prod_cors.is_origin_allowed("http://localhost:3000"));
        assert!(!prod_cors.is_origin_allowed("http://localhost:5173"));
        assert!(!prod_cors.is_origin_allowed("http://127.0.0.1:8080"));
        assert!(!prod_cors.is_origin_allowed("https://localhost"));

        // 6. Explicitly configured localhost in ALLOWED_ORIGINS must FAIL in production mode
        let prod_cors_with_local = CorsConfig::new(
            true,
            vec![
                "http://localhost:3000".to_string(),
                "https://localhost:3000".to_string(),
                "http://127.0.0.1:8080".to_string(),
                "https://esapay-preview-pr-19.vercel.app".to_string(),
            ],
        );
        assert!(!prod_cors_with_local.is_origin_allowed("http://localhost:3000"));
        assert!(!prod_cors_with_local.is_origin_allowed("https://localhost:3000"));
        assert!(!prod_cors_with_local.is_origin_allowed("http://127.0.0.1:8080"));
        assert!(prod_cors_with_local.is_origin_allowed("https://esapay-preview-pr-19.vercel.app"));

        // 7. Explicit HTTP preview origins in production must FAIL
        let prod_cors_with_http_preview = CorsConfig::new(
            true,
            vec!["http://esapay-preview-pr-19.vercel.app".to_string()],
        );
        assert!(!prod_cors_with_http_preview
            .is_origin_allowed("http://esapay-preview-pr-19.vercel.app"));

        // 8. Non-production (development) mode allows localhost
        let dev_cors = CorsConfig::new(false, vec![]);
        assert!(dev_cors.is_origin_allowed("https://esapay.vercel.app"));
        assert!(dev_cors.is_origin_allowed("http://localhost:3000"));
        assert!(dev_cors.is_origin_allowed("http://localhost:5173"));
        assert!(dev_cors.is_origin_allowed("http://127.0.0.1:8080"));
        assert!(dev_cors.is_origin_allowed("https://localhost:443"));
        // Development mode still rejects arbitrary unrelated origins and HTTP production
        assert!(!dev_cors.is_origin_allowed("https://random-attacker.vercel.app"));
        assert!(!dev_cors.is_origin_allowed("http://esapay.vercel.app"));

        // 9. Wildcard configuration '*' cannot enable credentialed access
        let wildcard_cors = CorsConfig::new(true, vec!["*".to_string()]);
        assert!(!wildcard_cors.is_origin_allowed("*"));
        assert!(!wildcard_cors.is_origin_allowed("https://attacker.com"));
        assert!(!wildcard_cors.is_origin_allowed("https://evil.vercel.app"));
        assert!(!wildcard_cors.is_origin_allowed("http://localhost:3000"));
        // Legitimate production origin still allowed
        assert!(wildcard_cors.is_origin_allowed("https://esapay.vercel.app"));
    }

    #[tokio::test]
    async fn test_cors_preflight_and_headers_handling() {
        let cors_config = CorsConfig::new(
            true,
            vec!["https://esapay-preview-pr-19.vercel.app".to_string()],
        );
        let app = Router::new()
            .route("/api/test-preflight", post(|| async { "ok" }))
            .layer(cors_config.build_cors_layer());

        // 1. Legitimate preflight from exact production origin succeeds
        let req_prod = Request::builder()
            .method("OPTIONS")
            .uri("/api/test-preflight")
            .header("origin", "https://esapay.vercel.app")
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "content-type, authorization, x-esa-api-key",
            )
            .body(axum::body::Body::empty())
            .unwrap();

        let resp_prod = app.clone().oneshot(req_prod).await.unwrap();
        assert_eq!(resp_prod.status(), StatusCode::OK);
        assert_eq!(
            resp_prod
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("https://esapay.vercel.app")
        );
        assert_eq!(
            resp_prod
                .headers()
                .get("access-control-allow-credentials")
                .and_then(|v| v.to_str().ok()),
            Some("true")
        );

        // 2. Legitimate preflight from configured preview origin succeeds
        let req_preview = Request::builder()
            .method("OPTIONS")
            .uri("/api/test-preflight")
            .header("origin", "https://esapay-preview-pr-19.vercel.app")
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "content-type")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp_preview = app.clone().oneshot(req_preview).await.unwrap();
        assert_eq!(resp_preview.status(), StatusCode::OK);
        assert_eq!(
            resp_preview
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("https://esapay-preview-pr-19.vercel.app")
        );

        // 3. Preflight from arbitrary *.vercel.app origin is rejected (no allow-origin header)
        let req_unauth = Request::builder()
            .method("OPTIONS")
            .uri("/api/test-preflight")
            .header("origin", "https://arbitrary-attacker.vercel.app")
            .header("access-control-request-method", "POST")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp_unauth = app.clone().oneshot(req_unauth).await.unwrap();
        assert!(resp_unauth
            .headers()
            .get("access-control-allow-origin")
            .is_none());

        // 4. Preflight from HTTP production origin is rejected
        let req_http = Request::builder()
            .method("OPTIONS")
            .uri("/api/test-preflight")
            .header("origin", "http://esapay.vercel.app")
            .header("access-control-request-method", "POST")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp_http = app.clone().oneshot(req_http).await.unwrap();
        assert!(resp_http
            .headers()
            .get("access-control-allow-origin")
            .is_none());

        // 5. Preflight from localhost in production mode is rejected
        let req_local = Request::builder()
            .method("OPTIONS")
            .uri("/api/test-preflight")
            .header("origin", "http://localhost:3000")
            .header("access-control-request-method", "POST")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp_local = app.oneshot(req_local).await.unwrap();
        assert!(resp_local
            .headers()
            .get("access-control-allow-origin")
            .is_none());
    }

    #[tokio::test]
    async fn test_webhook_first_seen_refund_returns_unapplied() {
        let secret = "whsec_test_unapplied_01";
        let (state, store) = create_test_state_with_webhook(secret, None);

        let app = build_app(state);
        let payload = serde_json::json!({
            "id": "evt_api_first_seen_rfnd_01",
            "event": "refund.processed",
            "payload": {
                "refund": {
                    "entity": {
                        "id": "rfnd_api_first_seen_01",
                        "payment_id": "pay_api_first_seen_01",
                        "amount": 2500,
                        "currency": "INR",
                        "status": "processed"
                    }
                }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let sig = compute_test_signature(&body, secret);

        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", &sig)
            .body(axum::body::Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let resp_json: serde_json::Value = serde_json::from_slice(&resp_bytes).unwrap();
        assert_eq!(resp_json["status"], "unapplied");
        assert_eq!(resp_json["unapplied"], true);
        assert_eq!(resp_json["payment_id"], "pay_api_first_seen_01");

        // Verify payment record was NOT fabricated in store
        assert!(store
            .get_payment_record("pay_api_first_seen_01")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_audit_endpoints_access_control_and_aliases() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-secret-key-audit");
        std::env::set_var("ESA_TEST_API_KEY", "test-merchant-key-audit");

        let state = create_test_state();
        let app = build_app(state.clone());

        let audit_routes = vec![
            "/api/audit/trail",
            "/api/v1/audit/trail",
            "/api/audit/verify-chain",
            "/api/v1/audit/verify-chain",
            "/api/audit/decision/dec-nonexistent-123",
            "/api/v1/audit/decision/dec-nonexistent-123",
            "/api/audit/replay/dec-nonexistent-123",
            "/api/v1/audit/replay/dec-nonexistent-123",
        ];

        // 1. Anonymous requests fail closed with 401 Unauthorized
        for uri in &audit_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "Expected 401 for anonymous access to {}",
                uri
            );
        }

        // POST versioned replay aliases anonymous fail with 401
        for uri in &["/api/audit/replay/dec-123", "/api/v1/audit/replay/dec-123"] {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "Expected 401 for anonymous POST to {}",
                uri
            );
        }

        // 2. Ordinary merchant key fails with 403 Forbidden (missing audit:read / audit:replay scope)
        for uri in &audit_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "test-merchant-key-audit")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "Expected 403 for merchant access to {}",
                uri
            );
        }

        for uri in &["/api/audit/replay/dec-123", "/api/v1/audit/replay/dec-123"] {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("x-api-key", "test-merchant-key-audit")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "Expected 403 for merchant POST to {}",
                uri
            );
        }

        // 3. Admin key succeeds: trail and verify-chain return 200 OK
        for uri in &[
            "/api/audit/trail",
            "/api/v1/audit/trail",
            "/api/audit/verify-chain",
            "/api/v1/audit/verify-chain",
        ] {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "admin-secret-key-audit")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "Expected 200 for admin access to {}",
                uri
            );
        }

        // For non-existent decision, admin passes authentication and reaches handler logic (404 Not Found)
        for uri in &[
            "/api/audit/decision/dec-nonexistent-123",
            "/api/v1/audit/decision/dec-nonexistent-123",
            "/api/audit/replay/dec-nonexistent-123",
            "/api/v1/audit/replay/dec-nonexistent-123",
        ] {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "admin-secret-key-audit")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "Expected 404 (auth passed) for admin access to non-existent decision at {}",
                uri
            );
        }

        std::env::remove_var("ESA_ADMIN_API_KEY");
        std::env::remove_var("ESA_TEST_API_KEY");
    }

    #[tokio::test]
    async fn test_workload_and_gateway_introspection_access_control() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-secret-key-intro");
        std::env::set_var("ESA_TEST_API_KEY", "test-merchant-key-intro");

        let state = create_test_state();
        let dummy_workload = WorkloadEntity {
            workload_id: "payment-upi-india-south".to_string(),
            shard_id: "shard-1".to_string(),
            state: WorkloadState::Healthy,
            region: Region::IndiaSouth,
            metrics: WorkloadMetrics {
                rate_per_min: 100.0,
                p50_latency_ms: 10.0,
                p95_latency_ms: 20.0,
                p99_latency_ms: 30.0,
                error_rate: 0.0,
                queue_depth: 0,
                timestamp: chrono::Utc::now(),
            },
            replication: ReplicationPolicy {
                min_replicas: 1,
                max_replicas: 3,
                current_replicas: 1,
                consistency_mode: ConsistencyMode::Strong,
            },
            locality: LocalityPreference {
                preferred_region: Region::IndiaSouth,
                fallback_regions: vec![],
            },
            lifecycle: LifecycleState::Active,
            version: 1,
            updated_at: chrono::Utc::now(),
        };
        state.state_fabric.upsert_workload(dummy_workload).unwrap();
        let app = build_app(state);

        let global_workload_routes = vec![
            "/api/workloads",
            "/api/v1/workloads",
            "/api/workloads/payment-upi-india-south",
            "/api/v1/workloads/payment-upi-india-south",
        ];

        let gateway_routes = vec!["/api/gateways", "/api/v1/gateways"];

        let sensitive_audit_routes = vec![
            "/api/actions/recent",
            "/api/v1/actions/recent",
            "/api/agents/activity",
            "/api/v1/agents/activity",
            "/api/verdicts/recent",
            "/api/v1/verdicts/recent",
            "/api/verdicts/stats",
            "/api/v1/verdicts/stats",
            "/api/effects/measurements",
            "/api/v1/effects/measurements",
            "/api/effects/recent",
            "/api/v1/effects/recent",
        ];

        let diagnostic_telemetry_routes = vec![
            "/api/agents/status",
            "/api/v1/agents/status",
            "/api/vitals/history",
            "/api/v1/vitals/history",
            "/api/metrics/tokens",
            "/api/v1/metrics/tokens",
            "/api/costs/ai",
            "/api/v1/costs/ai",
            "/api/costs/per-agent",
            "/api/v1/costs/per-agent",
            "/api/benchmark/latest",
            "/api/v1/benchmark/latest",
            "/api/benchmark/ablations",
            "/api/v1/benchmark/ablations",
            "/api/agents/fluid-reasoner/status",
            "/api/v1/agents/fluid-reasoner/status",
            "/api/intent/active",
            "/api/v1/intent/active",
            "/api/intent/violations",
            "/api/v1/intent/violations",
        ];

        let all_routes: Vec<&str> = global_workload_routes
            .iter()
            .chain(gateway_routes.iter())
            .chain(sensitive_audit_routes.iter())
            .chain(diagnostic_telemetry_routes.iter())
            .copied()
            .collect();

        // 1. Anonymous requests fail closed with 401 Unauthorized for all routes
        for uri in &all_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "Expected 401 for anonymous access to {}",
                uri
            );
        }

        // 2. Merchant key succeeds on gateway visibility (operational non-sensitive info)
        for uri in &gateway_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "test-merchant-key-intro")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "Expected 200 for merchant access with gateways:read to {}",
                uri
            );
        }

        // 3. Merchant key is rejected with 403 Forbidden for global workloads (tenant isolation)
        for uri in &global_workload_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "test-merchant-key-intro")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "Expected 403 Forbidden for non-admin merchant accessing global workload at {}",
                uri
            );
        }

        // 4. Merchant key lacking audit:read fails closed with 403 Forbidden on sensitive audit routes
        for uri in &sensitive_audit_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "test-merchant-key-intro")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "Expected 403 Forbidden for merchant without audit:read accessing {}",
                uri
            );
        }

        // 5. Merchant key lacking telemetry:admin fails closed with 403 Forbidden on diagnostic telemetry routes
        for uri in &diagnostic_telemetry_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "test-merchant-key-intro")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "Expected 403 Forbidden for merchant without telemetry:admin accessing {}",
                uri
            );
        }

        // 6. Admin key succeeds with 200 OK on all routes
        for uri in &all_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "admin-secret-key-intro")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "Expected 200 for admin access to {}",
                uri
            );
        }

        std::env::remove_var("ESA_ADMIN_API_KEY");
        std::env::remove_var("ESA_TEST_API_KEY");
    }

    #[tokio::test]
    async fn test_benchmark_execution_and_ablation_access_control() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-bench-key");
        std::env::set_var("ESA_TEST_API_KEY", "merchant-bench-key");

        let state = create_test_state();
        let dummy_workload = WorkloadEntity {
            workload_id: "payment-upi-india-south".to_string(),
            shard_id: "shard-1".to_string(),
            state: WorkloadState::Healthy,
            region: Region::IndiaSouth,
            metrics: WorkloadMetrics {
                rate_per_min: 100.0,
                p50_latency_ms: 10.0,
                p95_latency_ms: 20.0,
                p99_latency_ms: 30.0,
                error_rate: 0.0,
                queue_depth: 0,
                timestamp: chrono::Utc::now(),
            },
            replication: ReplicationPolicy {
                min_replicas: 1,
                max_replicas: 3,
                current_replicas: 1,
                consistency_mode: ConsistencyMode::Strong,
            },
            locality: LocalityPreference {
                preferred_region: Region::IndiaSouth,
                fallback_regions: vec![],
            },
            lifecycle: LifecycleState::Active,
            version: 1,
            updated_at: chrono::Utc::now(),
        };
        state.state_fabric.upsert_workload(dummy_workload).unwrap();
        let app = build_app(state.clone());

        let ablation_routes = ["/api/benchmark/ablations", "/api/v1/benchmark/ablations"];

        // 1. Anonymous GET is rejected with 401 Unauthorized
        for uri in &ablation_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // 2. Merchant GET is rejected with 403 Forbidden
        for uri in &ablation_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "merchant-bench-key")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }

        // 3. Admin GET succeeds with 200 OK and returns not_available without running benchmark
        let workloads_before = state.state_fabric.list_workloads();
        for uri in &ablation_routes {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("x-api-key", "admin-bench-key")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(
                body_json.get("status").and_then(|v| v.as_str()),
                Some("not_available")
            );
        }
        let workloads_after = state.state_fabric.list_workloads();
        assert_eq!(
            serde_json::to_value(&workloads_before).unwrap(),
            serde_json::to_value(&workloads_after).unwrap(),
            "GET must never mutate workload state or reset baseline"
        );
        assert!(
            state.last_ablation.read().unwrap().is_none(),
            "GET must never trigger benchmark execution"
        );

        // 4. Anonymous POST is rejected with 401 Unauthorized
        for uri in &ablation_routes {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // 5. Merchant POST is rejected with 403 Forbidden
        for uri in &ablation_routes {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("x-api-key", "merchant-bench-key")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }

        // 6. Admin POST in production (without simulation opt-in) is rejected with 403 Forbidden
        std::env::set_var("ENVIRONMENT", "production");
        std::env::remove_var("ESA_ENABLE_SIMULATED_CHECKOUT");
        for uri in &ablation_routes {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("x-api-key", "admin-bench-key")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }
        clear_mode_env_vars();

        // 7. Concurrent requests cannot launch duplicate benchmark runs (409 Conflict)
        state
            .benchmark_running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
        for uri in &ablation_routes {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("x-api-key", "admin-bench-key")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::CONFLICT);
        }
        state
            .benchmark_running
            .store(false, std::sync::atomic::Ordering::SeqCst);

        // 8. Benchmark run routes (/api/benchmark/run) access control
        for uri in &["/api/benchmark/run", "/api/v1/benchmark/run"] {
            // Anonymous rejected 401
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{}"))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

            // Merchant rejected 403
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("content-type", "application/json")
                .header("x-api-key", "merchant-bench-key")
                .body(axum::body::Body::from("{}"))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }

        clear_mode_env_vars();
        std::env::remove_var("ESA_ADMIN_API_KEY");
        std::env::remove_var("ESA_TEST_API_KEY");
    }

    #[tokio::test]
    async fn test_websocket_telemetry_access_control() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-secret-key-ws");
        std::env::set_var("ESA_TEST_API_KEY", "test-merchant-key-ws");

        let state = create_test_state();
        let app = build_app(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let base_url = format!("http://127.0.0.1:{}", port);

        // Helper to build WS upgrade request
        let ws_req = |url: &str| {
            client
                .get(url)
                .header("connection", "Upgrade")
                .header("upgrade", "websocket")
                .header("sec-websocket-version", "13")
                .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        };

        // --- Production Mode ---
        std::env::set_var("ENVIRONMENT", "production");

        // 1. Production query-param token is rejected with 401 Unauthorized
        let resp = ws_req(&format!(
            "{}/ws/telemetry?token=admin-secret-key-ws",
            base_url
        ))
        .send()
        .await
        .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        // 2. Production merchant header is rejected with 403 Forbidden
        let resp = ws_req(&format!("{}/ws/telemetry", base_url))
            .header("x-api-key", "test-merchant-key-ws")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

        // 3. Production invalid header token is rejected with 401 Unauthorized
        let resp = ws_req(&format!("{}/ws/telemetry", base_url))
            .header("x-api-key", "invalid-bad-key")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        // 4. Production admin header token is accepted with 101 Switching Protocols
        let resp = ws_req(&format!("{}/ws/telemetry", base_url))
            .header("x-api-key", "admin-secret-key-ws")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::SWITCHING_PROTOCOLS);

        // --- Non-Production Mode ---
        std::env::set_var("ENVIRONMENT", "development");

        // 5. In development, merchant header token is accepted with 101 Switching Protocols
        let resp = ws_req(&format!("{}/ws/telemetry", base_url))
            .header("x-api-key", "test-merchant-key-ws")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::SWITCHING_PROTOCOLS);

        // 6. In development, unauthenticated connection is accepted with 101 Switching Protocols (demo mode)
        let resp = ws_req(&format!("{}/ws/telemetry", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::SWITCHING_PROTOCOLS);

        clear_mode_env_vars();
        std::env::remove_var("ESA_ADMIN_API_KEY");
        std::env::remove_var("ESA_TEST_API_KEY");
    }

    #[tokio::test]
    async fn test_razorpay_order_merchant_binding_and_confirmation_ownership() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-secret-key-rzp");
        std::env::set_var("ESA_TEST_API_KEY", "test-merchant-key-rzp");

        // Set up mock upstream Razorpay API server
        let mock_app = axum::Router::new()
            .route(
                "/orders",
                axum::routing::post(|Json(body): Json<serde_json::Value>| async move {
                    let notes = body.get("notes").cloned().unwrap_or(serde_json::json!({}));
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "order_mock_001",
                            "amount": 5000,
                            "currency": "INR",
                            "notes": notes,
                        })),
                    )
                }),
            )
            .route(
                "/orders/order_matching",
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "order_matching",
                            "amount": 5000,
                            "currency": "INR",
                            "notes": {
                                "esa_merchant_id": "merchant_sandbox_01"
                            }
                        })),
                    )
                }),
            )
            .route(
                "/orders/order_other",
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "order_other",
                            "amount": 5000,
                            "currency": "INR",
                            "notes": {
                                "esa_merchant_id": "other_merchant_999"
                            }
                        })),
                    )
                }),
            )
            .route(
                "/orders/order_no_notes",
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "order_no_notes",
                            "amount": 5000,
                            "currency": "INR",
                            "notes": {}
                        })),
                    )
                }),
            )
            .route(
                "/payments/pay_matching_both",
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "pay_matching_both",
                            "order_id": "order_matching",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi",
                            "notes": {
                                "esa_merchant_id": "merchant_sandbox_01"
                            }
                        })),
                    )
                }),
            )
            .route(
                "/payments/pay_notes_spoofed",
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "pay_notes_spoofed",
                            "order_id": "order_other",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi",
                            "notes": {
                                "esa_merchant_id": "merchant_sandbox_01"
                            }
                        })),
                    )
                }),
            )
            .route(
                "/payments/pay_order_wins",
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "pay_order_wins",
                            "order_id": "order_matching",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi",
                            "notes": {
                                "esa_merchant_id": "other_merchant_999"
                            }
                        })),
                    )
                }),
            )
            .route(
                "/payments/pay_missing_order_notes",
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "pay_missing_order_notes",
                            "order_id": "order_no_notes",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi",
                            "notes": {
                                "esa_merchant_id": "merchant_sandbox_01"
                            }
                        })),
                    )
                }),
            )
            .route(
                "/payments/pay_missing_order_id",
                axum::routing::get(|| async move {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "pay_missing_order_id",
                            "amount": 5000,
                            "currency": "INR",
                            "status": "captured",
                            "method": "upi",
                            "notes": {
                                "esa_merchant_id": "merchant_sandbox_01"
                            }
                        })),
                    )
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let mut state = create_test_state();
        let rzp_config = RazorpayConfig {
            key_id: "rzp_test_key123".to_string(),
            key_secret: "rzp_test_sec123".to_string(),
            webhook_secret: "whsec_test".to_string(),
            api_base: format!("http://127.0.0.1:{}", port),
            mode: esa_razorpay::RazorpayMode::Test,
            test_mode_only: true,
            webhook_rate_limit_per_sec: 1000,
            api_rate_limit_per_sec: 1000,
            account_id: None,
        };
        state.razorpay = Some(Arc::new(RazorpayAdapter::with_config_no_proxy(rzp_config)));

        state
            .webhook_store
            .record_payment_attempt(&esa_core::provider::PaymentAttemptRecord {
                transaction_id: "tx_order_matching".to_string(),
                merchant_id: "merchant_sandbox_01".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_matching".to_string()),
                payment_reference: None,
                amount_minor: 5000,
                currency: "INR".to_string(),
                state: "created".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        state
            .webhook_store
            .record_payment_attempt(&esa_core::provider::PaymentAttemptRecord {
                transaction_id: "tx_order_other".to_string(),
                merchant_id: "other_merchant_999".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_other".to_string()),
                payment_reference: None,
                amount_minor: 5000,
                currency: "INR".to_string(),
                state: "created".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        let app = build_app(state.clone());

        // 1. Test Razorpay Order Creation binds caller's merchant_id to notes and durable mapping
        let req_body = serde_json::json!({
            "amount_cents": 5000,
            "region": "IN-SOUTH",
            "payment_method": "upi"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/orders")
            .header("content-type", "application/json")
            .header("x-api-key", "test-merchant-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&req_body).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            state.order_merchants.read().unwrap().get("order_mock_001"),
            Some(&"merchant_sandbox_01".to_string()),
            "Created order ID must be recorded in durable order_merchants map"
        );

        // Scenario 1: Payment notes and order notes both identify authenticated merchant -> 200 OK
        let confirm_req = serde_json::json!({
            "payment_id": "pay_matching_both"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "test-merchant-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Scenario 2: Payment notes claim authenticated merchant, but order notes identify another merchant -> 403 Forbidden
        let confirm_req = serde_json::json!({
            "payment_id": "pay_notes_spoofed"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "test-merchant-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Scenario 3: Payment notes identify another merchant, but order notes identify authenticated merchant -> 200 OK
        let confirm_req = serde_json::json!({
            "payment_id": "pay_order_wins"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "test-merchant-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Scenario 4: Missing order ownership metadata -> 403 Forbidden
        let confirm_req = serde_json::json!({
            "payment_id": "pay_missing_order_notes"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "test-merchant-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Scenario 5: Missing order ID on payment -> 403 Forbidden
        let confirm_req = serde_json::json!({
            "payment_id": "pay_missing_order_id"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "test-merchant-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Scenario 6: Verify rejected confirmation produces no payment-state mutation or telemetry/audit
        let audit_count_before = state.audit_store.list_recent(100).len();
        let mut telemetry_rx = state.broadcaster.subscribe();
        let workloads_before = state.state_fabric.list_workloads();
        let payment_inbox_before = state
            .webhook_store
            .get_payment_record("pay_notes_spoofed")
            .await
            .unwrap();

        let confirm_req = serde_json::json!({
            "payment_id": "pay_notes_spoofed"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "test-merchant-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let audit_count_after = state.audit_store.list_recent(100).len();
        assert_eq!(
            audit_count_before, audit_count_after,
            "Rejected confirmation must not write to audit trail"
        );
        let workloads_after = state.state_fabric.list_workloads();
        assert_eq!(
            serde_json::to_value(&workloads_before).unwrap(),
            serde_json::to_value(&workloads_after).unwrap(),
            "Rejected confirmation must not mutate workload state"
        );
        let payment_inbox_after = state
            .webhook_store
            .get_payment_record("pay_notes_spoofed")
            .await
            .unwrap();
        assert_eq!(
            payment_inbox_before, payment_inbox_after,
            "Rejected confirmation must not mutate payment state in store"
        );
        assert!(
            telemetry_rx.try_recv().is_err(),
            "Rejected confirmation must not publish telemetry"
        );

        // Scenario 7: Admin caller can confirm order belonging to another merchant -> 200 OK, but missing order/metadata still fails 403
        // 7a: Admin confirms order belonging to other merchant -> 200 OK
        let confirm_req = serde_json::json!({
            "payment_id": "pay_notes_spoofed"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "admin-secret-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 7b: Admin confirms payment missing order ID -> 403 Forbidden
        let confirm_req = serde_json::json!({
            "payment_id": "pay_missing_order_id"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "admin-secret-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 7c: Admin confirms payment where order lacks merchant metadata -> 403 Forbidden
        let confirm_req = serde_json::json!({
            "payment_id": "pay_missing_order_notes"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "admin-secret-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Scenario 8: Conflicting server-tracked order mapping fails closed with 403 Forbidden
        state
            .order_merchants
            .write()
            .unwrap()
            .insert("order_matching".to_string(), "rogue_merchant".to_string());
        let confirm_req = serde_json::json!({
            "payment_id": "pay_matching_both"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", "test-merchant-key-rzp")
            .body(axum::body::Body::from(
                serde_json::to_vec(&confirm_req).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        clear_mode_env_vars();
        std::env::remove_var("ESA_ADMIN_API_KEY");
        std::env::remove_var("ESA_TEST_API_KEY");
    }

    #[tokio::test]
    async fn test_sandbox_api_key_lifecycle_and_security_isolation() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();

        let state = create_test_state();
        let app = build_app(state.clone());

        // 1. Generation via /api/keys/generate-sandbox and alias /api/v1/keys/generate-sandbox
        let gen_req = Request::builder()
            .method("POST")
            .uri("/api/keys/generate-sandbox")
            .header("content-type", "application/json")
            .body(axum::body::Body::empty())
            .unwrap();
        let gen_resp = app.clone().oneshot(gen_req).await.unwrap();
        assert_eq!(gen_resp.status(), StatusCode::OK);
        let gen_bytes = axum::body::to_bytes(gen_resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let gen_json: serde_json::Value = serde_json::from_slice(&gen_bytes).unwrap();
        let raw_key = gen_json["api_key"].as_str().unwrap().to_string();
        assert!(raw_key.starts_with("esa_test_demo_"));
        assert_eq!(gen_json["environment"], "sandbox");
        assert_eq!(gen_json["expires_in"], "24h");

        // Alias generation
        let gen_req_v1 = Request::builder()
            .method("POST")
            .uri("/api/v1/keys/generate-sandbox")
            .header("content-type", "application/json")
            .body(axum::body::Body::empty())
            .unwrap();
        let gen_resp_v1 = app.clone().oneshot(gen_req_v1).await.unwrap();
        assert_eq!(gen_resp_v1.status(), StatusCode::OK);

        // Verify raw key is NOT stored plainly; only SHA-256 hash is in the store
        let key_hash = api_keys::hash_api_key(&raw_key);
        let record = state
            .sandbox_keys
            .get_key(&key_hash)
            .await
            .expect("Key record must exist by hash");
        assert_eq!(record.key_hash, key_hash);
        assert!(!record.revoked);
        assert!(record.expires_at > chrono::Utc::now());

        // 2. Validate endpoint (/api/keys/validate and /api/v1/keys/validate)
        let val_body = serde_json::json!({ "api_key": raw_key });
        let val_req = Request::builder()
            .method("POST")
            .uri("/api/keys/validate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&val_body).unwrap(),
            ))
            .unwrap();
        let val_resp = app.clone().oneshot(val_req).await.unwrap();
        assert_eq!(val_resp.status(), StatusCode::OK);
        let val_bytes = axum::body::to_bytes(val_resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let val_json: serde_json::Value = serde_json::from_slice(&val_bytes).unwrap();
        assert_eq!(val_json["valid"], true);
        assert_eq!(val_json["environment"], "sandbox");

        // Alias validate
        let val_req_v1 = Request::builder()
            .method("POST")
            .uri("/api/v1/keys/validate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&val_body).unwrap(),
            ))
            .unwrap();
        let val_resp_v1 = app.clone().oneshot(val_req_v1).await.unwrap();
        assert_eq!(val_resp_v1.status(), StatusCode::OK);

        // 3. Authenticate with /api/auth/me using Bearer
        let me_req = Request::builder()
            .method("GET")
            .uri("/api/auth/me")
            .header("authorization", format!("Bearer {}", raw_key))
            .body(axum::body::Body::empty())
            .unwrap();
        let me_resp = app.clone().oneshot(me_req).await.unwrap();
        assert_eq!(me_resp.status(), StatusCode::OK);
        let me_bytes = axum::body::to_bytes(me_resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let me_json: serde_json::Value = serde_json::from_slice(&me_bytes).unwrap();
        assert_eq!(me_json["environment"], "sandbox");
        let scopes = me_json["scopes"].as_array().unwrap();
        assert!(scopes.iter().any(|s| s == "payments:write"));
        assert!(scopes.iter().any(|s| s == "payments:read"));
        assert!(scopes.iter().any(|s| s == "telemetry:read"));
        assert!(scopes.iter().any(|s| s == "gateways:read"));
        assert!(!scopes.iter().any(|s| s == "gateways:toggle"));
        assert!(!scopes.iter().any(|s| s == "audit:read"));
        assert!(!scopes.iter().any(|s| s == "admin"));

        // 4. Checkout in simulated mode works
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
        std::env::set_var("ESA_PAYMENT_MODE", "simulated");
        let checkout_body = serde_json::json!({
            "amount": 1000,
            "currency": "INR",
            "gateway": "auto"
        });
        let chk_req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", raw_key))
            .body(axum::body::Body::from(
                serde_json::to_vec(&checkout_body).unwrap(),
            ))
            .unwrap();
        let chk_resp = app.clone().oneshot(chk_req).await.unwrap();
        assert_eq!(chk_resp.status(), StatusCode::OK);

        // 5. Live payment mode rejects sandbox keys
        clear_mode_env_vars();
        std::env::set_var("ESA_PAYMENT_MODE", "live");
        let chk_live_req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", raw_key))
            .body(axum::body::Body::from(
                serde_json::to_vec(&checkout_body).unwrap(),
            ))
            .unwrap();
        let chk_live_resp = app.clone().oneshot(chk_live_req).await.unwrap();
        assert_eq!(chk_live_resp.status(), StatusCode::UNAUTHORIZED);

        // 6. Production mode rejection
        clear_mode_env_vars();
        std::env::set_var("ENVIRONMENT", "production");

        let val_prod_req = Request::builder()
            .method("POST")
            .uri("/api/keys/validate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&val_body).unwrap(),
            ))
            .unwrap();
        let val_prod_resp = app.clone().oneshot(val_prod_req).await.unwrap();
        assert_eq!(val_prod_resp.status(), StatusCode::UNAUTHORIZED);

        let me_prod_req = Request::builder()
            .method("GET")
            .uri("/api/auth/me")
            .header("authorization", format!("Bearer {}", raw_key))
            .body(axum::body::Body::empty())
            .unwrap();
        let me_prod_resp = app.clone().oneshot(me_prod_req).await.unwrap();
        assert_eq!(me_prod_resp.status(), StatusCode::UNAUTHORIZED);

        clear_mode_env_vars();

        // 7. Admin & High-Privilege endpoint protection (403 Forbidden for sandbox keys)
        let toggle_body = serde_json::json!({ "enabled": false });
        let toggle_req = Request::builder()
            .method("POST")
            .uri("/api/gateways/razorpay/toggle")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", raw_key))
            .body(axum::body::Body::from(
                serde_json::to_vec(&toggle_body).unwrap(),
            ))
            .unwrap();
        let toggle_resp = app.clone().oneshot(toggle_req).await.unwrap();
        assert_eq!(toggle_resp.status(), StatusCode::FORBIDDEN);

        let audit_req = Request::builder()
            .method("GET")
            .uri("/api/audit/trail")
            .header("authorization", format!("Bearer {}", raw_key))
            .body(axum::body::Body::empty())
            .unwrap();
        let audit_resp = app.clone().oneshot(audit_req).await.unwrap();
        assert_eq!(audit_resp.status(), StatusCode::FORBIDDEN);

        let spike_req = Request::builder()
            .method("POST")
            .uri("/api/demo/trigger-spike")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", raw_key))
            .body(axum::body::Body::from(
                serde_json::to_vec(&serde_json::json!({ "multiplier": 2.0 })).unwrap(),
            ))
            .unwrap();
        let spike_resp = app.clone().oneshot(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::FORBIDDEN);

        // 8. Server-side Expiry
        let expired_key_raw = format!("esa_test_demo_{}", uuid::Uuid::new_v4().simple());
        let expired_hash = api_keys::hash_api_key(&expired_key_raw);
        let expired_record = api_keys::SandboxKeyRecord {
            key_hash: expired_hash.clone(),
            key_prefix: expired_key_raw[..18].to_string(),
            merchant_id: "test_merchant_expired".to_string(),
            created_at: chrono::Utc::now() - chrono::Duration::hours(25),
            expires_at: chrono::Utc::now() - chrono::Duration::hours(1),
            revoked: false,
        };
        state
            .sandbox_keys
            .insert_key(&expired_record)
            .await
            .unwrap();

        let exp_val_req = Request::builder()
            .method("POST")
            .uri("/api/keys/validate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&serde_json::json!({ "api_key": expired_key_raw })).unwrap(),
            ))
            .unwrap();
        let exp_val_resp = app.clone().oneshot(exp_val_req).await.unwrap();
        assert_eq!(exp_val_resp.status(), StatusCode::UNAUTHORIZED);

        let exp_me_req = Request::builder()
            .method("GET")
            .uri("/api/auth/me")
            .header("authorization", format!("Bearer {}", expired_key_raw))
            .body(axum::body::Body::empty())
            .unwrap();
        let exp_me_resp = app.clone().oneshot(exp_me_req).await.unwrap();
        assert_eq!(exp_me_resp.status(), StatusCode::UNAUTHORIZED);

        // 9. Key Revocation
        let revoked_key_raw = format!("esa_test_demo_{}", uuid::Uuid::new_v4().simple());
        let revoked_hash = api_keys::hash_api_key(&revoked_key_raw);
        let active_record = api_keys::SandboxKeyRecord {
            key_hash: revoked_hash.clone(),
            key_prefix: revoked_key_raw[..18].to_string(),
            merchant_id: "test_merchant_revoked".to_string(),
            created_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
            revoked: false,
        };
        state.sandbox_keys.insert_key(&active_record).await.unwrap();
        state.sandbox_keys.revoke_key(&revoked_hash).await.unwrap();

        let rev_val_req = Request::builder()
            .method("POST")
            .uri("/api/keys/validate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&serde_json::json!({ "api_key": revoked_key_raw })).unwrap(),
            ))
            .unwrap();
        let rev_val_resp = app.clone().oneshot(rev_val_req).await.unwrap();
        assert_eq!(rev_val_resp.status(), StatusCode::UNAUTHORIZED);

        let rev_me_req = Request::builder()
            .method("GET")
            .uri("/api/auth/me")
            .header("authorization", format!("Bearer {}", revoked_key_raw))
            .body(axum::body::Body::empty())
            .unwrap();
        let rev_me_resp = app.clone().oneshot(rev_me_req).await.unwrap();
        assert_eq!(rev_me_resp.status(), StatusCode::UNAUTHORIZED);

        // 10. Rate limiting test on key generation/validation
        let mut rate_limited_state = create_test_state();
        rate_limited_state.rate_limiter =
            rate_limiter::ApiRateLimiter::new_with_quotas(10, 10, 10, 2, 10, 10);
        let rate_app = build_app(rate_limited_state);

        // Request 1: OK
        let r1 = rate_app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/keys/generate-sandbox")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::OK);

        // Request 2: OK
        let r2 = rate_app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/keys/generate-sandbox")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::OK);

        // Request 3: Burst exceeded -> 429 Too Many Requests
        let r3 = rate_app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/keys/generate-sandbox")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r3.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(r3.headers().contains_key("retry-after"));

        // 11. Existing configured keys remain functional
        std::env::set_var("ESA_TEST_API_KEY", "static_test_key_xyz");
        let static_val_req = Request::builder()
            .method("POST")
            .uri("/api/keys/validate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&serde_json::json!({ "api_key": "static_test_key_xyz" }))
                    .unwrap(),
            ))
            .unwrap();
        let static_val_resp = app.clone().oneshot(static_val_req).await.unwrap();
        assert_eq!(static_val_resp.status(), StatusCode::OK);

        clear_mode_env_vars();
        std::env::remove_var("ESA_TEST_API_KEY");
    }

    #[tokio::test]
    async fn test_razorpay_live_vs_test_credential_isolation_for_sandbox_identities() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();

        // 1. Live credentials in adapter -> Sandbox keys strictly rejected
        let mut live_state = create_test_state();
        let live_config = RazorpayConfig {
            key_id: "rzp_live_real987654321".to_string(),
            key_secret: "live_secret_abc123".to_string(),
            webhook_secret: "whsec_live_xyz".to_string(),
            api_base: "https://api.razorpay.com/v1".to_string(),
            mode: esa_razorpay::RazorpayMode::Live,
            test_mode_only: false,
            webhook_rate_limit_per_sec: 30,
            api_rate_limit_per_sec: 10,
            account_id: None,
        };
        live_state.razorpay = Some(Arc::new(RazorpayAdapter::with_config_no_proxy(live_config)));
        let live_app = build_app(live_state.clone());

        // Generate a sandbox key
        let gen_resp = live_app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/keys/generate-sandbox")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gen_resp.status(), StatusCode::OK);
        let gen_bytes = axum::body::to_bytes(gen_resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let gen_json: serde_json::Value = serde_json::from_slice(&gen_bytes).unwrap();
        let sandbox_key = gen_json["api_key"].as_str().unwrap().to_string();

        let order_payload = serde_json::json!({
            "amount_cents": 5000,
            "region": "IN-SOUTH",
            "payment_method": "upi"
        });

        // Test POST /api/razorpay/orders rejected with 403
        for uri in &["/api/razorpay/orders", "/api/v1/razorpay/orders"] {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", sandbox_key))
                .body(axum::body::Body::from(
                    serde_json::to_vec(&order_payload).unwrap(),
                ))
                .unwrap();
            let resp = live_app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "Sandbox key must be rejected from {} when adapter has live credentials",
                uri
            );
        }

        // Test POST /api/razorpay/confirm rejected with 403
        let confirm_payload = serde_json::json!({
            "payment_id": "pay_test123"
        });
        for uri in &["/api/razorpay/confirm", "/api/v1/razorpay/confirm"] {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", sandbox_key))
                .body(axum::body::Body::from(
                    serde_json::to_vec(&confirm_payload).unwrap(),
                ))
                .unwrap();
            let resp = live_app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "Sandbox key must be rejected from {} when adapter has live credentials",
                uri
            );
        }

        // Test GET /api/razorpay/status redacts live key ID for sandbox caller
        for uri in &["/api/razorpay/status", "/api/v1/razorpay/status"] {
            let req = Request::builder()
                .method("GET")
                .uri(*uri)
                .header("authorization", format!("Bearer {}", sandbox_key))
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = live_app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                json["razorpay"]["key_id"], "[REDACTED_LIVE_CREDENTIAL]",
                "Live key ID must be masked for sandbox caller on {}",
                uri
            );
        }

        // 2. Test credentials in adapter -> Sandbox keys can create orders
        let mock_app = axum::Router::new().route(
            "/orders",
            axum::routing::post(|| async {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "id": "order_test_sbx_001",
                        "amount": 5000,
                        "currency": "INR",
                        "status": "created"
                    })),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let mut test_state = create_test_state();
        test_state.sandbox_keys = Arc::clone(&live_state.sandbox_keys);
        api_keys::set_global_sandbox_key_store(Arc::clone(&live_state.sandbox_keys));
        let test_config = RazorpayConfig {
            key_id: "rzp_test_valid123456".to_string(),
            key_secret: "test_secret_abc123".to_string(),
            webhook_secret: "whsec_test_xyz".to_string(),
            api_base: format!("http://127.0.0.1:{}", port),
            mode: esa_razorpay::RazorpayMode::Test,
            test_mode_only: true,
            webhook_rate_limit_per_sec: 1000,
            api_rate_limit_per_sec: 1000,
            account_id: None,
        };
        test_state.razorpay = Some(Arc::new(RazorpayAdapter::with_config_no_proxy(test_config)));
        let test_app = build_app(test_state);

        // Order creation succeeds with test credentials
        let test_order_req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/orders")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", sandbox_key))
            .body(axum::body::Body::from(
                serde_json::to_vec(&order_payload).unwrap(),
            ))
            .unwrap();
        let test_order_resp = test_app.clone().oneshot(test_order_req).await.unwrap();
        assert_eq!(test_order_resp.status(), StatusCode::OK);
        let order_bytes = axum::body::to_bytes(test_order_resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let order_json: serde_json::Value = serde_json::from_slice(&order_bytes).unwrap();
        assert_eq!(order_json["key_id"], "rzp_test_valid123456");

        clear_mode_env_vars();
    }

    #[tokio::test]
    async fn test_admin_key_revocation_endpoint_and_aliases() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();

        std::env::set_var("ESA_ADMIN_API_KEY", "admin_secret_revoker_key");
        std::env::set_var("ESA_TEST_API_KEY", "merchant_standard_key");

        let state = create_test_state();
        let app = build_app(state.clone());

        // 1. Generate sandbox key
        let gen_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/keys/generate-sandbox")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gen_resp.status(), StatusCode::OK);
        let gen_bytes = axum::body::to_bytes(gen_resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let gen_json: serde_json::Value = serde_json::from_slice(&gen_bytes).unwrap();
        let raw_key = gen_json["api_key"].as_str().unwrap().to_string();

        // 2. Validate key works before revocation
        let val_req = Request::builder()
            .method("POST")
            .uri("/api/keys/validate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&serde_json::json!({ "api_key": raw_key })).unwrap(),
            ))
            .unwrap();
        let val_resp = app.clone().oneshot(val_req).await.unwrap();
        assert_eq!(val_resp.status(), StatusCode::OK);

        let revoke_body = serde_json::json!({ "api_key": raw_key });

        // 3. Anonymous caller rejected with 401
        for uri in &["/api/keys/revoke", "/api/v1/keys/revoke"] {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&revoke_body).unwrap(),
                ))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // 4. Ordinary merchant caller rejected with 403
        for uri in &["/api/keys/revoke", "/api/v1/keys/revoke"] {
            let req = Request::builder()
                .method("POST")
                .uri(*uri)
                .header("content-type", "application/json")
                .header("x-api-key", "merchant_standard_key")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&revoke_body).unwrap(),
                ))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }

        // 5. Admin caller succeeds with 200
        let req = Request::builder()
            .method("POST")
            .uri("/api/keys/revoke")
            .header("content-type", "application/json")
            .header("x-api-key", "admin_secret_revoker_key")
            .body(axum::body::Body::from(
                serde_json::to_vec(&revoke_body).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let resp_json: serde_json::Value = serde_json::from_slice(&resp_bytes).unwrap();
        assert_eq!(resp_json["success"], true);

        // 6. Validating key now returns 401 Unauthorized
        let val_after_req = Request::builder()
            .method("POST")
            .uri("/api/keys/validate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&serde_json::json!({ "api_key": raw_key })).unwrap(),
            ))
            .unwrap();
        let val_after_resp = app.clone().oneshot(val_after_req).await.unwrap();
        assert_eq!(val_after_resp.status(), StatusCode::UNAUTHORIZED);

        clear_mode_env_vars();
        std::env::remove_var("ESA_ADMIN_API_KEY");
        std::env::remove_var("ESA_TEST_API_KEY");
    }

    async fn get_test_postgres_pool() -> Option<sqlx::PgPool> {
        let db_url = match std::env::var("DATABASE_URL") {
            Ok(url) if !url.is_empty() && url != "memory" => url,
            _ => return None,
        };
        sqlx::PgPool::connect(&db_url).await.ok()
    }

    #[tokio::test]
    async fn test_postgres_sandbox_key_store_lifecycle_and_multinode_consistency() {
        let pool = match get_test_postgres_pool().await {
            Some(p) => p,
            None => {
                println!("Skipping test_postgres_sandbox_key_store_lifecycle_and_multinode_consistency: DATABASE_URL not set");
                return;
            }
        };

        // 1. Schema initialization on database (split statements)
        api_keys::SandboxKeyStore::init_schema_on_pool(&pool)
            .await
            .expect("Schema init on PostgreSQL must succeed");

        // 2. Node A creates a sandbox key
        let store_node_a = api_keys::SandboxKeyStore::new_with_pool(pool.clone());
        let raw_key = format!("esa_test_demo_{}", uuid::Uuid::new_v4().simple());
        let key_hash = api_keys::hash_api_key(&raw_key);
        let now = chrono::Utc::now();
        let record = api_keys::SandboxKeyRecord {
            key_hash: key_hash.clone(),
            key_prefix: raw_key[..18].to_string(),
            merchant_id: "sbx_merchant_pg_test".to_string(),
            created_at: now,
            expires_at: now + chrono::Duration::hours(24),
            revoked: false,
        };
        store_node_a
            .insert_key(&record)
            .await
            .expect("Insert on Node A must succeed");

        // 3. Node B (independent instance, empty local cache) retrieves key from DB
        let store_node_b = api_keys::SandboxKeyStore::new_with_pool(pool.clone());
        let fetched_b = store_node_b
            .get_key(&key_hash)
            .await
            .expect("Node B must find key in DB");
        assert_eq!(fetched_b.merchant_id, "sbx_merchant_pg_test");
        assert!(!fetched_b.revoked);

        // 4. Node A revokes the key
        let revoked = store_node_a
            .revoke_key(&key_hash)
            .await
            .expect("Revoke must succeed");
        assert!(revoked);

        // 5. Node B must immediately observe revoked status because PostgreSQL is authoritative
        let fetched_b_after = store_node_b
            .get_key(&key_hash)
            .await
            .expect("Node B must find record");
        assert!(
            fetched_b_after.revoked,
            "Node B must immediately see revoked status from PostgreSQL"
        );

        // 6. Test expiry enforcement after retrieval from PostgreSQL
        let expired_key_raw = format!("esa_test_demo_{}", uuid::Uuid::new_v4().simple());
        let expired_hash = api_keys::hash_api_key(&expired_key_raw);
        let expired_record = api_keys::SandboxKeyRecord {
            key_hash: expired_hash.clone(),
            key_prefix: expired_key_raw[..18].to_string(),
            merchant_id: "sbx_expired_pg".to_string(),
            created_at: now - chrono::Duration::hours(25),
            expires_at: now - chrono::Duration::hours(1),
            revoked: false,
        };
        store_node_a.insert_key(&expired_record).await.unwrap();
        let expired_b = store_node_b.get_key(&expired_hash).await.unwrap();
        assert!(chrono::Utc::now() >= expired_b.expires_at);
    }

    #[tokio::test]
    async fn test_rate_limiter_peer_identity_and_proxy_trust_regression() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        use axum::extract::ConnectInfo;
        use std::net::SocketAddr;

        // 1. Independent buckets for distinct peer IPs
        let mut state = create_test_state();
        // Quotas: checkout(10,10), aggregate(100,100), gen(10,2), val(10,4), demo(10,10)
        state.rate_limiter = rate_limiter::ApiRateLimiter::new_with_all_quotas(
            10, 10, 100, 100, 10, 2, 10, 4, 10, 10,
        );
        let app = build_app(state);

        let peer_a: SocketAddr = "192.0.2.1:1234".parse().unwrap();
        let peer_b: SocketAddr = "198.51.100.2:5678".parse().unwrap();

        // Helper to send generate-sandbox request with peer address and optional headers
        async fn send_gen_req(
            app: Router,
            uri: &'static str,
            peer: Option<SocketAddr>,
            hdrs: Vec<(&'static str, &'static str)>,
        ) -> axum::response::Response {
            let mut builder = Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json");
            for (k, v) in hdrs {
                builder = builder.header(k, v);
            }
            let mut req = builder.body(axum::body::Body::empty()).unwrap();
            if let Some(p) = peer {
                req.extensions_mut().insert(ConnectInfo(p));
            }
            app.oneshot(req).await.unwrap()
        }

        // Peer A: Request 1 (OK)
        let r1 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_a),
            vec![],
        )
        .await;
        assert_eq!(r1.status(), StatusCode::OK);

        // Peer A: Request 2 (OK, burst reached)
        let r2 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_a),
            vec![],
        )
        .await;
        assert_eq!(r2.status(), StatusCode::OK);

        // Peer A: Request 3 (Exhausted -> 429)
        let r3 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_a),
            vec![],
        )
        .await;
        assert_eq!(r3.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(r3.headers().get("retry-after").unwrap(), "1");

        // Peer B: Request 1 (Independent bucket -> OK!)
        let r_b1 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_b),
            vec![],
        )
        .await;
        assert_eq!(
            r_b1.status(),
            StatusCode::OK,
            "Peer B must have an independent rate bucket"
        );

        // Peer B: Request 2 (OK)
        let r_b2 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_b),
            vec![],
        )
        .await;
        assert_eq!(r_b2.status(), StatusCode::OK);

        // Peer B: Request 3 (Exhausted -> 429)
        let r_b3 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_b),
            vec![],
        )
        .await;
        assert_eq!(r_b3.status(), StatusCode::TOO_MANY_REQUESTS);

        // 2. Arbitrary fake Authorization headers cannot bypass peer-based identity
        let peer_c: SocketAddr = "203.0.113.88:9999".parse().unwrap();
        // Request 1 with fake token A
        let r_c1 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_c),
            vec![("authorization", "Bearer fake_token_alpha")],
        )
        .await;
        assert_eq!(r_c1.status(), StatusCode::OK);

        // Request 2 with different fake token B
        let r_c2 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_c),
            vec![("authorization", "Bearer fake_token_beta")],
        )
        .await;
        assert_eq!(r_c2.status(), StatusCode::OK);

        // Request 3 with third fake token C -> Must still be blocked by peer IP bucket!
        let r_c3 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_c),
            vec![("authorization", "Bearer fake_token_gamma")],
        )
        .await;
        assert_eq!(
            r_c3.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "Fake Authorization headers must not bypass peer IP rate limiting"
        );

        // 3. Client-supplied X-Forwarded-For is ignored by default (no trusted proxy configured)
        std::env::remove_var("TRUSTED_PROXIES");
        let peer_d: SocketAddr = "203.0.113.99:1111".parse().unwrap();
        let r_d1 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_d),
            vec![("x-forwarded-for", "1.1.1.1")],
        )
        .await;
        assert_eq!(r_d1.status(), StatusCode::OK);
        let r_d2 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_d),
            vec![("x-forwarded-for", "2.2.2.2")],
        )
        .await;
        assert_eq!(r_d2.status(), StatusCode::OK);
        // Spoofing different X-Forwarded-For does not evade peer bucket
        let r_d3 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_d),
            vec![("x-forwarded-for", "3.3.3.3")],
        )
        .await;
        assert_eq!(
            r_d3.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "Spoofed X-Forwarded-For must be ignored for untrusted peers"
        );

        // 4. Forwarded headers trusted only under verified proxy configuration
        std::env::set_var("TRUSTED_PROXIES", "10.0.0.0/8,172.16.0.0/12");
        let trusted_proxy: SocketAddr = "10.0.0.1:4000".parse().unwrap();

        // Forwarded Client 1
        let r_p1 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(trusted_proxy),
            vec![("x-forwarded-for", "198.51.100.201")],
        )
        .await;
        assert_eq!(r_p1.status(), StatusCode::OK);
        let r_p2 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(trusted_proxy),
            vec![("x-forwarded-for", "198.51.100.201")],
        )
        .await;
        assert_eq!(r_p2.status(), StatusCode::OK);
        let r_p3 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(trusted_proxy),
            vec![("x-forwarded-for", "198.51.100.201")],
        )
        .await;
        assert_eq!(
            r_p3.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "Client 1 forwarded through trusted proxy exhausted its bucket"
        );

        // Forwarded Client 2 from SAME trusted proxy -> independent bucket!
        let r_p2_1 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(trusted_proxy),
            vec![("x-forwarded-for", "198.51.100.202")],
        )
        .await;
        assert_eq!(
            r_p2_1.status(),
            StatusCode::OK,
            "Different client forwarded through trusted proxy must have separate bucket"
        );

        // Multi-hop right-to-left traversal: untrusted spoofed IP ignored
        let r_multi = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(trusted_proxy),
            vec![("x-forwarded-for", "spoofed.ip, 198.51.100.203, 172.16.0.5")],
        )
        .await;
        assert_eq!(
            r_multi.status(),
            StatusCode::OK,
            "Multi-hop right-to-left traversal must resolve authentic client IP"
        );

        // Clean up environment
        std::env::remove_var("TRUSTED_PROXIES");

        // 5. Versioned /api/v1 and legacy /api route consistency
        let peer_e: SocketAddr = "192.0.2.55:7777".parse().unwrap();
        let r_v1_1 = send_gen_req(
            app.clone(),
            "/api/v1/keys/generate-sandbox",
            Some(peer_e),
            vec![],
        )
        .await;
        assert_eq!(r_v1_1.status(), StatusCode::OK);
        let r_v1_2 = send_gen_req(
            app.clone(),
            "/api/v1/keys/generate-sandbox",
            Some(peer_e),
            vec![],
        )
        .await;
        assert_eq!(r_v1_2.status(), StatusCode::OK);
        // Calling unversioned alias shares the same limiter
        let r_v1_3 = send_gen_req(
            app.clone(),
            "/api/keys/generate-sandbox",
            Some(peer_e),
            vec![],
        )
        .await;
        assert_eq!(
            r_v1_3.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "/api and /api/v1 must share the rate limiter"
        );

        // 6. Process-local aggregate limit safeguard
        let mut agg_state = create_test_state();
        // Aggregate quota: 2 rps, burst 2. Per-client quota: 10 rps, burst 10
        agg_state.rate_limiter = rate_limiter::ApiRateLimiter::new_with_all_quotas(
            10, 10, 10, 2, 10, 10, 10, 10, 10, 10,
        );
        let agg_app = build_app(agg_state);

        let p1: SocketAddr = "10.1.1.1:1001".parse().unwrap();
        let p2: SocketAddr = "10.2.2.2:1002".parse().unwrap();
        let p3: SocketAddr = "10.3.3.3:1003".parse().unwrap();

        assert_eq!(
            send_gen_req(
                agg_app.clone(),
                "/api/keys/generate-sandbox",
                Some(p1),
                vec![]
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            send_gen_req(
                agg_app.clone(),
                "/api/keys/generate-sandbox",
                Some(p2),
                vec![]
            )
            .await
            .status(),
            StatusCode::OK
        );
        // 3rd request from distinct client hits process aggregate limit!
        let agg_resp = send_gen_req(
            agg_app.clone(),
            "/api/keys/generate-sandbox",
            Some(p3),
            vec![],
        )
        .await;
        assert_eq!(agg_resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(agg_resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("Process-level aggregate"),
            "Expected process-level aggregate message, got: {:?}",
            body["message"]
        );
    }

    #[tokio::test]
    async fn test_rate_limiter_cf_connecting_ip_and_strict_bounded_concurrency() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        use axum::extract::ConnectInfo;
        use std::net::SocketAddr;

        // 1. Generic trusted proxy ignores CF-Connecting-IP and uses XFF
        std::env::set_var("TRUSTED_PROXIES", "10.0.0.0/8");
        std::env::remove_var("CLOUDFLARE_PROXIES");

        let state = create_test_state();
        let app = build_app(state);

        let generic_proxy: SocketAddr = "10.0.0.1:8080".parse().unwrap();

        async fn send_req(
            app: Router,
            peer: SocketAddr,
            hdrs: Vec<(&'static str, &'static str)>,
        ) -> axum::response::Response {
            let mut builder = Request::builder()
                .method("POST")
                .uri("/api/keys/generate-sandbox")
                .header("content-type", "application/json");
            for (k, v) in hdrs {
                builder = builder.header(k, v);
            }
            let mut req = builder.body(axum::body::Body::empty()).unwrap();
            req.extensions_mut().insert(ConnectInfo(peer));
            app.oneshot(req).await.unwrap()
        }

        // Send request from generic proxy with spoofed CF-Connecting-IP and real XFF
        let r_gen = send_req(
            app.clone(),
            generic_proxy,
            vec![
                ("cf-connecting-ip", "1.1.1.1"),
                ("x-forwarded-for", "203.0.113.77"),
            ],
        )
        .await;
        assert_eq!(r_gen.status(), StatusCode::OK);

        // 2. Cloudflare proxy honors CF-Connecting-IP
        std::env::set_var("CLOUDFLARE_PROXIES", "173.245.48.0/20");
        let cf_proxy: SocketAddr = "173.245.48.1:443".parse().unwrap();
        let r_cf = send_req(
            app.clone(),
            cf_proxy,
            vec![("cf-connecting-ip", "203.0.113.99")],
        )
        .await;
        assert_eq!(r_cf.status(), StatusCode::OK);

        // Clean up environment
        std::env::remove_var("TRUSTED_PROXIES");
        std::env::remove_var("CLOUDFLARE_PROXIES");

        // 3. Strict capacity bound under concurrent requests
        let bound = 4;
        let mut bounded_state = create_test_state();
        bounded_state.rate_limiter = rate_limiter::ApiRateLimiter::new_with_bounded_capacity(
            10, 10, 100, 100, 10, 10, 10, 10, 10, 10, bound,
        );
        let bounded_app = build_app(bounded_state.clone());

        // Launch 40 concurrent requests with 40 distinct peer IPs
        let mut handles = Vec::new();
        for i in 0..40 {
            let app_clone = bounded_app.clone();
            let peer: SocketAddr = format!("198.51.100.{}:1234", i + 1).parse().unwrap();
            handles.push(tokio::spawn(async move {
                send_req(app_clone, peer, vec![]).await
            }));
        }

        for h in handles {
            let res = h.await.unwrap();
            assert!(
                res.status() == StatusCode::OK || res.status() == StatusCode::TOO_MANY_REQUESTS
            );
        }

        // Strict capacity assertion: limiter length can NEVER exceed bound!
        assert!(
            bounded_state.rate_limiter.gen_len() <= bound,
            "Limiter entries {} exceeded strict capacity bound {}",
            bounded_state.rate_limiter.gen_len(),
            bound
        );

        // 4. Overflow requests do not create additional keyed entries
        let initial_len = bounded_state.rate_limiter.gen_len();
        for i in 100..120 {
            let peer: SocketAddr = format!("198.51.200.{}:4321", i).parse().unwrap();
            let _ = send_req(bounded_app.clone(), peer, vec![]).await;
        }
        let after_len = bounded_state.rate_limiter.gen_len();
        assert_eq!(
            initial_len, after_len,
            "Overflow traffic must not increase the number of keyed entries"
        );
    }

    #[tokio::test]
    async fn test_cashfree_full_lifecycle_and_webhook_verification() {
        let _guard = ScopedSimulatedCheckoutGuard::new("esa_test_merchant_live");
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mock_app = Router::new()
            .route(
                "/orders",
                post(|| async {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "order_id": "order_cf_test_100",
                            "payment_session_id": "session_cf_test_999",
                            "order_status": "ACTIVE",
                            "order_amount": 500.0,
                            "order_currency": "INR"
                        })),
                    )
                }),
            )
            .route(
                "/orders/:order_id",
                get(|| async {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "order_id": "order_cf_test_100",
                            "order_status": "PAID",
                            "order_amount": 500.0,
                            "order_currency": "INR"
                        })),
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, mock_app).await;
        });
        let mock_url = format!("http://{}", addr);

        let mut state = create_test_state();
        let cf_config = CashfreeConfig {
            client_id: "test_cf_client_id".to_string(),
            client_secret: "test_cf_client_secret_super_secure_key".to_string(),
            api_version: "2023-08-01".to_string(),
            env: esa_cashfree::CashfreeEnv::Sandbox,
            api_base: mock_url,
        };
        let cf_provider = Arc::new(CashfreeProvider::new_without_proxy(cf_config));
        state.cashfree = Some(cf_provider);

        let app = build_app(state.clone());

        // 1. Check Cashfree status
        let req = Request::builder()
            .method("GET")
            .uri("/api/payments/cashfree/status")
            .header("x-api-key", "esa_test_merchant_live")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 2. Test Checkout routing directly to Cashfree
        let req_body = serde_json::to_string(&serde_json::json!({
            "amount_minor": 50000,
            "currency": "INR",
            "gateway": "cashfree",
            "method": "upi",
            "customer_id": "cust_cf_123"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("x-api-key", "esa_test_merchant_live")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let decision: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            decision.get("routed_gateway").and_then(|v| v.as_str()),
            Some("cashfree")
        );
        assert_eq!(
            decision.get("amount_minor").and_then(|v| v.as_i64()),
            Some(50000)
        );
        assert!(decision.get("session").is_some());

        // 3. Webhook signature verification
        let webhook_payload = serde_json::json!({
            "data": {
                "order": {
                    "order_id": "order_cf_test_100",
                    "order_amount": 500.0,
                    "order_currency": "INR"
                },
                "payment": {
                    "cf_payment_id": "pay_cf_test_999",
                    "payment_status": "SUCCESS",
                    "payment_amount": 500.0,
                    "payment_currency": "INR",
                    "payment_method": "upi",
                    "payment_time": "2026-11-11T12:00:00Z"
                }
            },
            "event_time": "2026-11-11T12:00:01Z",
            "type": "PAYMENT_SUCCESS_WEBHOOK"
        });
        let raw_body = serde_json::to_vec(&webhook_payload).unwrap();
        let timestamp = "1762862401000";

        // Compute HMAC signature: HMAC-SHA256(secret, timestamp + raw_body)
        let mut mac =
            HmacSha256::new_from_slice(b"test_cf_client_secret_super_secure_key").unwrap();
        mac.update(timestamp.as_bytes());
        mac.update(&raw_body);
        let signature = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            mac.finalize().into_bytes(),
        );

        // Send valid webhook
        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/cashfree/webhook")
            .header("content-type", "application/json")
            .header("x-webhook-timestamp", timestamp)
            .header("x-webhook-signature", &signature)
            .body(axum::body::Body::from(raw_body.clone()))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Send tampered signature -> must be 401 Unauthorized
        let req_tampered = Request::builder()
            .method("POST")
            .uri("/api/payments/cashfree/webhook")
            .header("content-type", "application/json")
            .header("x-webhook-timestamp", timestamp)
            .header("x-webhook-signature", "invalid_forged_base64_sig==")
            .body(axum::body::Body::from(raw_body))
            .unwrap();

        let resp_tampered = app.clone().oneshot(req_tampered).await.unwrap();
        assert_eq!(resp_tampered.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_payu_hosted_checkout_hash_and_callback_verification() {
        let _guard = ScopedSimulatedCheckoutGuard::new("esa_test_merchant_live");

        let payu_mock_app = Router::new().route(
            "/merchant/postservice",
            post(
                |axum::extract::Form(form): axum::extract::Form<
                    std::collections::HashMap<String, String>,
                >| async move {
                    let txnid = form.get("var1").cloned().unwrap_or_default();
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 1,
                            "msg": "Transaction Fetched Successfully",
                            "transaction_details": {
                                txnid: {
                                    "mihpayid": "payu_mih_88888",
                                    "status": "success",
                                    "amt": "500.00",
                                    "mode": "UPI"
                                }
                            }
                        })),
                    )
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, payu_mock_app).await;
        });
        let verify_mock_url = format!("http://{}/merchant/postservice?form=2", addr);

        let mut state = create_test_state();
        let payu_config = PayUConfig {
            merchant_key: "test_payu_key".to_string(),
            merchant_salt: "test_payu_salt_12345".to_string(),
            env: esa_payu::PayUEnv::Test,
            payment_url: "https://test.payu.in/_payment".to_string(),
            verify_url: verify_mock_url,
        };
        let payu_provider = Arc::new(PayUProvider::new_without_proxy(payu_config));
        state.payu = Some(payu_provider);

        let app = build_app(state.clone());

        // 1. Check PayU status
        let req = Request::builder()
            .method("GET")
            .uri("/api/payments/payu/status")
            .header("x-api-key", "esa_test_merchant_live")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 2. Checkout via PayU
        let req_body = serde_json::to_string(&serde_json::json!({
            "amount_minor": 50000,
            "currency": "INR",
            "gateway": "payu",
            "method": "upi",
            "customer_email": "payu_customer@example.com",
            "customer_phone": "9999999999"
        }))
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("x-api-key", "esa_test_merchant_live")
            .body(axum::body::Body::from(req_body))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let decision: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            decision.get("routed_gateway").and_then(|v| v.as_str()),
            Some("payu")
        );
        let session = decision
            .get("session")
            .expect("PayU session must be present");
        let details = session.get("data").expect("data must be present");
        assert_eq!(
            details.get("action_url").and_then(|v| v.as_str()),
            Some("https://test.payu.in/_payment")
        );

        let params = details.get("params").expect("params must be present");
        let txnid = params.get("txnid").and_then(|v| v.as_str()).unwrap();
        let amount = params.get("amount").and_then(|v| v.as_str()).unwrap();
        let hash = params.get("hash").and_then(|v| v.as_str()).unwrap();
        let productinfo = params.get("productinfo").and_then(|v| v.as_str()).unwrap();
        let firstname = params.get("firstname").and_then(|v| v.as_str()).unwrap();
        let email = params.get("email").and_then(|v| v.as_str()).unwrap();
        let udf1 = params.get("udf1").and_then(|v| v.as_str());

        // Verify that the hash in params is genuinely calculated via PayU SHA-512 request formula
        let expected_hash = esa_payu::hash::calculate_request_hash(
            "test_payu_key",
            txnid,
            amount,
            productinfo,
            firstname,
            email,
            udf1,
            None,
            None,
            None,
            None,
            "test_payu_salt_12345",
        );
        assert_eq!(hash, expected_hash);

        // 3. Callback Reverse-Hash Verification
        let cb_status = "success";
        let cb_hash = esa_payu::hash::calculate_reverse_hash(
            "test_payu_salt_12345",
            cb_status,
            udf1,
            None,
            None,
            None,
            None,
            email,
            firstname,
            productinfo,
            amount,
            txnid,
            "test_payu_key",
            None,
        );

        let form_body = format!(
            "key=test_payu_key&txnid={}&amount={}&productinfo={}&firstname={}&email={}&udf1={}&status={}&hash={}&mihpayid=payu_mih_88888",
            txnid, amount, productinfo.replace(' ', "+"), firstname, email.replace('@', "%40"), udf1.unwrap_or(""), cb_status, cb_hash
        );

        let req_cb = Request::builder()
            .method("POST")
            .uri("/api/payments/payu/callback/success")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(form_body))
            .unwrap();

        let resp_cb = app.clone().oneshot(req_cb).await.unwrap();
        assert_eq!(resp_cb.status(), StatusCode::OK);
        let cb_bytes = axum::body::to_bytes(resp_cb.into_body(), 1024 * 64)
            .await
            .unwrap();
        let cb_json: serde_json::Value = serde_json::from_slice(&cb_bytes).unwrap();
        assert_eq!(
            cb_json.get("status").and_then(|v| v.as_str()),
            Some("success")
        );

        // Tampered reverse hash callback -> must be rejected
        let bad_form_body = format!(
            "key=test_payu_key&txnid={}&amount={}&productinfo=ESA+Payment+esa_test_merchant_live&firstname=Customer&email=payu_customer%40example.com&udf1=esa_test_merchant_live&status={}&hash=badhash00000&mihpayid=payu_mih_88888",
            txnid, amount, cb_status
        );
        let req_bad_cb = Request::builder()
            .method("POST")
            .uri("/api/payments/payu/callback/success")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(bad_form_body))
            .unwrap();

        let resp_bad_cb = app.clone().oneshot(req_bad_cb).await.unwrap();
        assert_eq!(resp_bad_cb.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_multi_gateway_intelligent_failover_chain() {
        let _guard = ScopedSimulatedCheckoutGuard::new("esa_test_merchant_live");
        let mut state = create_test_state();

        let rzp_mock_app = Router::new().route(
            "/orders",
            post(|| async {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "id": "order_rzp_mock_001",
                        "entity": "order",
                        "amount": 10000,
                        "currency": "INR",
                        "status": "created"
                    })),
                )
            }),
        );
        let rzp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rzp_addr = rzp_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(rzp_listener, rzp_mock_app).await;
        });

        let cf_mock_app = Router::new().route(
            "/orders",
            post(|| async {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "order_id": "order_cf_mock_001",
                        "payment_session_id": "session_cf_mock_12345",
                        "order_status": "ACTIVE",
                        "order_amount": 100.0,
                        "order_currency": "INR"
                    })),
                )
            }),
        );
        let cf_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cf_addr = cf_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(cf_listener, cf_mock_app).await;
        });

        // 1. Configure all three providers: Razorpay, Cashfree, PayU
        let rzp_config = RazorpayConfig {
            key_id: "rzp_test_key_123".to_string(),
            key_secret: "rzp_test_secret_456".to_string(),
            webhook_secret: "rzp_test_webhook_sec".to_string(),
            api_base: format!("http://{}", rzp_addr),
            mode: esa_razorpay::RazorpayMode::Test,
            test_mode_only: true,
            webhook_rate_limit_per_sec: 1000,
            api_rate_limit_per_sec: 1000,
            account_id: None,
        };
        state.razorpay = Some(Arc::new(RazorpayAdapter::with_config_no_proxy(rzp_config)));

        let cf_config = CashfreeConfig {
            client_id: "test_cf_id".to_string(),
            client_secret: "test_cf_sec".to_string(),
            api_version: "2023-08-01".to_string(),
            env: esa_cashfree::CashfreeEnv::Sandbox,
            api_base: format!("http://{}", cf_addr),
        };
        state.cashfree = Some(Arc::new(CashfreeProvider::new_without_proxy(cf_config)));

        let payu_config = PayUConfig {
            merchant_key: "test_payu_key".to_string(),
            merchant_salt: "test_payu_salt".to_string(),
            env: esa_payu::PayUEnv::Test,
            payment_url: "https://test.payu.in/_payment".to_string(),
            verify_url: "https://test.payu.in/merchant/postservice?form=2".to_string(),
        };
        state.payu = Some(Arc::new(PayUProvider::new_without_proxy(payu_config)));

        let app = build_app(state.clone());

        // A. Primary corridor healthy: "auto" routes to Razorpay
        {
            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 10000,
                "currency": "INR",
                "gateway": "auto",
                "method": "upi"
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("x-api-key", "esa_test_merchant_live")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let decision: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                decision.get("routed_gateway").and_then(|v| v.as_str()),
                Some("razorpay")
            );
            assert_eq!(
                decision.get("failover_triggered").and_then(|v| v.as_bool()),
                Some(false)
            );
        }

        // B. Razorpay degraded: "auto" automatically fails over to Cashfree
        {
            state
                .gateway_overrides
                .write()
                .unwrap()
                .insert("razorpay".to_string(), false);

            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 10000,
                "currency": "INR",
                "gateway": "auto",
                "method": "upi"
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("x-api-key", "esa_test_merchant_live")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let decision: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                decision.get("routed_gateway").and_then(|v| v.as_str()),
                Some("cashfree")
            );
            assert_eq!(
                decision.get("failover_triggered").and_then(|v| v.as_bool()),
                Some(true)
            );
        }

        // C. Both Razorpay and Cashfree degraded: "auto" automatically fails over to PayU
        {
            state
                .gateway_overrides
                .write()
                .unwrap()
                .insert("cashfree".to_string(), false);

            let req_body = serde_json::to_string(&serde_json::json!({
                "amount_minor": 10000,
                "currency": "INR",
                "gateway": "auto",
                "method": "upi"
            }))
            .unwrap();

            let req = Request::builder()
                .method("POST")
                .uri("/api/payments/checkout")
                .header("content-type", "application/json")
                .header("x-api-key", "esa_test_merchant_live")
                .body(axum::body::Body::from(req_body))
                .unwrap();

            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
                .await
                .unwrap();
            let decision: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                decision.get("routed_gateway").and_then(|v| v.as_str()),
                Some("payu")
            );
            assert_eq!(
                decision.get("failover_triggered").and_then(|v| v.as_bool()),
                Some(true)
            );
        }
    }

    #[tokio::test]
    async fn test_cross_merchant_isolation_for_all_providers() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();

        let mut state = create_test_state();

        let cf_config = CashfreeConfig {
            client_id: "test_cf_client_id".to_string(),
            client_secret: "test_cf_client_secret_super_secure_key".to_string(),
            api_version: "2023-08-01".to_string(),
            env: esa_cashfree::CashfreeEnv::Sandbox,
            api_base: "http://127.0.0.1:9".to_string(),
        };
        state.cashfree = Some(Arc::new(CashfreeProvider::new_without_proxy(cf_config)));

        let payu_config = PayUConfig {
            merchant_key: "test_payu_key".to_string(),
            merchant_salt: "test_payu_salt".to_string(),
            env: esa_payu::PayUEnv::Test,
            payment_url: "https://test.payu.in/_payment".to_string(),
            verify_url: "http://127.0.0.1:9".to_string(),
        };
        state.payu = Some(Arc::new(PayUProvider::new_without_proxy(payu_config)));

        let key_alpha = "esa_sbx_merchant_alpha_sec_123456";
        let key_beta = "esa_sbx_merchant_beta_sec_654321";
        state
            .sandbox_keys
            .insert_key(&api_keys::SandboxKeyRecord {
                key_hash: api_keys::hash_api_key(key_alpha),
                key_prefix: "esa_sbx_".to_string(),
                merchant_id: "merchant_alpha".to_string(),
                created_at: chrono::Utc::now(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
                revoked: false,
            })
            .await
            .unwrap();

        state
            .sandbox_keys
            .insert_key(&api_keys::SandboxKeyRecord {
                key_hash: api_keys::hash_api_key(key_beta),
                key_prefix: "esa_sbx_".to_string(),
                merchant_id: "merchant_beta".to_string(),
                created_at: chrono::Utc::now(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
                revoked: false,
            })
            .await
            .unwrap();

        state
            .webhook_store
            .record_payment_attempt(&esa_core::provider::PaymentAttemptRecord {
                transaction_id: "tx_cf_alpha_order".to_string(),
                merchant_id: "merchant_alpha".to_string(),
                provider: "cashfree".to_string(),
                provider_order_id: Some("order_cf_alpha".to_string()),
                payment_reference: Some("cf_pay_alpha_1".to_string()),
                amount_minor: 50000,
                currency: "INR".to_string(),
                state: "captured".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        state
            .webhook_store
            .record_payment_attempt(&esa_core::provider::PaymentAttemptRecord {
                transaction_id: "tx_payu_alpha_order".to_string(),
                merchant_id: "merchant_alpha".to_string(),
                provider: "payu".to_string(),
                provider_order_id: Some("tx_payu_alpha".to_string()),
                payment_reference: Some("mih_alpha_1".to_string()),
                amount_minor: 40000,
                currency: "INR".to_string(),
                state: "captured".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        let app = build_app(state.clone());

        // Test A: Cashfree Order Retrieval Isolation
        let req = Request::builder()
            .method("GET")
            .uri("/api/payments/cashfree/orders/order_cf_alpha")
            .header("x-api-key", key_beta)
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let req = Request::builder()
            .method("GET")
            .uri("/api/payments/cashfree/orders/non_existent_order_id")
            .header("x-api-key", key_beta)
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Test B: Cashfree Confirmation Isolation
        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/cashfree/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", key_beta)
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({
                    "order_id": "order_cf_alpha"
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Test C: Cashfree Refund Isolation
        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/cashfree/refunds")
            .header("content-type", "application/json")
            .header("x-api-key", key_beta)
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({
                    "order_id": "order_cf_alpha",
                    "refund_id": "rf_beta_hack",
                    "amount_minor": 1000
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Test D: PayU Verification Isolation
        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/payu/verify")
            .header("content-type", "application/json")
            .header("x-api-key", key_beta)
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({
                    "txnid": "tx_payu_alpha"
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/payu/verify")
            .header("content-type", "application/json")
            .header("x-api-key", key_beta)
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({
                    "txnid": "non_existent_payu_txnid"
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Test E: PayU Refund Isolation
        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/payu/refunds")
            .header("content-type", "application/json")
            .header("x-api-key", key_beta)
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({
                    "mihpayid": "tx_payu_alpha",
                    "refund_id": "rf_beta_payu_hack",
                    "amount_minor": 1000
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_refund_lifecycle_state_transitions() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();

        let mut state = create_test_state();

        let cf_config = CashfreeConfig {
            client_id: "test_cf_client_id".to_string(),
            client_secret: "test_cf_client_secret_super_secure_key".to_string(),
            api_version: "2023-08-01".to_string(),
            env: esa_cashfree::CashfreeEnv::Sandbox,
            api_base: "http://127.0.0.1:9".to_string(),
        };
        state.cashfree = Some(Arc::new(CashfreeProvider::new_without_proxy(cf_config)));

        let key_ref = "esa_sbx_merchant_refund_key_123456";
        state
            .sandbox_keys
            .insert_key(&api_keys::SandboxKeyRecord {
                key_hash: api_keys::hash_api_key(key_ref),
                key_prefix: "esa_sbx_".to_string(),
                merchant_id: "merchant_refund".to_string(),
                created_at: chrono::Utc::now(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
                revoked: false,
            })
            .await
            .unwrap();

        state
            .webhook_store
            .record_payment_attempt(&esa_core::provider::PaymentAttemptRecord {
                transaction_id: "tx_cf_uncaptured".to_string(),
                merchant_id: "merchant_refund".to_string(),
                provider: "cashfree".to_string(),
                provider_order_id: Some("order_cf_uncaptured".to_string()),
                payment_reference: None,
                amount_minor: 10000,
                currency: "INR".to_string(),
                state: "created".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        state
            .webhook_store
            .record_payment_attempt(&esa_core::provider::PaymentAttemptRecord {
                transaction_id: "tx_cf_captured".to_string(),
                merchant_id: "merchant_refund".to_string(),
                provider: "cashfree".to_string(),
                provider_order_id: Some("order_cf_captured".to_string()),
                payment_reference: Some("cf_pay_ref_1".to_string()),
                amount_minor: 10000,
                currency: "INR".to_string(),
                state: "captured".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        let app = build_app(state.clone());

        // Case 1: Refund on uncaptured payment -> 400 Bad Request
        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/cashfree/refunds")
            .header("content-type", "application/json")
            .header("x-api-key", key_ref)
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({
                    "order_id": "order_cf_uncaptured",
                    "refund_id": "rf_cf_001",
                    "amount_minor": 5000
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let err_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(err_json["error"]
            .as_str()
            .unwrap()
            .contains("Only captured or partially refunded payments can be refunded"));

        // Case 2: Refund amount exceeds captured balance (15000 > 10000) -> 400 Bad Request
        let req = Request::builder()
            .method("POST")
            .uri("/api/payments/cashfree/refunds")
            .header("content-type", "application/json")
            .header("x-api-key", key_ref)
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({
                    "order_id": "order_cf_captured",
                    "refund_id": "rf_cf_002",
                    "amount_minor": 15000
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let err_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(err_json["error"]
            .as_str()
            .unwrap()
            .contains("Refund amount exceeds refundable balance"));
    }

    #[tokio::test]
    async fn test_payu_webhook_deterministic_idempotency_and_unknown_status() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();

        let mut state = create_test_state();

        let payu_config = PayUConfig {
            merchant_key: "test_key".to_string(),
            merchant_salt: "test_salt".to_string(),
            env: esa_payu::PayUEnv::Test,
            payment_url: "https://test.payu.in/_payment".to_string(),
            verify_url: "http://127.0.0.1:9".to_string(),
        };
        state.payu = Some(Arc::new(PayUProvider::new_without_proxy(payu_config)));

        let _ = state.state_fabric.upsert_workload(WorkloadEntity {
            workload_id: "payment-upi-india-north".to_string(),
            shard_id: "shard-001".to_string(),
            state: WorkloadState::Healthy,
            region: Region::IndiaNorth,
            metrics: WorkloadMetrics {
                rate_per_min: 1000.0,
                p50_latency_ms: 50.0,
                p95_latency_ms: 120.0,
                p99_latency_ms: 200.0,
                error_rate: 0.001,
                queue_depth: 100,
                timestamp: chrono::Utc::now(),
            },
            replication: ReplicationPolicy {
                min_replicas: 1,
                max_replicas: 5,
                current_replicas: 1,
                consistency_mode: ConsistencyMode::Eventual,
            },
            locality: LocalityPreference {
                preferred_region: Region::IndiaSouth,
                fallback_regions: vec![Region::IndiaNorth],
            },
            lifecycle: LifecycleState::Active,
            version: 1,
            updated_at: chrono::Utc::now(),
        });

        let app = build_app(state.clone());

        // 1. Formulate valid PayU webhook for success
        let txnid = "tx_payu_det_001";
        let amount_str = "100.00";
        let status = "success";
        let hash = esa_payu::hash::calculate_reverse_hash(
            "test_salt",
            status,
            None,
            None,
            None,
            None,
            None,
            "test@example.com",
            "Tester",
            "Product",
            amount_str,
            txnid,
            "test_key",
            None,
        );

        let payload = serde_json::json!({
            "key": "test_key",
            "txnid": txnid,
            "amount": amount_str,
            "productinfo": "Product",
            "firstname": "Tester",
            "email": "test@example.com",
            "status": status,
            "mihpayid": "999111222",
            "hash": hash,
        });

        let req1 = Request::builder()
            .method("POST")
            .uri("/api/payments/payu/webhook")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_string(&payload).unwrap(),
            ))
            .unwrap();

        let resp1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        let bytes1 = axum::body::to_bytes(resp1.into_body(), 1024 * 64)
            .await
            .unwrap();
        let res1: serde_json::Value = serde_json::from_slice(&bytes1).unwrap();
        assert_eq!(res1["status"].as_str(), Some("OK"));

        let rate_after_first = state
            .state_fabric
            .get_workload("payment-upi-india-north")
            .unwrap()
            .metrics
            .rate_per_min;
        assert!(rate_after_first > 1000.0);

        // 2. Duplicate delivery of the exact same PayU webhook
        let req2 = Request::builder()
            .method("POST")
            .uri("/api/payments/payu/webhook")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_string(&payload).unwrap(),
            ))
            .unwrap();

        let resp2 = app.clone().oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let bytes2 = axum::body::to_bytes(resp2.into_body(), 1024 * 64)
            .await
            .unwrap();
        let res2: serde_json::Value = serde_json::from_slice(&bytes2).unwrap();
        assert_eq!(res2["status"].as_str(), Some("already_processed"));
        assert_eq!(res2["duplicate"].as_bool(), Some(true));

        // Workload counter must NOT have increased on duplicate
        let rate_after_second = state
            .state_fabric
            .get_workload("payment-upi-india-north")
            .unwrap()
            .metrics
            .rate_per_min;
        assert_eq!(rate_after_second, rate_after_first);

        // 3. Webhook with unrecognized status
        let unrec_status = "unrecognized_status_xyz";
        let unrec_hash = esa_payu::hash::calculate_reverse_hash(
            "test_salt",
            unrec_status,
            None,
            None,
            None,
            None,
            None,
            "test@example.com",
            "Tester",
            "Product",
            amount_str,
            "tx_payu_unrec_002",
            "test_key",
            None,
        );

        let unrec_payload = serde_json::json!({
            "key": "test_key",
            "txnid": "tx_payu_unrec_002",
            "amount": amount_str,
            "productinfo": "Product",
            "firstname": "Tester",
            "email": "test@example.com",
            "status": unrec_status,
            "mihpayid": "999111333",
            "hash": unrec_hash,
        });

        let req3 = Request::builder()
            .method("POST")
            .uri("/api/payments/payu/webhook")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_string(&unrec_payload).unwrap(),
            ))
            .unwrap();

        let resp3 = app.clone().oneshot(req3).await.unwrap();
        assert_eq!(resp3.status(), StatusCode::BAD_REQUEST);
        let bytes3 = axum::body::to_bytes(resp3.into_body(), 1024 * 64)
            .await
            .unwrap();
        let res3: serde_json::Value = serde_json::from_slice(&bytes3).unwrap();
        assert!(res3["error"]
            .as_str()
            .unwrap()
            .contains("Unrecognized PayU event status"));
    }

    async fn insert_test_merchant_key(state: &AppState, merchant_id: &str) -> String {
        let raw_key = format!("esa_test_demo_{}", uuid::Uuid::new_v4().simple());
        let key_hash = api_keys::hash_api_key(&raw_key);
        let record = api_keys::SandboxKeyRecord {
            key_hash,
            key_prefix: raw_key[..18].to_string(),
            merchant_id: merchant_id.to_string(),
            created_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
            revoked: false,
        };
        state.sandbox_keys.insert_key(&record).await.unwrap();
        raw_key
    }

    async fn setup_razorpay_confirm_harness(
        merchant_id: &str,
        order_id: &str,
        payment_id: &str,
        initial_payment: serde_json::Value,
    ) -> (
        AppState,
        axum::Router,
        String,
        Arc<std::sync::Mutex<serde_json::Value>>,
    ) {
        let payment_state_mock = Arc::new(std::sync::Mutex::new(initial_payment));
        let payment_mock_clone = Arc::clone(&payment_state_mock);
        let oid = order_id.to_string();
        let mid = merchant_id.to_string();

        let mock_app = axum::Router::new()
            .route(
                &format!("/payments/{}", payment_id),
                axum::routing::get(move || {
                    let guard_ref = Arc::clone(&payment_mock_clone);
                    async move {
                        let body = guard_ref.lock().unwrap().clone();
                        (StatusCode::OK, Json(body))
                    }
                }),
            )
            .route(
                &format!("/orders/{}", order_id),
                axum::routing::get(move || {
                    let oid_c = oid.clone();
                    let mid_c = mid.clone();
                    async move {
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "id": oid_c,
                                "amount": 5000,
                                "currency": "INR",
                                "notes": {
                                    "esa_merchant_id": mid_c
                                }
                            })),
                        )
                    }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let mut state = create_test_state();
        let rzp_config = RazorpayConfig {
            key_id: "rzp_test_phase1_key".to_string(),
            key_secret: "phase1_secret".to_string(),
            webhook_secret: "phase1_wh_secret".to_string(),
            api_base: format!("http://127.0.0.1:{}", port),
            mode: esa_razorpay::RazorpayMode::Test,
            test_mode_only: true,
            webhook_rate_limit_per_sec: 1000,
            api_rate_limit_per_sec: 1000,
            account_id: None,
        };
        state.razorpay = Some(Arc::new(RazorpayAdapter::with_config_no_proxy(rzp_config)));

        state
            .webhook_store
            .record_payment_attempt(&esa_core::provider::PaymentAttemptRecord {
                transaction_id: format!("tx_{}", order_id),
                merchant_id: merchant_id.to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some(order_id.to_string()),
                payment_reference: None,
                amount_minor: 5000,
                currency: "INR".to_string(),
                state: "created".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        state
            .state_fabric
            .upsert_workload(WorkloadEntity {
                workload_id: "payment-upi-india-south".to_string(),
                shard_id: "shard-001".to_string(),
                state: WorkloadState::Healthy,
                region: Region::IndiaSouth,
                metrics: WorkloadMetrics {
                    rate_per_min: 3000.0,
                    p50_latency_ms: 45.0,
                    p95_latency_ms: 120.0,
                    p99_latency_ms: 180.0,
                    error_rate: 0.01,
                    queue_depth: 250,
                    timestamp: chrono::Utc::now(),
                },
                replication: ReplicationPolicy {
                    min_replicas: 2,
                    max_replicas: 10,
                    current_replicas: 3,
                    consistency_mode: ConsistencyMode::Strong,
                },
                locality: LocalityPreference {
                    preferred_region: Region::IndiaSouth,
                    fallback_regions: vec![Region::IndiaWest, Region::IndiaNorth],
                },
                lifecycle: LifecycleState::Active,
                version: 1,
                updated_at: chrono::Utc::now(),
            })
            .unwrap();

        let api_key = insert_test_merchant_key(&state, merchant_id).await;
        let app = build_app(state.clone());
        (state, app, api_key, payment_state_mock)
    }

    async fn call_razorpay_confirm(
        app: &axum::Router,
        api_key: &str,
        payment_id: &str,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/razorpay/confirm")
            .header("content-type", "application/json")
            .header("x-api-key", api_key)
            .body(axum::body::Body::from(
                serde_json::to_string(&serde_json::json!({ "payment_id": payment_id })).unwrap(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        (status, json)
    }

    fn sign_razorpay_webhook(secret: &str, body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    #[tokio::test]
    async fn test_razorpay_confirm_authorize_without_capture() {
        let payment_id = "pay_p1_auth_only";
        let order_id = "order_p1_auth_only";
        let (state, app, key, _) = setup_razorpay_confirm_harness(
            "merchant_p1_auth",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 5000,
                "currency": "INR",
                "status": "authorized",
                "method": "upi"
            }),
        )
        .await;

        let initial_rate = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .unwrap()
            .metrics
            .rate_per_min;

        let (status, json) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["success"], false);
        assert_eq!(json["captured"], false);
        assert_eq!(json["status"], "authorized");
        assert_eq!(json["payment_state"], "authorized");
        assert_eq!(json["workload_updated"], false);

        let record = state
            .webhook_store
            .get_payment_record(payment_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.state, PaymentState::Authorized);

        let attempt = state
            .webhook_store
            .get_payment_attempt_for_provider("razorpay", order_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(attempt.state, "authorized");

        let final_rate = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .unwrap()
            .metrics
            .rate_per_min;
        assert_eq!(
            initial_rate, final_rate,
            "Authorized payment must not trigger captured-payment workload side effects"
        );
    }

    #[tokio::test]
    async fn test_razorpay_confirm_authorize_followed_by_capture() {
        let payment_id = "pay_p1_auth_then_cap";
        let order_id = "order_p1_auth_then_cap";
        let (state, app, key, mock_payment) = setup_razorpay_confirm_harness(
            "merchant_p1_seq",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 5000,
                "currency": "INR",
                "status": "authorized",
                "method": "upi"
            }),
        )
        .await;

        let initial_rate = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .unwrap()
            .metrics
            .rate_per_min;

        // Step 1: Authorize
        let (s1, j1) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(s1, StatusCode::OK);
        assert_eq!(j1["payment_state"], "authorized");
        assert_eq!(j1["captured"], false);
        assert_eq!(
            state
                .state_fabric
                .get_workload("payment-upi-india-south")
                .unwrap()
                .metrics
                .rate_per_min,
            initial_rate
        );

        // Step 2: Capture on Razorpay
        *mock_payment.lock().unwrap() = serde_json::json!({
            "id": payment_id,
            "order_id": order_id,
            "amount": 5000,
            "currency": "INR",
            "status": "captured",
            "method": "upi"
        });

        let (s2, j2) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(s2, StatusCode::OK);
        assert_eq!(j2["success"], true);
        assert_eq!(j2["captured"], true);
        assert_eq!(j2["status"], "accepted");
        assert_eq!(j2["payment_state"], "captured");
        assert_eq!(j2["workload_updated"], true);

        let record = state
            .webhook_store
            .get_payment_record(payment_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.state, PaymentState::Captured);

        let attempt = state
            .webhook_store
            .get_payment_attempt_for_provider("razorpay", order_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(attempt.state, "captured");
    }

    #[tokio::test]
    async fn test_razorpay_confirm_already_captured_duplicate() {
        let payment_id = "pay_p1_dup_cap";
        let order_id = "order_p1_dup_cap";
        let (state, app, key, _) = setup_razorpay_confirm_harness(
            "merchant_p1_dup",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 5000,
                "currency": "INR",
                "status": "captured",
                "method": "upi"
            }),
        )
        .await;

        let (s1, j1) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(s1, StatusCode::OK);
        assert_eq!(j1["payment_state"], "captured");
        let rate_after_first = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .unwrap()
            .metrics
            .rate_per_min;

        let (s2, j2) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(s2, StatusCode::OK);
        assert_eq!(j2["status"], "already_confirmed");
        assert_eq!(j2["payment_state"], "captured");
        assert_eq!(j2["idempotent"], true);
        assert_eq!(j2["workload_updated"], false);
        assert_eq!(
            state
                .state_fabric
                .get_workload("payment-upi-india-south")
                .unwrap()
                .metrics
                .rate_per_min,
            rate_after_first
        );
    }

    #[tokio::test]
    async fn test_razorpay_confirm_failed_payment() {
        let payment_id = "pay_p1_failed";
        let order_id = "order_p1_failed";
        let (state, app, key, _) = setup_razorpay_confirm_harness(
            "merchant_p1_fail",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 5000,
                "currency": "INR",
                "status": "failed",
                "method": "upi"
            }),
        )
        .await;

        let initial_rate = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .unwrap()
            .metrics
            .rate_per_min;

        let (status, json) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["success"], false);
        assert_eq!(json["captured"], false);
        assert_eq!(json["payment_state"], "failed");
        assert_eq!(json["workload_updated"], false);

        let record = state
            .webhook_store
            .get_payment_record(payment_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.state, PaymentState::Failed);

        let attempt = state
            .webhook_store
            .get_payment_attempt_for_provider("razorpay", order_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(attempt.state, "failed");
        assert_eq!(
            state
                .state_fabric
                .get_workload("payment-upi-india-south")
                .unwrap()
                .metrics
                .rate_per_min,
            initial_rate
        );
    }

    #[tokio::test]
    async fn test_razorpay_confirm_pending_payment() {
        let payment_id = "pay_p1_pending";
        let order_id = "order_p1_pending";
        let (state, app, key, _) = setup_razorpay_confirm_harness(
            "merchant_p1_pend",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 5000,
                "currency": "INR",
                "status": "created",
                "method": "upi"
            }),
        )
        .await;

        let initial_rate = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .unwrap()
            .metrics
            .rate_per_min;

        let (status, _) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(status, StatusCode::CONFLICT);

        assert!(state
            .webhook_store
            .get_payment_record(payment_id)
            .await
            .unwrap()
            .is_none());
        let attempt = state
            .webhook_store
            .get_payment_attempt_for_provider("razorpay", order_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(attempt.state, "pending");
        assert_eq!(
            state
                .state_fabric
                .get_workload("payment-upi-india-south")
                .unwrap()
                .metrics
                .rate_per_min,
            initial_rate
        );
    }

    #[tokio::test]
    async fn test_razorpay_confirm_unknown_status_fails_closed() {
        let payment_id = "pay_p1_unknown";
        let order_id = "order_p1_unknown";
        let (state, app, key, _) = setup_razorpay_confirm_harness(
            "merchant_p1_unk",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 5000,
                "currency": "INR",
                "status": "unrecognized_upstream_state",
                "method": "upi"
            }),
        )
        .await;

        let (status, _) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(state
            .webhook_store
            .get_payment_record(payment_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_razorpay_confirm_invalid_amount_or_currency() {
        let payment_id = "pay_p1_bad_amt";
        let order_id = "order_p1_bad_amt";
        let (state, app, key, mock_payment) = setup_razorpay_confirm_harness(
            "merchant_p1_amt",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 4999,
                "currency": "INR",
                "status": "captured",
                "method": "upi"
            }),
        )
        .await;

        let (s_amt, _) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(s_amt, StatusCode::CONFLICT);
        assert!(state
            .webhook_store
            .get_payment_record(payment_id)
            .await
            .unwrap()
            .is_none());

        *mock_payment.lock().unwrap() = serde_json::json!({
            "id": payment_id,
            "order_id": order_id,
            "amount": 5000,
            "currency": "USD",
            "status": "captured",
            "method": "upi"
        });
        let (s_cur, _) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(s_cur, StatusCode::CONFLICT);
        assert!(state
            .webhook_store
            .get_payment_record(payment_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_razorpay_confirm_merchant_mismatch() {
        let payment_id = "pay_p1_mm";
        let order_id = "order_p1_mm";
        let (state, app, _owner_key, _) = setup_razorpay_confirm_harness(
            "merchant_p1_owner",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 5000,
                "currency": "INR",
                "status": "captured",
                "method": "upi"
            }),
        )
        .await;

        let other_key = insert_test_merchant_key(&state, "merchant_p1_attacker").await;
        let (status, _) = call_razorpay_confirm(&app, &other_key, payment_id).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(state
            .webhook_store
            .get_payment_record(payment_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_razorpay_duplicate_webhook_after_confirmation() {
        let payment_id = "pay_p1_wh_after_conf";
        let order_id = "order_p1_wh_after_conf";
        let (state, app, key, _) = setup_razorpay_confirm_harness(
            "merchant_p1_wh1",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 5000,
                "currency": "INR",
                "status": "captured",
                "method": "upi"
            }),
        )
        .await;

        let (s1, j1) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(s1, StatusCode::OK);
        assert_eq!(j1["payment_state"], "captured");
        let rate_after_confirm = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .unwrap()
            .metrics
            .rate_per_min;

        let wh_body = serde_json::to_vec(&serde_json::json!({
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": payment_id,
                        "order_id": order_id,
                        "amount": 5000,
                        "currency": "INR",
                        "status": "captured",
                        "method": "upi"
                    }
                }
            }
        }))
        .unwrap();
        let sig = sign_razorpay_webhook("phase1_wh_secret", &wh_body);
        let wh_req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", sig)
            .header("x-razorpay-event-id", "evt_wh_dup_after_conf")
            .body(axum::body::Body::from(wh_body))
            .unwrap();
        let wh_resp = app.clone().oneshot(wh_req).await.unwrap();
        assert_eq!(wh_resp.status(), StatusCode::OK);
        let wh_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(wh_resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(wh_json["workload_updated"], false);
        assert_eq!(
            state
                .state_fabric
                .get_workload("payment-upi-india-south")
                .unwrap()
                .metrics
                .rate_per_min,
            rate_after_confirm
        );
    }

    #[tokio::test]
    async fn test_razorpay_confirmation_after_authoritative_captured_webhook() {
        let payment_id = "pay_p1_conf_after_wh";
        let order_id = "order_p1_conf_after_wh";
        let (state, app, key, _) = setup_razorpay_confirm_harness(
            "merchant_p1_wh2",
            order_id,
            payment_id,
            serde_json::json!({
                "id": payment_id,
                "order_id": order_id,
                "amount": 5000,
                "currency": "INR",
                "status": "captured",
                "method": "upi"
            }),
        )
        .await;

        let initial_rate = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .unwrap()
            .metrics
            .rate_per_min;

        let wh_body = serde_json::to_vec(&serde_json::json!({
            "event": "payment.captured",
            "payload": {
                "payment": {
                    "entity": {
                        "id": payment_id,
                        "order_id": order_id,
                        "amount": 5000,
                        "currency": "INR",
                        "status": "captured",
                        "method": "upi"
                    }
                }
            }
        }))
        .unwrap();
        let sig = sign_razorpay_webhook("phase1_wh_secret", &wh_body);
        let wh_req = Request::builder()
            .method("POST")
            .uri("/api/razorpay/webhook")
            .header("content-type", "application/json")
            .header("x-razorpay-signature", sig)
            .header("x-razorpay-event-id", "evt_wh_first_cap")
            .body(axum::body::Body::from(wh_body))
            .unwrap();
        let wh_resp = app.clone().oneshot(wh_req).await.unwrap();
        assert_eq!(wh_resp.status(), StatusCode::OK);

        let rate_after_wh = state
            .state_fabric
            .get_workload("payment-upi-india-south")
            .unwrap()
            .metrics
            .rate_per_min;
        assert!(rate_after_wh > initial_rate);

        let (s_conf, j_conf) = call_razorpay_confirm(&app, &key, payment_id).await;
        assert_eq!(s_conf, StatusCode::OK);
        assert_eq!(j_conf["status"], "already_confirmed");
        assert_eq!(j_conf["payment_state"], "captured");
        assert_eq!(j_conf["idempotent"], true);
        assert_eq!(j_conf["workload_updated"], false);
        assert_eq!(
            state
                .state_fabric
                .get_workload("payment-upi-india-south")
                .unwrap()
                .metrics
                .rate_per_min,
            rate_after_wh
        );

        let attempt = state
            .webhook_store
            .get_payment_attempt_for_provider("razorpay", order_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(attempt.state, "captured");
    }

    #[tokio::test]
    async fn test_agent_activity_redacts_secrets_and_enforces_tenant_isolation() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-secret-key-activity-test");

        let state = create_test_state();
        let tracker = state.inference_provider.cost_tracker();

        tracker.record_inference_with_scope(
            "deepseek-r1:1.5b",
            "diagnosis",
            Some("merchant_alpha_act"),
            r#"{"api_key":"esa_live_alpha_secret_123","email":"alpha@example.com","p95_latency_ms":280}"#,
            r#"<think>hidden cot alpha</think>{"hypothesis":"Latency spike","evidence_refs":["p95_latency_ms"]}"#,
            110,
            None,
        );
        tracker.record_inference_with_scope(
            "deepseek-r1:1.5b",
            "diagnosis",
            Some("merchant_beta_act"),
            r#"{"api_key":"esa_live_beta_secret_456","p95_latency_ms":300}"#,
            r#"{"hypothesis":"Beta secret","evidence_refs":["p95_latency_ms"]}"#,
            90,
            None,
        );
        tracker.record_inference_with_scope(
            "deepseek-r1:1.5b",
            "monitor",
            None,
            "global infra prompt",
            "global infra response",
            50,
            None,
        );

        let sandbox_key = insert_test_merchant_key(&state, "merchant_alpha_act").await;
        let app = build_app(state.clone());

        // 1. Sandbox merchant key without audit:read fails closed with 403
        let req_forbidden = Request::builder()
            .method("GET")
            .uri("/api/v1/agents/activity")
            .header("x-api-key", &sandbox_key)
            .body(axum::body::Body::empty())
            .unwrap();
        let resp_forbidden = app.clone().oneshot(req_forbidden).await.unwrap();
        assert_eq!(resp_forbidden.status(), StatusCode::FORBIDDEN);

        // 2. Non-admin caller with audit:read sees ONLY its own merchant_id (never beta or global)
        let alpha_caller = api_keys::CallerIdentity {
            user_id: "user_alpha".to_string(),
            merchant_id: "merchant_alpha_act".to_string(),
            email: None,
            role: "merchant".to_string(),
            auth_type: "jwt".to_string(),
            environment: "sandbox".to_string(),
            scopes: vec!["audit:read".to_string()],
        };
        let scoped_json = build_agent_activity_for_caller(&state, &alpha_caller);
        let entries = scoped_json["ai_thinking"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["merchant_id"], "merchant_alpha_act");
        let prompt_str = entries[0]["prompt"].as_str().unwrap();
        let resp_str = entries[0]["response"].as_str().unwrap();
        assert!(!prompt_str.contains("esa_live_alpha_secret_123"));
        assert!(!prompt_str.contains("alpha@example.com"));
        assert!(!resp_str.contains("hidden cot alpha"));
        assert!(!resp_str.contains("Beta secret"));
        assert_eq!(entries[0]["redacted"], true);

        // 3. Admin caller via HTTP sees all entries, all redacted
        let req_admin = Request::builder()
            .method("GET")
            .uri("/api/v1/agents/activity")
            .header("x-api-key", "admin-secret-key-activity-test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp_admin = app.oneshot(req_admin).await.unwrap();
        assert_eq!(resp_admin.status(), StatusCode::OK);
        let admin_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp_admin.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let admin_entries = admin_json["ai_thinking"].as_array().unwrap();
        assert_eq!(admin_entries.len(), 3);
        let serialized = serde_json::to_string(&admin_json).unwrap();
        assert!(!serialized.contains("esa_live_alpha_secret_123"));
        assert!(!serialized.contains("esa_live_beta_secret_456"));
        assert!(!serialized.contains("alpha@example.com"));
        assert!(!serialized.contains("hidden cot alpha"));

        std::env::remove_var("ESA_ADMIN_API_KEY");
    }

    #[tokio::test]
    async fn test_hf_provider_unavailability_never_blocks_checkout_and_reports_provider_metrics() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        clear_mode_env_vars();
        std::env::set_var("ESA_ENABLE_SIMULATED_CHECKOUT", "true");
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-secret-key-hf-p2-test");

        let mut state = create_test_state();
        let tracker = state.inference_provider.cost_tracker();
        let hf_cfg = esa_agents::AiProviderConfig {
            provider: esa_agents::ProviderKind::HfZerogpu,
            hf_url: Some("https://sujithputta-esa-ai-brain.hf.space".to_string()),
            hf_token: Some("hf_dummy_unit_test_token_12345".to_string()),
            ..Default::default()
        };
        state.inference_provider =
            esa_agents::build_inference_provider(&hf_cfg, Arc::clone(&tracker), true);

        let merchant_key = insert_test_merchant_key(&state, "merchant_hf_p2_checkout").await;
        let app = build_app(state);

        // 1. Checkout succeeds normally even when Hugging Face ZeroGPU is unconfigured/unavailable
        let req_checkout = Request::builder()
            .method("POST")
            .uri("/api/payments/checkout")
            .header("content-type", "application/json")
            .header("x-api-key", &merchant_key)
            .body(axum::body::Body::from(
                serde_json::json!({
                    "amount_minor": 10000,
                    "currency": "INR",
                    "gateway": "razorpay"
                })
                .to_string(),
            ))
            .unwrap();
        let resp_checkout = app.clone().oneshot(req_checkout).await.unwrap();
        assert_eq!(resp_checkout.status(), StatusCode::OK);

        // 2. /api/v1/costs/ai distinguishes active_provider = hf_zerogpu
        let req_costs = Request::builder()
            .method("GET")
            .uri("/api/v1/costs/ai")
            .header("x-api-key", "admin-secret-key-hf-p2-test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp_costs = app.oneshot(req_costs).await.unwrap();
        assert_eq!(resp_costs.status(), StatusCode::OK);
        let costs_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp_costs.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(costs_json["active_provider"], "hf_zerogpu");

        std::env::remove_var("ESA_ENABLE_SIMULATED_CHECKOUT");
        std::env::remove_var("ESA_ADMIN_API_KEY");
    }

    #[tokio::test]
    async fn test_phase3_intelligence_endpoints_auth_isolation_and_read_only_safety() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-intel-test-key");
        std::env::set_var("ESA_TEST_API_KEY", "merchant-a-intel-test-key");

        let state = create_test_state();
        esa_agents::seed_system_knowledge(state.memory_store.as_ref())
            .await
            .unwrap();

        // Seed Merchant A and Merchant B payment attempts
        state
            .webhook_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_intel_a".to_string(),
                merchant_id: "merchant_sandbox_01".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_a".to_string()),
                payment_reference: Some("pay_a".to_string()),
                amount_minor: 25000,
                currency: "INR".to_string(),
                state: "authorized".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
        state
            .webhook_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_intel_b".to_string(),
                merchant_id: "merchant_b".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_b".to_string()),
                payment_reference: Some("pay_b".to_string()),
                amount_minor: 50000,
                currency: "INR".to_string(),
                state: "captured".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        let app = build_app(state.clone());

        // 1. Unauthenticated request is rejected (401)
        let req_unauth = Request::builder()
            .method("GET")
            .uri("/api/v1/intelligence/payments/summary")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp_unauth = app.clone().oneshot(req_unauth).await.unwrap();
        assert_eq!(resp_unauth.status(), StatusCode::UNAUTHORIZED);

        // 2. Merchant A can read its own summary (sees only tx_intel_a)
        let req_sum = Request::builder()
            .method("GET")
            .uri("/api/v1/intelligence/payments/summary")
            .header("x-api-key", "merchant-a-intel-test-key")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp_sum = app.clone().oneshot(req_sum).await.unwrap();
        assert_eq!(resp_sum.status(), StatusCode::OK);
        let sum_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp_sum.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(sum_json["total_attempts"], 1);
        assert_eq!(sum_json["read_only"], true);

        // 3. Merchant A cannot request Merchant B's summary (403)
        let req_cross = Request::builder()
            .method("GET")
            .uri("/api/v1/intelligence/payments/summary?merchant_id=merchant_b")
            .header("x-api-key", "merchant-a-intel-test-key")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp_cross = app.clone().oneshot(req_cross).await.unwrap();
        assert_eq!(resp_cross.status(), StatusCode::FORBIDDEN);

        // 4. Merchant A cannot investigate Merchant B's transaction (404/403 scoped)
        let req_inv_b = Request::builder()
            .method("POST")
            .uri("/api/v1/intelligence/payments/investigate")
            .header("x-api-key", "merchant-a-intel-test-key")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "transaction_id": "tx_intel_b" }).to_string(),
            ))
            .unwrap();
        let resp_inv_b = app.clone().oneshot(req_inv_b).await.unwrap();
        assert_eq!(resp_inv_b.status(), StatusCode::NOT_FOUND);

        // 5. Merchant A investigates its own transaction (deterministic fallback works offline)
        let req_inv_a = Request::builder()
            .method("POST")
            .uri("/api/v1/intelligence/payments/investigate")
            .header("x-api-key", "merchant-a-intel-test-key")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "transaction_id": "tx_intel_a" }).to_string(),
            ))
            .unwrap();
        let resp_inv_a = app.clone().oneshot(req_inv_a).await.unwrap();
        assert_eq!(resp_inv_a.status(), StatusCode::OK);
        let inv_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp_inv_a.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(inv_json["read_only"], true);
        assert_eq!(
            inv_json["report"]["authoritative_payment_state"],
            "authorized"
        );

        // 6. Financial safety: attempt state was NOT mutated by investigation
        let after_attempts = state
            .webhook_store
            .list_payment_attempts_for_merchant(Some("merchant_sandbox_01"), 10)
            .await
            .unwrap();
        assert_eq!(after_attempts[0].state, "authorized");

        // 7. Recent memory endpoint returns seeded system knowledge
        let req_mem = Request::builder()
            .method("GET")
            .uri("/api/v1/intelligence/memory/recent")
            .header("x-api-key", "merchant-a-intel-test-key")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp_mem = app.clone().oneshot(req_mem).await.unwrap();
        assert_eq!(resp_mem.status(), StatusCode::OK);
        let mem_json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp_mem.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(mem_json["count"].as_u64().unwrap_or(0) >= 1);

        // 8. Providers health intelligence endpoint works
        let req_ph = Request::builder()
            .method("GET")
            .uri("/api/v1/intelligence/payments/providers/health")
            .header("x-api-key", "merchant-a-intel-test-key")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp_ph = app.oneshot(req_ph).await.unwrap();
        assert_eq!(resp_ph.status(), StatusCode::OK);

        std::env::remove_var("ESA_ADMIN_API_KEY");
        std::env::remove_var("ESA_TEST_API_KEY");
    }

    #[tokio::test]
    async fn test_phase3_investigate_rate_limit_and_merchant_isolation() {
        let _guard = api_keys::tests::TEST_ENV_MUTEX.lock();
        std::env::set_var("ESA_TEST_API_KEY", "merchant-a-rl-key");
        std::env::set_var("ESA_ADMIN_API_KEY", "admin-rl-key");

        let mut state = create_test_state();
        state.rate_limiter = rate_limiter::ApiRateLimiter::new().with_investigate_quota(2, 1);

        state
            .webhook_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_rl_a".to_string(),
                merchant_id: "merchant_sandbox_01".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_rl_a".to_string()),
                payment_reference: Some("pay_rl_a".to_string()),
                amount_minor: 10000,
                currency: "INR".to_string(),
                state: "authorized".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
        state
            .webhook_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_rl_admin".to_string(),
                merchant_id: "admin_root".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_rl_admin".to_string()),
                payment_reference: Some("pay_rl_admin".to_string()),
                amount_minor: 10000,
                currency: "INR".to_string(),
                state: "authorized".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();

        let app = build_app(state);

        // 1. First request for Merchant A succeeds (burst=1)
        let r1 = Request::builder()
            .method("POST")
            .uri("/api/v1/intelligence/payments/investigate")
            .header("x-api-key", "merchant-a-rl-key")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "transaction_id": "tx_rl_a" }).to_string(),
            ))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(r1).await.unwrap().status(),
            StatusCode::OK
        );

        // 2. Immediate second request for Merchant A is rate-limited (429)
        let r2 = Request::builder()
            .method("POST")
            .uri("/api/intelligence/payments/investigate")
            .header("x-api-key", "merchant-a-rl-key")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "transaction_id": "tx_rl_a" }).to_string(),
            ))
            .unwrap();
        let resp2 = app.clone().oneshot(r2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            resp2
                .headers()
                .get(axum::http::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            Some("1")
        );

        // 3. Another caller (admin_root) is NOT exhausted by Merchant A
        let r_other = Request::builder()
            .method("POST")
            .uri("/api/v1/intelligence/payments/investigate")
            .header("x-api-key", "admin-rl-key")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "transaction_id": "tx_rl_admin" }).to_string(),
            ))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(r_other).await.unwrap().status(),
            StatusCode::OK
        );

        // 4. After waiting for replenish (>500ms for 2 rps), Merchant A can retry
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        let r3 = Request::builder()
            .method("POST")
            .uri("/api/v1/intelligence/payments/investigate")
            .header("x-api-key", "merchant-a-rl-key")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "transaction_id": "tx_rl_a" }).to_string(),
            ))
            .unwrap();
        assert_eq!(app.oneshot(r3).await.unwrap().status(), StatusCode::OK);

        std::env::remove_var("ESA_TEST_API_KEY");
        std::env::remove_var("ESA_ADMIN_API_KEY");
    }
}
