use async_trait::async_trait;
use chrono::{DateTime, Utc};
use esa_core::payment_state::{PaymentEntity, PaymentError, PaymentState, TransitionOutcome};
use esa_core::PaymentEventType;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebhookStatus {
    Pending,
    Processed,
    Failed,
}

impl WebhookStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processed => "processed",
            Self::Failed => "failed",
        }
    }
}

impl std::str::FromStr for WebhookStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "processed" => Self::Processed,
            "failed" => Self::Failed,
            _ => Self::Pending,
        })
    }
}

fn parse_payment_state(s: &str) -> Result<PaymentState, WebhookStoreError> {
    serde_json::from_str(&format!("\"{}\"", s))
        .or_else(|_| serde_json::from_str(&format!("\"{}\"", s.to_uppercase())))
        .map_err(|e| WebhookStoreError::Serialization(e.to_string()))
}

/// Parameters for processing an incoming provider webhook event
#[derive(Debug, Clone)]
pub struct WebhookEventParams<'a> {
    pub provider: &'a str,
    pub provider_event_id: &'a str,
    pub event_type: PaymentEventType,
    pub payment_id: &'a str,
    pub amount_minor: u64,
    pub currency: &'a str,
    pub refund_amount_minor: Option<u64>,
    pub payload: &'a serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookInboxRecord {
    pub id: i64,
    pub provider: String,
    pub provider_event_id: String,
    pub event_type: String,
    pub payment_id: Option<String>,
    pub payload: serde_json::Value,
    pub status: WebhookStatus,
    pub error_message: Option<String>,
    pub received_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebhookProcessingResult {
    /// Newly applied payment transition committed to durable storage
    Applied {
        inbox_id: i64,
        provider: String,
        provider_event_id: String,
        payment_id: String,
        transition: TransitionOutcome,
        payment: PaymentEntity,
    },
    /// Event already processed in a previous delivery (idempotent no-op)
    AlreadyProcessed {
        inbox_id: i64,
        provider: String,
        provider_event_id: String,
        payment_id: String,
        processed_at: Option<DateTime<Utc>>,
    },
    /// Event currently in-flight by a concurrent delivery worker
    InFlight {
        inbox_id: i64,
        provider: String,
        provider_event_id: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum WebhookStoreError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Payment state error: {0}")]
    PaymentState(#[from] PaymentError),
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[async_trait]
pub trait WebhookStore: Send + Sync {
    /// Process an incoming webhook event transactionally.
    /// Guarantees:
    /// - Atomically claims or records the webhook event in `webhook_inbox`
    /// - Deduplicates via UNIQUE(provider, provider_event_id)
    /// - Safely coordinates concurrent deliveries (returning InFlight if pending)
    /// - Transitions payment entity via PaymentStateMachine with row-level locks
    /// - Updates payment_records and marks webhook_inbox as processed
    /// - Rolls back transaction on database or state machine errors
    async fn process_webhook_event(
        &self,
        params: &WebhookEventParams<'_>,
    ) -> Result<WebhookProcessingResult, WebhookStoreError>;

    /// Retrieve an inbox record by provider and event ID
    async fn get_inbox_record(
        &self,
        provider: &str,
        provider_event_id: &str,
    ) -> Result<Option<WebhookInboxRecord>, WebhookStoreError>;

    /// Retrieve a payment record by payment ID
    async fn get_payment_record(
        &self,
        payment_id: &str,
    ) -> Result<Option<PaymentEntity>, WebhookStoreError>;
}

/// Durable PostgreSQL-backed implementation of WebhookStore
pub struct PostgresWebhookStore {
    pool: PgPool,
}

impl PostgresWebhookStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn init_schema(pool: &PgPool) -> Result<(), WebhookStoreError> {
        let statements = [
            r#"
            CREATE TABLE IF NOT EXISTS webhook_inbox (
                id BIGSERIAL PRIMARY KEY,
                provider VARCHAR(64) NOT NULL,
                provider_event_id VARCHAR(255) NOT NULL,
                event_type VARCHAR(100) NOT NULL,
                payment_id VARCHAR(255),
                payload JSONB NOT NULL,
                status VARCHAR(50) NOT NULL,
                error_message TEXT,
                received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                processed_at TIMESTAMPTZ,
                CONSTRAINT uq_webhook_inbox_provider_event UNIQUE (provider, provider_event_id)
            );
            "#,
            r#"
            CREATE TABLE IF NOT EXISTS payment_records (
                payment_id VARCHAR(255) PRIMARY KEY,
                provider VARCHAR(64) NOT NULL,
                state VARCHAR(50) NOT NULL,
                amount_minor BIGINT NOT NULL,
                currency VARCHAR(10) NOT NULL,
                refunded_amount_minor BIGINT NOT NULL DEFAULT 0,
                events_seen JSONB NOT NULL DEFAULT '[]'::jsonb,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            "#,
            "CREATE INDEX IF NOT EXISTS idx_webhook_inbox_provider_event ON webhook_inbox(provider, provider_event_id);",
            "CREATE INDEX IF NOT EXISTS idx_webhook_inbox_payment_id ON webhook_inbox(payment_id);",
            "CREATE INDEX IF NOT EXISTS idx_payment_records_provider ON payment_records(provider);",
        ];

        for stmt in statements {
            sqlx::query(stmt)
                .execute(pool)
                .await
                .map_err(|e| WebhookStoreError::Database(e.to_string()))?;
        }

        Ok(())
    }
}

#[async_trait]
impl WebhookStore for PostgresWebhookStore {
    async fn process_webhook_event(
        &self,
        params: &WebhookEventParams<'_>,
    ) -> Result<WebhookProcessingResult, WebhookStoreError> {
        let provider = params.provider;
        let provider_event_id = params.provider_event_id;
        let event_type = params.event_type.clone();
        let payment_id = params.payment_id;
        let amount_minor = params.amount_minor;
        let currency = params.currency;
        let refund_amount_minor = params.refund_amount_minor;
        let payload = params.payload;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        // 1. Atomic insertion with unique constraint over (provider, provider_event_id)
        let insert_res = sqlx::query(
            r#"
            INSERT INTO webhook_inbox (provider, provider_event_id, event_type, payment_id, payload, status)
            VALUES ($1, $2, $3, $4, $5, 'pending')
            ON CONFLICT (provider, provider_event_id) DO NOTHING
            RETURNING id;
            "#,
        )
        .bind(provider)
        .bind(provider_event_id)
        .bind(format!("{:?}", event_type))
        .bind(payment_id)
        .bind(payload)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        let inbox_id = match insert_res {
            Some(row) => row.get::<i64, _>("id"),
            None => {
                // Record already exists. Query with row lock to coordinate concurrent delivery
                let existing = sqlx::query(
                    r#"
                    SELECT id, status, processed_at, payment_id
                    FROM webhook_inbox
                    WHERE provider = $1 AND provider_event_id = $2
                    FOR UPDATE;
                    "#,
                )
                .bind(provider)
                .bind(provider_event_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

                match existing {
                    Some(row) => {
                        let id: i64 = row.get("id");
                        let status_str: String = row.get("status");
                        let processed_at: Option<DateTime<Utc>> = row.get("processed_at");
                        let existing_payment_id: Option<String> = row.get("payment_id");
                        let pid = existing_payment_id.unwrap_or_else(|| payment_id.to_string());

                        if status_str == "processed" {
                            let _ = tx.rollback().await;
                            return Ok(WebhookProcessingResult::AlreadyProcessed {
                                inbox_id: id,
                                provider: provider.to_string(),
                                provider_event_id: provider_event_id.to_string(),
                                payment_id: pid,
                                processed_at,
                            });
                        } else if status_str == "pending" {
                            let _ = tx.rollback().await;
                            return Ok(WebhookProcessingResult::InFlight {
                                inbox_id: id,
                                provider: provider.to_string(),
                                provider_event_id: provider_event_id.to_string(),
                            });
                        } else {
                            // Safe recovery: prior attempt failed; claim it for retry
                            sqlx::query(
                                r#"
                                UPDATE webhook_inbox
                                SET status = 'pending', error_message = NULL, received_at = NOW()
                                WHERE id = $1;
                                "#,
                            )
                            .bind(id)
                            .execute(&mut *tx)
                            .await
                            .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

                            id
                        }
                    }
                    None => {
                        let _ = tx.rollback().await;
                        return Err(WebhookStoreError::Database(
                            "Failed to resolve existing webhook record conflict".into(),
                        ));
                    }
                }
            }
        };

        // 2. Load payment record FOR UPDATE to prevent race conditions on payment state
        let payment_row = sqlx::query(
            r#"
            SELECT state, amount_minor, currency, refunded_amount_minor, events_seen, created_at, updated_at
            FROM payment_records
            WHERE payment_id = $1
            FOR UPDATE;
            "#,
        )
        .bind(payment_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        let mut payment_entity = match payment_row {
            Some(row) => {
                let state_str: String = row.get("state");
                let state: PaymentState = parse_payment_state(&state_str)?;
                let amt: i64 = row.get("amount_minor");
                let curr: String = row.get("currency");
                let ref_amt: i64 = row.get("refunded_amount_minor");
                let events_seen_json: serde_json::Value = row.get("events_seen");
                let events_seen: Vec<String> = serde_json::from_value(events_seen_json)
                    .map_err(|e| WebhookStoreError::Serialization(e.to_string()))?;
                let created_at: DateTime<Utc> = row.get("created_at");
                let updated_at: DateTime<Utc> = row.get("updated_at");

                PaymentEntity {
                    payment_id: payment_id.to_string(),
                    state,
                    amount_minor: amt as u64,
                    currency: curr,
                    refunded_amount_minor: ref_amt as u64,
                    events_seen,
                    created_at,
                    updated_at,
                }
            }
            None => PaymentEntity::new(payment_id.to_string(), amount_minor, currency.to_string()),
        };

        // 3. Apply state machine transition
        let transition =
            match payment_entity.apply_event(provider_event_id, &event_type, refund_amount_minor) {
                Ok(t) => t,
                Err(err) => {
                    // Record failure in webhook inbox and rollback payment changes
                    let _ = sqlx::query(
                        r#"
                    UPDATE webhook_inbox
                    SET status = 'failed', error_message = $1
                    WHERE id = $2;
                    "#,
                    )
                    .bind(err.to_string())
                    .bind(inbox_id)
                    .execute(&self.pool)
                    .await;

                    let _ = tx.rollback().await;
                    return Err(WebhookStoreError::PaymentState(err));
                }
            };

        // 4. Upsert payment record
        let events_seen_json = serde_json::to_value(&payment_entity.events_seen)
            .map_err(|e| WebhookStoreError::Serialization(e.to_string()))?;
        let state_val = serde_json::to_value(payment_entity.state)
            .map_err(|e| WebhookStoreError::Serialization(e.to_string()))?;
        let state_str = state_val.as_str().unwrap_or("CREATED");

        sqlx::query(
            r#"
            INSERT INTO payment_records (payment_id, provider, state, amount_minor, currency, refunded_amount_minor, events_seen, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (payment_id) DO UPDATE SET
                state = EXCLUDED.state,
                refunded_amount_minor = EXCLUDED.refunded_amount_minor,
                events_seen = EXCLUDED.events_seen,
                updated_at = EXCLUDED.updated_at;
            "#,
        )
        .bind(&payment_entity.payment_id)
        .bind(provider)
        .bind(state_str)
        .bind(payment_entity.amount_minor as i64)
        .bind(&payment_entity.currency)
        .bind(payment_entity.refunded_amount_minor as i64)
        .bind(&events_seen_json)
        .bind(payment_entity.created_at)
        .bind(payment_entity.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        // 5. Update webhook inbox status to processed
        sqlx::query(
            r#"
            UPDATE webhook_inbox
            SET status = 'processed', processed_at = NOW()
            WHERE id = $1;
            "#,
        )
        .bind(inbox_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        // 6. Commit transaction
        tx.commit()
            .await
            .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        Ok(WebhookProcessingResult::Applied {
            inbox_id,
            provider: provider.to_string(),
            provider_event_id: provider_event_id.to_string(),
            payment_id: payment_id.to_string(),
            transition,
            payment: payment_entity,
        })
    }

    async fn get_inbox_record(
        &self,
        provider: &str,
        provider_event_id: &str,
    ) -> Result<Option<WebhookInboxRecord>, WebhookStoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, provider, provider_event_id, event_type, payment_id, payload, status, error_message, received_at, processed_at
            FROM webhook_inbox
            WHERE provider = $1 AND provider_event_id = $2;
            "#,
        )
        .bind(provider)
        .bind(provider_event_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        if let Some(r) = row {
            let status_str: String = r.get("status");
            Ok(Some(WebhookInboxRecord {
                id: r.get("id"),
                provider: r.get("provider"),
                provider_event_id: r.get("provider_event_id"),
                event_type: r.get("event_type"),
                payment_id: r.get("payment_id"),
                payload: r.get("payload"),
                status: status_str.parse().unwrap_or(WebhookStatus::Pending),
                error_message: r.get("error_message"),
                received_at: r.get("received_at"),
                processed_at: r.get("processed_at"),
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_payment_record(
        &self,
        payment_id: &str,
    ) -> Result<Option<PaymentEntity>, WebhookStoreError> {
        let row = sqlx::query(
            r#"
            SELECT payment_id, state, amount_minor, currency, refunded_amount_minor, events_seen, created_at, updated_at
            FROM payment_records
            WHERE payment_id = $1;
            "#,
        )
        .bind(payment_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        if let Some(r) = row {
            let state_str: String = r.get("state");
            let state: PaymentState = parse_payment_state(&state_str)?;
            let amt: i64 = r.get("amount_minor");
            let curr: String = r.get("currency");
            let ref_amt: i64 = r.get("refunded_amount_minor");
            let events_seen_json: serde_json::Value = r.get("events_seen");
            let events_seen: Vec<String> = serde_json::from_value(events_seen_json)
                .map_err(|e| WebhookStoreError::Serialization(e.to_string()))?;

            Ok(Some(PaymentEntity {
                payment_id: r.get("payment_id"),
                state,
                amount_minor: amt as u64,
                currency: curr,
                refunded_amount_minor: ref_amt as u64,
                events_seen,
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }
}

/// Thread-safe in-memory implementation of WebhookStore for unit testing.
/// Replicates exact same unique constraint, concurrency coordination,
/// row locking, and rollback semantics without needing PostgreSQL running.
#[derive(Clone, Default)]
pub struct InMemoryWebhookStore {
    inbox: Arc<parking_lot::Mutex<HashMap<(String, String), WebhookInboxRecord>>>,
    payments: Arc<parking_lot::Mutex<HashMap<String, PaymentEntity>>>,
    next_id: Arc<parking_lot::Mutex<i64>>,
}

impl InMemoryWebhookStore {
    pub fn new() -> Self {
        Self {
            inbox: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            payments: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            next_id: Arc::new(parking_lot::Mutex::new(1)),
        }
    }
}

#[async_trait]
impl WebhookStore for InMemoryWebhookStore {
    async fn process_webhook_event(
        &self,
        params: &WebhookEventParams<'_>,
    ) -> Result<WebhookProcessingResult, WebhookStoreError> {
        let provider = params.provider;
        let provider_event_id = params.provider_event_id;
        let event_type = params.event_type.clone();
        let payment_id = params.payment_id;
        let amount_minor = params.amount_minor;
        let currency = params.currency;
        let refund_amount_minor = params.refund_amount_minor;
        let payload = params.payload;
        let key = (provider.to_string(), provider_event_id.to_string());

        let mut inbox_guard = self.inbox.lock();
        if let Some(existing) = inbox_guard.get_mut(&key) {
            match existing.status {
                WebhookStatus::Processed => {
                    return Ok(WebhookProcessingResult::AlreadyProcessed {
                        inbox_id: existing.id,
                        provider: provider.to_string(),
                        provider_event_id: provider_event_id.to_string(),
                        payment_id: existing
                            .payment_id
                            .clone()
                            .unwrap_or_else(|| payment_id.to_string()),
                        processed_at: existing.processed_at,
                    });
                }
                WebhookStatus::Pending => {
                    return Ok(WebhookProcessingResult::InFlight {
                        inbox_id: existing.id,
                        provider: provider.to_string(),
                        provider_event_id: provider_event_id.to_string(),
                    });
                }
                WebhookStatus::Failed => {
                    // Safe retry: claim it by setting to Pending
                    existing.status = WebhookStatus::Pending;
                    existing.error_message = None;
                    existing.received_at = Utc::now();
                }
            }
        } else {
            let mut id_guard = self.next_id.lock();
            let new_id = *id_guard;
            *id_guard += 1;

            let record = WebhookInboxRecord {
                id: new_id,
                provider: provider.to_string(),
                provider_event_id: provider_event_id.to_string(),
                event_type: format!("{:?}", event_type),
                payment_id: Some(payment_id.to_string()),
                payload: payload.clone(),
                status: WebhookStatus::Pending,
                error_message: None,
                received_at: Utc::now(),
                processed_at: None,
            };
            inbox_guard.insert(key.clone(), record);
        }

        let inbox_id = inbox_guard.get(&key).unwrap().id;
        drop(inbox_guard);

        // Transition payment entity
        let mut payments_guard = self.payments.lock();
        let mut payment_entity = payments_guard.get(payment_id).cloned().unwrap_or_else(|| {
            PaymentEntity::new(payment_id.to_string(), amount_minor, currency.to_string())
        });

        let transition =
            match payment_entity.apply_event(provider_event_id, &event_type, refund_amount_minor) {
                Ok(t) => t,
                Err(err) => {
                    let mut inbox_guard = self.inbox.lock();
                    if let Some(record) = inbox_guard.get_mut(&key) {
                        record.status = WebhookStatus::Failed;
                        record.error_message = Some(err.to_string());
                    }
                    return Err(WebhookStoreError::PaymentState(err));
                }
            };

        payments_guard.insert(payment_id.to_string(), payment_entity.clone());
        drop(payments_guard);

        // Mark inbox as processed
        let mut inbox_guard = self.inbox.lock();
        if let Some(record) = inbox_guard.get_mut(&key) {
            record.status = WebhookStatus::Processed;
            record.processed_at = Some(Utc::now());
        }

        Ok(WebhookProcessingResult::Applied {
            inbox_id,
            provider: provider.to_string(),
            provider_event_id: provider_event_id.to_string(),
            payment_id: payment_id.to_string(),
            transition,
            payment: payment_entity,
        })
    }

    async fn get_inbox_record(
        &self,
        provider: &str,
        provider_event_id: &str,
    ) -> Result<Option<WebhookInboxRecord>, WebhookStoreError> {
        let key = (provider.to_string(), provider_event_id.to_string());
        let guard = self.inbox.lock();
        Ok(guard.get(&key).cloned())
    }

    async fn get_payment_record(
        &self,
        payment_id: &str,
    ) -> Result<Option<PaymentEntity>, WebhookStoreError> {
        let guard = self.payments.lock();
        Ok(guard.get(payment_id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_webhook_store_lifecycle() {
        let store = InMemoryWebhookStore::new();
        let payload = serde_json::json!({"test": true});

        // 1. Initial capture
        let p1 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_001",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_001",
            amount_minor: 5000,
            currency: "INR",
            refund_amount_minor: None,
            payload: &payload,
        };
        let res1 = store
            .process_webhook_event(&p1)
            .await
            .expect("Initial webhook processing should succeed");

        match res1 {
            WebhookProcessingResult::Applied {
                payment,
                transition,
                ..
            } => {
                assert_eq!(payment.state, PaymentState::Captured);
                assert!(matches!(transition, TransitionOutcome::Applied { .. }));
            }
            _ => panic!("Expected Applied"),
        }

        // 2. Duplicate delivery of same event -> AlreadyProcessed
        let res2 = store
            .process_webhook_event(&p1)
            .await
            .expect("Duplicate event should return idempotent result");

        match res2 {
            WebhookProcessingResult::AlreadyProcessed { payment_id, .. } => {
                assert_eq!(payment_id, "pay_001");
            }
            _ => panic!("Expected AlreadyProcessed"),
        }

        // 3. Out-of-order failure event after capture -> LateEventIgnored
        let p3 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_002",
            event_type: PaymentEventType::PaymentFailed,
            payment_id: "pay_001",
            amount_minor: 5000,
            currency: "INR",
            refund_amount_minor: None,
            payload: &payload,
        };
        let res3 = store
            .process_webhook_event(&p3)
            .await
            .expect("Late failure processing should succeed");

        match res3 {
            WebhookProcessingResult::Applied {
                payment,
                transition,
                ..
            } => {
                // Must remain Captured!
                assert_eq!(payment.state, PaymentState::Captured);
                assert!(matches!(
                    transition,
                    TransitionOutcome::LateEventIgnored { .. }
                ));
            }
            _ => panic!("Expected Applied with LateEventIgnored"),
        }

        // 4. Refund event on captured payment -> Applied / PartiallyRefunded
        let p4 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_003",
            event_type: PaymentEventType::RefundCreated,
            payment_id: "pay_001",
            amount_minor: 5000,
            currency: "INR",
            refund_amount_minor: Some(2000),
            payload: &payload,
        };
        let res4 = store
            .process_webhook_event(&p4)
            .await
            .expect("Partial refund should succeed");

        match res4 {
            WebhookProcessingResult::Applied { payment, .. } => {
                assert_eq!(payment.state, PaymentState::PartiallyRefunded);
                assert_eq!(payment.refunded_amount_minor, 2000);
            }
            _ => panic!("Expected Applied"),
        }

        // Verify stored records
        let inbox_record = store
            .get_inbox_record("razorpay", "evt_001")
            .await
            .unwrap()
            .expect("Inbox record must exist");
        assert_eq!(inbox_record.status, WebhookStatus::Processed);

        let payment_record = store
            .get_payment_record("pay_001")
            .await
            .unwrap()
            .expect("Payment record must exist");
        assert_eq!(payment_record.state, PaymentState::PartiallyRefunded);
    }

    #[tokio::test]
    async fn test_in_memory_concurrent_duplicate_delivery() {
        let store = Arc::new(InMemoryWebhookStore::new());
        let payload = serde_json::json!({"concurrent": true});

        let mut handles = Vec::new();
        for _ in 0..10 {
            let s = Arc::clone(&store);
            let p = payload.clone();
            handles.push(tokio::spawn(async move {
                let params = WebhookEventParams {
                    provider: "razorpay",
                    provider_event_id: "evt_concurrent_100",
                    event_type: PaymentEventType::PaymentCaptured,
                    payment_id: "pay_concurrent_100",
                    amount_minor: 10000,
                    currency: "INR",
                    refund_amount_minor: None,
                    payload: &p,
                };
                s.process_webhook_event(&params).await
            }));
        }

        let mut applied_count = 0;
        let mut already_processed_count = 0;

        for h in handles {
            let res = h.await.unwrap().expect("Processing must not crash");
            match res {
                WebhookProcessingResult::Applied { .. } => applied_count += 1,
                WebhookProcessingResult::AlreadyProcessed { .. } => already_processed_count += 1,
                WebhookProcessingResult::InFlight { .. } => {}
            }
        }

        // Exactly one should have applied the transition
        assert_eq!(applied_count, 1);
        assert!(already_processed_count >= 1);

        let payment = store
            .get_payment_record("pay_concurrent_100")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(payment.state, PaymentState::Captured);
        assert_eq!(payment.events_seen.len(), 1);
    }

    #[tokio::test]
    async fn test_postgres_integration_if_configured() {
        let db_url = match std::env::var("DATABASE_URL") {
            Ok(url) if !url.is_empty() && url != "memory" => url,
            _ => return, // Skip test if no live PostgreSQL is configured
        };

        let pool = match PgPool::connect(&db_url).await {
            Ok(p) => p,
            Err(_) => return, // Skip test if postgres is unreachable
        };

        PostgresWebhookStore::init_schema(&pool)
            .await
            .expect("Schema init must succeed");

        let store = PostgresWebhookStore::new(pool.clone());
        let unique_event_id = format!("evt_pg_test_{}", uuid::Uuid::new_v4());
        let payment_id = format!("pay_pg_test_{}", uuid::Uuid::new_v4());
        let payload = serde_json::json!({"pg_test": true});

        // 1. Initial capture
        let params1 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &unique_event_id,
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &payment_id,
            amount_minor: 7500,
            currency: "INR",
            refund_amount_minor: None,
            payload: &payload,
        };
        let res1 = store
            .process_webhook_event(&params1)
            .await
            .expect("Postgres processing must succeed");

        match res1 {
            WebhookProcessingResult::Applied {
                payment,
                transition,
                ..
            } => {
                assert_eq!(payment.state, PaymentState::Captured);
                assert!(matches!(transition, TransitionOutcome::Applied { .. }));
            }
            _ => panic!("Expected Applied"),
        }

        // 2. Duplicate delivery -> AlreadyProcessed
        let res2 = store
            .process_webhook_event(&params1)
            .await
            .expect("Postgres duplicate delivery must succeed");

        match res2 {
            WebhookProcessingResult::AlreadyProcessed {
                provider_event_id, ..
            } => {
                assert_eq!(provider_event_id, unique_event_id);
            }
            _ => panic!("Expected AlreadyProcessed"),
        }

        // 3. Verify record in database
        let payment = store
            .get_payment_record(&payment_id)
            .await
            .unwrap()
            .expect("Payment record must exist in postgres");
        assert_eq!(payment.state, PaymentState::Captured);
        assert_eq!(payment.amount_minor, 7500);
        assert_eq!(payment.currency, "INR");
    }
}
