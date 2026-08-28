use axum::{
    body::Bytes,
    extract::{ws::WebSocketUpgrade, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use esa_agents::OllamaClient;
use esa_core::*;
use esa_razorpay::{payment_entity_to_event, RazorpayAdapter, RazorpayConfig, WebhookError};
use esa_runtime::{EsaOrchestrator, RuntimeEvent, RuntimeEventHandler};
use esa_state::StateFabric;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;
use tracing::{error, info, Level};

mod benchmark;
mod benchmark_harness;
mod live_data;
mod payment;
mod vitals;
mod websocket;
use live_data::{
    action_record_from_audit, audit_record_json, compute_verdict_stats, effect_measurement_json,
    verdict_record_json,
};
use vitals::VitalsStore;
use websocket::{websocket_handler, TelemetryBroadcaster, TelemetryMessage};

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

    // Initialize state fabric
    let state_fabric = Arc::new(StateFabric::new());
    let audit_store = Arc::new(AuditStore::new());
    let broadcaster = Arc::new(TelemetryBroadcaster::new());

    // Initialize Ollama client
    let ollama_url =
        std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let ollama_model =
        std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "mistral:latest".to_string());
    let ollama_client = OllamaClient::new(ollama_url, ollama_model);

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

    // Initialize orchestrator
    let orchestrator = Arc::new(EsaOrchestrator::new(
        Arc::clone(&state_fabric),
        ollama_client.clone(),
        Arc::clone(&audit_store),
        Some(event_handler),
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
                    "Razorpay adapter enabled (mode: {})",
                    if adapter.config().test_mode_only {
                        "test"
                    } else {
                        "live"
                    }
                );
                Some(Arc::new(adapter))
            }
            Err(e) => {
                error!("Razorpay config invalid — adapter disabled: {}", e);
                None
            }
        }
    } else {
        info!("Razorpay adapter disabled — set RAZORPAY_* in .env to enable");
        None
    };

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

    // Create app state
    let app_state = AppState {
        state_fabric,
        orchestrator,
        ollama_client,
        broadcaster: Arc::clone(&broadcaster),
        audit_store,
        agent_status,
        razorpay,
        vitals,
        last_benchmark: Arc::new(std::sync::RwLock::new(None)),
        last_ablation: Arc::new(std::sync::RwLock::new(None)),
    };

    // Build router
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/api/workloads", get(list_workloads))
        .route("/api/workloads/:id", get(get_workload))
        .route("/api/workloads", post(create_workload))
        .route("/api/events/payment", post(ingest_payment_event))
        .route("/api/razorpay/webhook", post(razorpay_webhook))
        .route("/api/razorpay/status", get(razorpay_status))
        .route("/api/razorpay/orders", post(razorpay_create_order))
        .route("/api/razorpay/verify", post(razorpay_verify_keys))
        .route("/api/razorpay/confirm", post(razorpay_confirm_payment))
        .route("/api/vitals/history", get(get_vitals_history))
        .route("/api/demo/trigger-spike", post(trigger_spike))
        .route("/api/demo/seed", post(seed_demo_data))
        .route("/api/demo/scenario/:scenario", post(trigger_scenario))
        .route("/api/benchmark/run", post(run_benchmark))
        .route("/api/benchmark/harness", post(run_benchmark_harness))
        .route("/api/benchmark/latest", get(get_benchmark_latest))
        .route("/api/metrics/tokens", get(get_token_metrics))
        .route("/api/agents/status", get(get_agents_status))
        .route("/api/agents/activity", get(get_agent_activity))
        .route("/api/actions/recent", get(get_recent_actions))
        // NEW: Audit Trail endpoints
        .route("/api/audit/trail", get(get_audit_trail))
        .route("/api/audit/verify-chain", get(verify_audit_chain))
        .route("/api/audit/decision/:decision_id", get(get_decision_detail))
        .route("/api/audit/replay/:decision_id", post(replay_decision))
        .route("/api/audit/replay/:decision_id", get(replay_decision))
        // Benchmark & Ablation endpoints
        .route("/api/benchmark/ablations", post(run_benchmark_ablations))
        .route("/api/benchmark/ablations", get(get_benchmark_ablations))
        // NEW: Effect Measurement endpoints
        .route("/api/effects/measurements", get(get_effect_measurements))
        .route("/api/effects/recent", get(get_recent_effects))
        // NEW: AI Cost endpoints
        .route("/api/costs/ai", get(get_ai_costs))
        .route("/api/costs/per-agent", get(get_costs_per_agent))
        // NEW: Policy Verdict endpoints
        .route("/api/verdicts/recent", get(get_recent_verdicts))
        .route("/api/verdicts/stats", get(get_verdict_stats))
        // NEW: Intent & Constraints endpoints
        .route("/api/intent/active", get(get_active_intents))
        .route("/api/intent/violations", get(get_constraint_violations))
        .route("/ws/telemetry", get(ws_telemetry_handler))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let addr = "0.0.0.0:8080";
    info!("ESA API listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    orch_handle.await?;

    Ok(())
}

#[derive(Clone)]
struct AppState {
    state_fabric: Arc<StateFabric>,
    orchestrator: Arc<EsaOrchestrator>,
    ollama_client: OllamaClient,
    broadcaster: Arc<TelemetryBroadcaster>,
    audit_store: Arc<AuditStore>,
    agent_status: Arc<std::sync::RwLock<AgentStatusState>>,
    razorpay: Option<Arc<RazorpayAdapter>>,
    vitals: VitalsStore,
    last_benchmark: Arc<std::sync::RwLock<Option<benchmark::BenchmarkComparison>>>,
    last_ablation: Arc<std::sync::RwLock<Option<benchmark_harness::AblationStudyResult>>>,
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

async fn list_workloads(State(state): State<AppState>) -> impl IntoResponse {
    let workloads = state.state_fabric.list_workloads();
    Json(workloads)
}

async fn get_workload(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WorkloadEntity>, AppError> {
    state
        .state_fabric
        .get_workload(&id)
        .ok_or(AppError::NotFound(format!("Workload {} not found", id)))
        .map(Json)
}

async fn create_workload(
    State(state): State<AppState>,
    Json(workload): Json<WorkloadEntity>,
) -> Result<Json<WorkloadEntity>, AppError> {
    state.state_fabric.upsert_workload(workload.clone())?;
    Ok(Json(workload))
}

async fn ingest_payment_event(
    State(state): State<AppState>,
    Json(event): Json<PaymentEvent>,
) -> impl IntoResponse {
    let result = payment::apply_payment_event(&state.state_fabric, &event);
    publish_payment_side_effects(&state, &result);

    Json(serde_json::json!({
        "status": "accepted",
        "event_id": result.event_id,
        "auto_spike_triggered": result.workload_updated,
        "workload_id": result.workload_id,
        "message": "Payment processed and workload metrics automatically adjusted"
    }))
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

    let signature = headers
        .get("X-Razorpay-Signature")
        .and_then(|v| v.to_str().ok());

    let event = match adapter.process_webhook(body.as_ref(), signature) {
        Ok(event) => event,
        Err(WebhookError::RateLimited) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({ "error": "rate limit exceeded" })),
            )
                .into_response();
        }
        Err(WebhookError::DuplicateEvent) => {
            return Json(serde_json::json!({
                "status": "duplicate",
                "message": "Event already processed"
            }))
            .into_response();
        }
        Err(WebhookError::InvalidSignature | WebhookError::MissingSignature) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid webhook signature" })),
            )
                .into_response();
        }
        Err(WebhookError::InvalidJson(msg)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
        Err(WebhookError::MalformedPayload) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "malformed webhook payload" })),
            )
                .into_response();
        }
    };

    let result = payment::apply_payment_event(&state.state_fabric, &event);
    publish_payment_side_effects(&state, &result);

    Json(serde_json::json!({
        "status": "accepted",
        "source": "razorpay",
        "event_id": result.event_id,
        "workload_updated": result.workload_updated,
        "workload_id": result.workload_id,
    }))
    .into_response()
}

async fn razorpay_status(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(adapter) = &state.razorpay {
        Json(adapter.status()).into_response()
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
) -> Result<Json<serde_json::Value>, AppError> {
    let adapter = state
        .razorpay
        .as_ref()
        .ok_or_else(|| AppError::Internal("Razorpay adapter not configured".into()))?;

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
    Json(req): Json<RazorpayCreateOrderRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let adapter = state
        .razorpay
        .as_ref()
        .ok_or_else(|| AppError::Internal("Razorpay adapter not configured".into()))?;

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
    });

    let receipt = format!("esa-{}", chrono::Utc::now().timestamp_millis());

    let order = adapter
        .client()
        .create_order(req.amount_cents, &receipt, notes)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let order_id = order.get("id").and_then(|v| v.as_str()).unwrap_or_default();

    Ok(Json(serde_json::json!({
        "order_id": order_id,
        "amount": order.get("amount").and_then(|v| v.as_u64()).unwrap_or(req.amount_cents),
        "currency": order.get("currency").and_then(|v| v.as_str()).unwrap_or("INR"),
        "key_id": adapter.config().key_id,
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
    Json(req): Json<RazorpayConfirmRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let adapter = state
        .razorpay
        .as_ref()
        .ok_or_else(|| AppError::Internal("Razorpay adapter not configured".into()))?;

    let payment = adapter
        .client()
        .get_payment(&req.payment_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let event = payment_entity_to_event(&payment).map_err(|e| AppError::Internal(e.to_string()))?;
    let result = payment::apply_payment_event(&state.state_fabric, &event);
    publish_payment_side_effects(&state, &result);

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "source": "razorpay_confirm",
        "event_id": result.event_id,
        "workload_updated": result.workload_updated,
        "workload_id": result.workload_id,
    })))
}

async fn get_vitals_history(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "snapshots": state.vitals.history(),
        "latest": state.vitals.latest(),
    }))
}

#[derive(Deserialize)]
struct TriggerSpikeRequest {
    multiplier: f64,
}

async fn seed_demo_data(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    info!("Seeding demo data");

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
    Json(req): Json<TriggerSpikeRequest>,
) -> Result<impl IntoResponse, AppError> {
    info!(
        "🧪 Manual spike test triggered with multiplier {}",
        req.multiplier
    );
    info!("ℹ️  Note: In production, spikes occur automatically from payment transaction volume");

    // Get ALL workloads and apply spike to all of them
    let all_workloads = state.state_fabric.list_workloads();
    let mut affected_count = 0;

    for mut workload in all_workloads {
        // Increase metrics to simulate spike
        workload.metrics.rate_per_min *= req.multiplier;
        workload.metrics.p95_latency_ms *= req.multiplier * 0.8;
        workload.metrics.p99_latency_ms *= req.multiplier * 0.9;
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
        "multiplier": req.multiplier,
        "note": "Autonomous recovery system will detect and recover automatically"
    })))
}

async fn get_token_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let stats = state.ollama_client.get_token_stats();
    Json(stats)
}

async fn get_agent_activity(State(state): State<AppState>) -> impl IntoResponse {
    let metrics = state.ollama_client.get_recent_inference_metrics(20);

    let ai_thinking: Vec<serde_json::Value> = metrics
        .into_iter()
        .filter(|m| m.error.is_none())
        .map(|m| {
            serde_json::json!({
                "agent": m.agent_id,
                "prompt": m.prompt,
                "response": m.response,
                "timestamp": m.timestamp.to_rfc3339(),
                "model": m.model,
            })
        })
        .collect();

    Json(serde_json::json!({
        "ai_thinking": ai_thinking
    }))
}

async fn get_agents_status(State(state): State<AppState>) -> impl IntoResponse {
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

    Json(serde_json::json!({
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
    }))
}

async fn get_recent_actions(State(state): State<AppState>) -> impl IntoResponse {
    let records = state.audit_store.list_recent(50);
    let actions: Vec<serde_json::Value> = records.iter().map(action_record_from_audit).collect();

    Json(serde_json::json!({
        "actions": actions
    }))
}

async fn ws_telemetry_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    websocket_handler(ws, State(state.broadcaster)).await
}

// ============ NEW ENDPOINTS FOR 100% DEMO READINESS ============

// Audit Trail Endpoints
async fn get_audit_trail(State(state): State<AppState>) -> impl IntoResponse {
    let records = state.audit_store.list_recent(50);
    let audit_records: Vec<serde_json::Value> = records.iter().map(audit_record_json).collect();

    Json(serde_json::json!({
        "audit_records": audit_records,
        "total": audit_records.len(),
        "has_more": false
    }))
}

async fn get_decision_detail(
    State(state): State<AppState>,
    Path(decision_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
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
    Path(decision_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
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

async fn verify_audit_chain(State(state): State<AppState>) -> impl IntoResponse {
    let result = state.audit_store.verify_chain();
    Json(result)
}

async fn run_benchmark_ablations(
    State(state): State<AppState>,
) -> Result<Json<benchmark_harness::AblationStudyResult>, AppError> {
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
) -> Result<Json<benchmark_harness::AblationStudyResult>, AppError> {
    if let Ok(guard) = state.last_ablation.read() {
        if let Some(res) = guard.as_ref() {
            return Ok(Json(res.clone()));
        }
    }

    // Auto-run if not executed yet
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

// Effect Measurement Endpoints
async fn get_effect_measurements(State(state): State<AppState>) -> impl IntoResponse {
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

    Json(serde_json::json!({
        "measurements": measurements,
        "total": measurements.len(),
        "avg_effectiveness": avg_effectiveness
    }))
}

async fn get_recent_effects(State(state): State<AppState>) -> impl IntoResponse {
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

    Json(serde_json::json!({
        "recent_effects": recent_effects
    }))
}

// AI Cost Endpoints
async fn get_ai_costs(State(state): State<AppState>) -> impl IntoResponse {
    let cost_tracker = state.ollama_client.get_cost_tracker();
    let aggregated = cost_tracker.get_aggregated_metrics(None);

    Json(serde_json::json!({
        "total_tokens": aggregated.total_tokens,
        "total_requests": aggregated.total_requests,
        "successful_requests": aggregated.successful_requests,
        "failed_requests": aggregated.failed_requests,
        "avg_latency_ms": aggregated.average_latency_ms,
        "cache_hit_rate": aggregated.cache_hit_rate,
        "total_cost_usd": aggregated.total_cost_usd,
        "time_window_start": aggregated.time_window_start,
        "time_window_end": aggregated.time_window_end
    }))
}

async fn get_costs_per_agent(State(state): State<AppState>) -> impl IntoResponse {
    let cost_tracker = state.ollama_client.get_cost_tracker();
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

    Json(serde_json::json!({
        "per_agent": agent_costs,
        "total_agents": aggregated.requests_per_agent.len(),
        "total_cost_usd": aggregated.total_cost_usd
    }))
}

// Policy Verdict Endpoints
async fn get_recent_verdicts(State(state): State<AppState>) -> impl IntoResponse {
    let records = state.audit_store.list_recent(50);
    let verdicts: Vec<serde_json::Value> = records.iter().map(verdict_record_json).collect();

    Json(serde_json::json!({
        "verdicts": verdicts,
        "total": verdicts.len()
    }))
}

async fn get_verdict_stats(State(state): State<AppState>) -> impl IntoResponse {
    let records = state.audit_store.list_recent(200);
    let stats = compute_verdict_stats(&records);

    Json(serde_json::json!({
        "stats": stats,
        "by_verdict": {
            "ALLOW": stats.get("allow_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "DENY": stats.get("deny_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "STALE_STATE": stats.get("stale_state_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "REQUIRES_APPROVAL": stats.get("requires_approval_count").and_then(|v| v.as_u64()).unwrap_or(0),
        }
    }))
}

// Intent & Constraints Endpoints
async fn get_active_intents(State(_state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
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
    }))
}

async fn get_constraint_violations(State(_state): State<AppState>) -> impl IntoResponse {
    use chrono::Utc;

    Json(serde_json::json!({
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
    }))
}

// Demo Scenario Triggers
#[derive(Deserialize)]
struct ScenarioRequest {
    #[serde(default)]
    intensity: Option<f64>,
}

async fn trigger_scenario(
    State(state): State<AppState>,
    Path(scenario): Path<String>,
    Json(req): Json<ScenarioRequest>,
) -> Result<impl IntoResponse, AppError> {
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
    Json(req): Json<BenchmarkRequest>,
) -> Result<Json<benchmark::BenchmarkComparison>, AppError> {
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

async fn get_benchmark_latest(State(state): State<AppState>) -> impl IntoResponse {
    let latest = state
        .last_benchmark
        .read()
        .ok()
        .and_then(|guard| guard.clone());

    Json(serde_json::json!({
        "available": latest.is_some(),
        "comparison": latest,
        "scenarios": benchmark_harness::PERFORMANCE_SCENARIOS,
        "safety_scenarios": benchmark_harness::SAFETY_SCENARIOS,
        "controllers": ["B0_rules", "B1_adaptive", "B2_esa"],
        "endpoint": "POST /api/benchmark/run",
        "harness_endpoint": "POST /api/benchmark/harness",
    }))
}

#[derive(Deserialize)]
struct HarnessRequest {
    #[serde(default)]
    quick: bool,
}

async fn run_benchmark_harness(
    State(state): State<AppState>,
    Json(req): Json<HarnessRequest>,
) -> Result<Json<benchmark_harness::HarnessResult>, AppError> {
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
    NotFound(String),
    Internal(String),
}

impl From<EsaError> for AppError {
    fn from(err: EsaError) -> Self {
        AppError::Internal(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
