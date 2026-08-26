use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

/// WebSocket handler for real-time telemetry streaming

#[derive(Clone)]
pub struct TelemetryBroadcaster {
    tx: broadcast::Sender<TelemetryMessage>,
}

impl TelemetryBroadcaster {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self { tx }
    }

    #[allow(dead_code)]
    pub fn send(&self, message: TelemetryMessage) {
        let _ = self.tx.send(message);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TelemetryMessage> {
        self.tx.subscribe()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum TelemetryMessage {
    #[serde(rename = "workload_update")]
    WorkloadUpdate {
        workload_id: String,
        state: String,
        metrics: serde_json::Value,
    },
    #[serde(rename = "agent_activity")]
    AgentActivity {
        agent_id: String,
        activity: String,
        timestamp: String,
    },
    #[serde(rename = "condition_detected")]
    ConditionDetected {
        condition_type: String,
        workload_id: String,
        severity: String,
        description: String,
    },
    #[serde(rename = "action_proposed")]
    ActionProposed {
        proposal_id: String,
        action_type: String,
        workload_id: String,
    },
    #[serde(rename = "action_executed")]
    ActionExecuted {
        execution_id: String,
        action_type: String,
        outcome: String,
    },
    #[serde(rename = "policy_decision")]
    PolicyDecision {
        proposal_id: String,
        verdict: String,
        risk_score: f64,
    },
    #[serde(rename = "vitals_update")]
    VitalsUpdate {
        timestamp: String,
        total_tps: f64,
        avg_p95_ms: f64,
        avg_error_rate: f64,
        total_queue: u64,
        healthy_count: u32,
        degraded_count: u32,
    },
}

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(broadcaster): State<Arc<TelemetryBroadcaster>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, broadcaster))
}

async fn handle_socket(socket: WebSocket, broadcaster: Arc<TelemetryBroadcaster>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = broadcaster.subscribe();

    info!("WebSocket client connected");

    // Send initial connection message
    let welcome = TelemetryMessage::AgentActivity {
        agent_id: "system".to_string(),
        activity: "WebSocket connected".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    
    if let Ok(json) = serde_json::to_string(&welcome) {
        if let Err(e) = sender.send(Message::Text(json)).await {
            info!("Failed to send welcome message: {:?}", e);
            return;
        }
    }

    // Spawn send task
    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(msg) => {
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if let Err(e) = sender.send(Message::Text(json)).await {
                                    info!("Send error: {:?}", e);
                                    break;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            info!("Broadcast lagged by {} messages", n);
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                    // Send ping to keep connection alive
                    if let Err(e) = sender.send(Message::Ping(vec![])).await {
                        info!("Ping error: {:?}", e);
                        break;
                    }
                }
            }
        }
        info!("Send task ended");
    });

    // Spawn receive task
    let mut recv_task = tokio::spawn(async move {
        while let Some(result) = receiver.next().await {
            match result {
                Ok(Message::Close(frame)) => {
                    info!("Client sent close frame: {:?}", frame);
                    break;
                }
                Ok(Message::Ping(_)) => {
                    // Pong is sent automatically by axum
                }
                Ok(Message::Pong(_)) => {
                    // Received pong response
                }
                Ok(Message::Text(text)) => {
                    info!("Received text message: {}", text);
                }
                Ok(_) => {
                    // Ignore other messages
                }
                Err(e) => {
                    info!("Receive error: {:?}", e);
                    break;
                }
            }
        }
        info!("Receive task ended");
    });

    // Wait for either task to finish
    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
        },
        _ = &mut recv_task => {
            send_task.abort();
        },
    };

    info!("WebSocket client disconnected");
}
