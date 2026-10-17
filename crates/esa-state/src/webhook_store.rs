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
    pub refund_id: Option<&'a str>,
    pub refund_amount_minor: Option<u64>,
    pub refund_status: Option<&'a str>,
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
    #[error("Entity mismatch: {0}")]
    Mismatch(String),
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
                refunds JSONB NOT NULL DEFAULT '[]'::jsonb,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            "#,
            r#"
            CREATE TABLE IF NOT EXISTS payment_refunds (
                refund_id VARCHAR(255) PRIMARY KEY,
                payment_id VARCHAR(255) NOT NULL,
                provider VARCHAR(64) NOT NULL,
                amount_minor BIGINT NOT NULL,
                currency VARCHAR(10) NOT NULL,
                status VARCHAR(50) NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            "#,
            "ALTER TABLE payment_records ADD COLUMN IF NOT EXISTS refunds JSONB NOT NULL DEFAULT '[]'::jsonb;",
            "CREATE INDEX IF NOT EXISTS idx_webhook_inbox_provider_event ON webhook_inbox(provider, provider_event_id);",
            "CREATE INDEX IF NOT EXISTS idx_webhook_inbox_payment_id ON webhook_inbox(payment_id);",
            "CREATE INDEX IF NOT EXISTS idx_payment_records_provider ON payment_records(provider);",
            "CREATE INDEX IF NOT EXISTS idx_payment_refunds_payment_id ON payment_refunds(payment_id);",
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
        let refund_id = params.refund_id;
        let refund_amount_minor = params.refund_amount_minor;
        let refund_status = params.refund_status;
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
                    SELECT id, status, processed_at, payment_id, event_type, payload
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
                        let existing_event_type: String = row.get("event_type");
                        let existing_payload: serde_json::Value = row.get("payload");

                        if let Some(ref pid) = existing_payment_id {
                            if pid != payment_id {
                                let _ = tx.rollback().await;
                                return Err(WebhookStoreError::Mismatch(format!(
                                    "Payment ID mismatch for event '{}': expected '{}', received '{}'",
                                    provider_event_id, pid, payment_id
                                )));
                            }
                        }

                        let expected_event_type_str = format!("{:?}", event_type);
                        if existing_event_type != expected_event_type_str {
                            let _ = tx.rollback().await;
                            return Err(WebhookStoreError::Mismatch(format!(
                                "Event type mismatch for duplicate event '{}': expected '{}', received '{}'",
                                provider_event_id, existing_event_type, expected_event_type_str
                            )));
                        }

                        if existing_payload != *payload {
                            let _ = tx.rollback().await;
                            return Err(WebhookStoreError::Mismatch(format!(
                                "Payload mismatch for duplicate event '{}'",
                                provider_event_id
                            )));
                        }

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

        // 2. Serialize concurrent webhook events for the same (provider, payment_id)
        // using a PostgreSQL transaction-scoped advisory lock.
        // This guarantees that when payment_records has no row yet, concurrent events
        // for the same payment are strictly serialized and cannot race to insert uncoordinated rows.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1), hashtext($2));")
            .bind(provider)
            .bind(payment_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        // 3. Verify refund ID consistency across refunds table
        if let Some(r_id) = refund_id {
            let existing_refund = sqlx::query(
                r#"
                SELECT payment_id, amount_minor, currency, status
                FROM payment_refunds
                WHERE refund_id = $1
                FOR UPDATE;
                "#,
            )
            .bind(r_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

            if let Some(rf_row) = existing_refund {
                let rf_pid: String = rf_row.get("payment_id");
                let rf_amt: i64 = rf_row.get("amount_minor");
                let rf_curr: String = rf_row.get("currency");

                if rf_pid != payment_id {
                    let err_msg = format!(
                        "Refund '{}' payment ID mismatch: existing '{}', received '{}'",
                        r_id, rf_pid, payment_id
                    );
                    let _ = sqlx::query(
                        "UPDATE webhook_inbox SET status = 'failed', error_message = $1 WHERE id = $2;",
                    )
                    .bind(&err_msg)
                    .bind(inbox_id)
                    .execute(&mut *tx)
                    .await;
                    let _ = tx.commit().await;
                    return Err(WebhookStoreError::Mismatch(err_msg));
                }

                if rf_curr != currency {
                    let err_msg = format!(
                        "Refund '{}' currency mismatch: existing '{}', received '{}'",
                        r_id, rf_curr, currency
                    );
                    let _ = sqlx::query(
                        "UPDATE webhook_inbox SET status = 'failed', error_message = $1 WHERE id = $2;",
                    )
                    .bind(&err_msg)
                    .bind(inbox_id)
                    .execute(&mut *tx)
                    .await;
                    let _ = tx.commit().await;
                    return Err(WebhookStoreError::Mismatch(err_msg));
                }

                if let Some(ramt) = refund_amount_minor {
                    if (rf_amt as u64) != ramt {
                        let err_msg = format!(
                            "Refund '{}' amount mismatch: existing {} minor units, received {} minor units",
                            r_id, rf_amt, ramt
                        );
                        let _ = sqlx::query(
                            "UPDATE webhook_inbox SET status = 'failed', error_message = $1 WHERE id = $2;",
                        )
                        .bind(&err_msg)
                        .bind(inbox_id)
                        .execute(&mut *tx)
                        .await;
                        let _ = tx.commit().await;
                        return Err(WebhookStoreError::Mismatch(err_msg));
                    }
                }
            }
        }

        // 4. Load payment record FOR UPDATE to prevent race conditions on payment state
        let payment_row = sqlx::query(
            r#"
            SELECT provider, state, amount_minor, currency, refunded_amount_minor, events_seen, refunds, created_at, updated_at
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
                let existing_provider: String = row.get("provider");
                if existing_provider != provider {
                    let err_msg = format!(
                        "Provider mismatch for payment '{}': expected '{}', received '{}'",
                        payment_id, existing_provider, provider
                    );
                    let _ = sqlx::query(
                        "UPDATE webhook_inbox SET status = 'failed', error_message = $1 WHERE id = $2;",
                    )
                    .bind(&err_msg)
                    .bind(inbox_id)
                    .execute(&mut *tx)
                    .await;
                    let _ = tx.commit().await;
                    return Err(WebhookStoreError::Mismatch(err_msg));
                }

                let curr: String = row.get("currency");
                if curr != currency {
                    let err_msg = format!(
                        "Currency mismatch for payment '{}': expected '{}', received '{}'",
                        payment_id, curr, currency
                    );
                    let _ = sqlx::query(
                        "UPDATE webhook_inbox SET status = 'failed', error_message = $1 WHERE id = $2;",
                    )
                    .bind(&err_msg)
                    .bind(inbox_id)
                    .execute(&mut *tx)
                    .await;
                    let _ = tx.commit().await;
                    return Err(WebhookStoreError::Mismatch(err_msg));
                }

                let amt: i64 = row.get("amount_minor");
                let existing_amount = amt as u64;
                let amount_matches = if refund_amount_minor.is_some() {
                    amount_minor == existing_amount || Some(amount_minor) == refund_amount_minor
                } else {
                    amount_minor == existing_amount
                };

                if !amount_matches {
                    let err_msg = format!(
                        "Amount mismatch for payment '{}': expected {} minor units, received {} minor units",
                        payment_id, existing_amount, amount_minor
                    );
                    let _ = sqlx::query(
                        "UPDATE webhook_inbox SET status = 'failed', error_message = $1 WHERE id = $2;",
                    )
                    .bind(&err_msg)
                    .bind(inbox_id)
                    .execute(&mut *tx)
                    .await;
                    let _ = tx.commit().await;
                    return Err(WebhookStoreError::Mismatch(err_msg));
                }

                let state_str: String = row.get("state");
                let state: PaymentState = parse_payment_state(&state_str)?;
                let ref_amt: i64 = row.get("refunded_amount_minor");
                let events_seen_json: serde_json::Value = row.get("events_seen");
                let events_seen: Vec<String> = serde_json::from_value(events_seen_json)
                    .map_err(|e| WebhookStoreError::Serialization(e.to_string()))?;
                let refunds_json: serde_json::Value =
                    row.try_get("refunds").unwrap_or(serde_json::json!([]));
                let refunds: Vec<esa_core::payment_state::RefundEntity> =
                    serde_json::from_value(refunds_json).unwrap_or_default();
                let created_at: DateTime<Utc> = row.get("created_at");
                let updated_at: DateTime<Utc> = row.get("updated_at");

                PaymentEntity {
                    payment_id: payment_id.to_string(),
                    state,
                    amount_minor: existing_amount,
                    currency: curr,
                    refunded_amount_minor: ref_amt as u64,
                    events_seen,
                    refunds,
                    created_at,
                    updated_at,
                }
            }
            None => PaymentEntity::new(payment_id.to_string(), amount_minor, currency.to_string()),
        };

        // 5. Apply state machine transition
        let transition = match payment_entity.apply_event_with_refund(
            provider_event_id,
            &event_type,
            refund_amount_minor,
            refund_id,
            refund_status,
        ) {
            Ok(t) => t,
            Err(err) => {
                // Record failure in webhook inbox using the CURRENT transaction.
                // DO NOT use a second pool connection, which deadlocks on the row lock held by tx!
                // Committing tx safely persists the failed status and releases all locks,
                // while leaving payment_records untouched because no writes occurred yet.
                let _ = sqlx::query(
                    r#"
                    UPDATE webhook_inbox
                    SET status = 'failed', error_message = $1
                    WHERE id = $2;
                    "#,
                )
                .bind(err.to_string())
                .bind(inbox_id)
                .execute(&mut *tx)
                .await;

                let _ = tx.commit().await;
                return Err(WebhookStoreError::PaymentState(err));
            }
        };

        // 6. Upsert payment record
        let events_seen_json = serde_json::to_value(&payment_entity.events_seen)
            .map_err(|e| WebhookStoreError::Serialization(e.to_string()))?;
        let refunds_json = serde_json::to_value(&payment_entity.refunds)
            .map_err(|e| WebhookStoreError::Serialization(e.to_string()))?;
        let state_val = serde_json::to_value(payment_entity.state)
            .map_err(|e| WebhookStoreError::Serialization(e.to_string()))?;
        let state_str = state_val.as_str().unwrap_or("CREATED");

        sqlx::query(
            r#"
            INSERT INTO payment_records (payment_id, provider, state, amount_minor, currency, refunded_amount_minor, events_seen, refunds, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (payment_id) DO UPDATE SET
                state = EXCLUDED.state,
                refunded_amount_minor = EXCLUDED.refunded_amount_minor,
                events_seen = EXCLUDED.events_seen,
                refunds = EXCLUDED.refunds,
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
        .bind(&refunds_json)
        .bind(payment_entity.created_at)
        .bind(payment_entity.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        // 7. Upsert refund record into payment_refunds if applicable
        if let Some(r_id) = refund_id {
            let r_status = refund_status.unwrap_or_else(|| {
                if event_type == PaymentEventType::RefundProcessed {
                    "processed"
                } else if event_type == PaymentEventType::RefundFailed {
                    "failed"
                } else {
                    "pending"
                }
            });
            let r_amt = refund_amount_minor.unwrap_or(amount_minor) as i64;

            sqlx::query(
                r#"
                INSERT INTO payment_refunds (refund_id, payment_id, provider, amount_minor, currency, status, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
                ON CONFLICT (refund_id) DO UPDATE SET
                    status = CASE WHEN payment_refunds.status = 'processed' THEN 'processed' ELSE EXCLUDED.status END,
                    updated_at = NOW();
                "#,
            )
            .bind(r_id)
            .bind(payment_id)
            .bind(provider)
            .bind(r_amt)
            .bind(currency)
            .bind(r_status)
            .execute(&mut *tx)
            .await
            .map_err(|e| WebhookStoreError::Database(e.to_string()))?;
        }

        // 8. Update webhook inbox status to processed
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

        // 9. Commit transaction
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
            SELECT payment_id, state, amount_minor, currency, refunded_amount_minor, events_seen, refunds, created_at, updated_at
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
            let refunds_json: serde_json::Value =
                r.try_get("refunds").unwrap_or(serde_json::json!([]));
            let refunds: Vec<esa_core::payment_state::RefundEntity> =
                serde_json::from_value(refunds_json).unwrap_or_default();

            Ok(Some(PaymentEntity {
                payment_id: r.get("payment_id"),
                state,
                amount_minor: amt as u64,
                currency: curr,
                refunded_amount_minor: ref_amt as u64,
                events_seen,
                refunds,
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone)]
struct InMemoryRefundEntry {
    payment_id: String,
    amount_minor: u64,
    currency: String,
    status: String,
}

/// Thread-safe in-memory implementation of WebhookStore for unit testing.
/// Replicates exact same unique constraint, concurrency coordination,
/// row locking, and rollback semantics without needing PostgreSQL running.
#[derive(Clone, Default)]
pub struct InMemoryWebhookStore {
    inbox: Arc<parking_lot::Mutex<HashMap<(String, String), WebhookInboxRecord>>>,
    payments: Arc<parking_lot::Mutex<HashMap<String, PaymentEntity>>>,
    refunds: Arc<parking_lot::Mutex<HashMap<String, InMemoryRefundEntry>>>,
    next_id: Arc<parking_lot::Mutex<i64>>,
}

impl InMemoryWebhookStore {
    pub fn new() -> Self {
        Self {
            inbox: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            payments: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            refunds: Arc::new(parking_lot::Mutex::new(HashMap::new())),
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
        let refund_id = params.refund_id;
        let refund_amount_minor = params.refund_amount_minor;
        let refund_status = params.refund_status;
        let payload = params.payload;
        let key = (provider.to_string(), provider_event_id.to_string());

        let mut inbox_guard = self.inbox.lock();
        if let Some(existing) = inbox_guard.get_mut(&key) {
            if let Some(ref pid) = existing.payment_id {
                if pid != payment_id {
                    return Err(WebhookStoreError::Mismatch(format!(
                        "Payment ID mismatch for event '{}': expected '{}', received '{}'",
                        provider_event_id, pid, payment_id
                    )));
                }
            }

            let expected_event_type_str = format!("{:?}", event_type);
            if existing.event_type != expected_event_type_str {
                return Err(WebhookStoreError::Mismatch(format!(
                    "Event type mismatch for duplicate event '{}': expected '{}', received '{}'",
                    provider_event_id, existing.event_type, expected_event_type_str
                )));
            }

            if existing.payload != *payload {
                return Err(WebhookStoreError::Mismatch(format!(
                    "Payload mismatch for duplicate event '{}'",
                    provider_event_id
                )));
            }

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

        // Check refund ID consistency across store
        if let Some(r_id) = refund_id {
            let refunds_guard = self.refunds.lock();
            if let Some(entry) = refunds_guard.get(r_id) {
                if entry.payment_id != payment_id {
                    let err_msg = format!(
                        "Refund '{}' payment ID mismatch: existing '{}', received '{}'",
                        r_id, entry.payment_id, payment_id
                    );
                    let mut inbox_guard = self.inbox.lock();
                    if let Some(record) = inbox_guard.get_mut(&key) {
                        record.status = WebhookStatus::Failed;
                        record.error_message = Some(err_msg.clone());
                    }
                    return Err(WebhookStoreError::Mismatch(err_msg));
                }

                if entry.currency != currency {
                    let err_msg = format!(
                        "Refund '{}' currency mismatch: existing '{}', received '{}'",
                        r_id, entry.currency, currency
                    );
                    let mut inbox_guard = self.inbox.lock();
                    if let Some(record) = inbox_guard.get_mut(&key) {
                        record.status = WebhookStatus::Failed;
                        record.error_message = Some(err_msg.clone());
                    }
                    return Err(WebhookStoreError::Mismatch(err_msg));
                }

                if let Some(ramt) = refund_amount_minor {
                    if entry.amount_minor != ramt {
                        let err_msg = format!(
                            "Refund '{}' amount mismatch: existing {} minor units, received {} minor units",
                            r_id, entry.amount_minor, ramt
                        );
                        let mut inbox_guard = self.inbox.lock();
                        if let Some(record) = inbox_guard.get_mut(&key) {
                            record.status = WebhookStatus::Failed;
                            record.error_message = Some(err_msg.clone());
                        }
                        return Err(WebhookStoreError::Mismatch(err_msg));
                    }
                }
            }
        }

        // Transition payment entity with strict mismatch validation
        let mut payments_guard = self.payments.lock();
        let mut payment_entity = if let Some(existing) = payments_guard.get(payment_id) {
            if existing.currency != currency {
                let err_msg = format!(
                    "Currency mismatch for payment '{}': expected '{}', received '{}'",
                    payment_id, existing.currency, currency
                );
                let mut inbox_guard = self.inbox.lock();
                if let Some(record) = inbox_guard.get_mut(&key) {
                    record.status = WebhookStatus::Failed;
                    record.error_message = Some(err_msg.clone());
                }
                return Err(WebhookStoreError::Mismatch(err_msg));
            }

            let amount_matches = if refund_amount_minor.is_some() {
                amount_minor == existing.amount_minor || Some(amount_minor) == refund_amount_minor
            } else {
                amount_minor == existing.amount_minor
            };

            if !amount_matches {
                let err_msg = format!(
                    "Amount mismatch for payment '{}': expected {} minor units, received {} minor units",
                    payment_id, existing.amount_minor, amount_minor
                );
                let mut inbox_guard = self.inbox.lock();
                if let Some(record) = inbox_guard.get_mut(&key) {
                    record.status = WebhookStatus::Failed;
                    record.error_message = Some(err_msg.clone());
                }
                return Err(WebhookStoreError::Mismatch(err_msg));
            }

            existing.clone()
        } else {
            PaymentEntity::new(payment_id.to_string(), amount_minor, currency.to_string())
        };

        let transition = match payment_entity.apply_event_with_refund(
            provider_event_id,
            &event_type,
            refund_amount_minor,
            refund_id,
            refund_status,
        ) {
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

        // Update refund tracker
        if let Some(r_id) = refund_id {
            let r_amt = refund_amount_minor.unwrap_or(amount_minor);
            let mut refunds_guard = self.refunds.lock();
            let new_st = refund_status.unwrap_or_else(|| {
                if event_type == PaymentEventType::RefundProcessed {
                    "processed"
                } else if event_type == PaymentEventType::RefundFailed {
                    "failed"
                } else {
                    "pending"
                }
            });
            if let Some(existing_entry) = refunds_guard.get_mut(r_id) {
                if existing_entry.status != "processed" {
                    existing_entry.status = new_st.to_string();
                }
            } else {
                refunds_guard.insert(
                    r_id.to_string(),
                    InMemoryRefundEntry {
                        payment_id: payment_id.to_string(),
                        amount_minor: r_amt,
                        currency: currency.to_string(),
                        status: new_st.to_string(),
                    },
                );
            }
        }

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
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
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
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
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

        // 4. Refund processed event on captured payment -> Applied / PartiallyRefunded
        let p4 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_003",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_001",
            amount_minor: 5000,
            currency: "INR",
            refund_id: Some("rfnd_001"),
            refund_amount_minor: Some(2000),
            refund_status: Some("processed"),
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
    async fn test_refund_lifecycle_and_idempotency_by_refund_id() {
        let store = InMemoryWebhookStore::new();
        let payload = serde_json::json!({"test": true});

        // 1. Initial capture
        let p_cap = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_cap_1",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_rfnd_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&p_cap).await.unwrap();

        // 2. Pending refund (refund.created with status="pending")
        // MUST NOT increment refunded_amount_minor!
        let p_rf_pending = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rf_pending_1",
            event_type: PaymentEventType::RefundCreated,
            payment_id: "pay_rfnd_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_001"),
            refund_amount_minor: Some(3000),
            refund_status: Some("pending"),
            payload: &payload,
        };
        let res_pending = store.process_webhook_event(&p_rf_pending).await.unwrap();
        match res_pending {
            WebhookProcessingResult::Applied { payment, .. } => {
                assert_eq!(
                    payment.refunded_amount_minor, 0,
                    "Pending refund must not change refunded balance"
                );
                assert_eq!(payment.state, PaymentState::Captured);
                assert_eq!(payment.refunds.len(), 1);
                assert_eq!(payment.refunds[0].status, "pending");
            }
            _ => panic!("Expected Applied"),
        }

        // 3. Failed refund (refund.failed)
        // MUST NOT increment refunded_amount_minor!
        let p_rf_failed = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rf_failed_1",
            event_type: PaymentEventType::RefundFailed,
            payment_id: "pay_rfnd_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_002"),
            refund_amount_minor: Some(2000),
            refund_status: Some("failed"),
            payload: &payload,
        };
        let res_failed = store.process_webhook_event(&p_rf_failed).await.unwrap();
        match res_failed {
            WebhookProcessingResult::Applied { payment, .. } => {
                assert_eq!(
                    payment.refunded_amount_minor, 0,
                    "Failed refund must not change refunded balance"
                );
                assert_eq!(payment.state, PaymentState::Captured);
            }
            _ => panic!("Expected Applied"),
        }

        // 4. Same refund ID (rfnd_001) transitions from pending to processed (refund.processed)
        // DIFFERENT provider event ID (evt_rf_proc_1 vs evt_rf_pending_1)!
        let p_rf_proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rf_proc_1",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_rfnd_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_001"),
            refund_amount_minor: Some(3000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_proc = store.process_webhook_event(&p_rf_proc).await.unwrap();
        match res_proc {
            WebhookProcessingResult::Applied { payment, .. } => {
                assert_eq!(payment.refunded_amount_minor, 3000);
                assert_eq!(payment.state, PaymentState::PartiallyRefunded);
            }
            _ => panic!("Expected Applied"),
        }

        // 5. Duplicate delivery of processed refund with yet ANOTHER event ID (evt_rf_proc_dup)
        // MUST NOT increment refunded balance a second time!
        let p_rf_dup = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rf_proc_dup",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_rfnd_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_001"),
            refund_amount_minor: Some(3000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_dup = store.process_webhook_event(&p_rf_dup).await.unwrap();
        match res_dup {
            WebhookProcessingResult::Applied {
                payment,
                transition,
                ..
            } => {
                assert_eq!(
                    payment.refunded_amount_minor, 3000,
                    "Duplicate delivery of processed refund must not increment balance"
                );
                assert!(matches!(
                    transition,
                    TransitionOutcome::DuplicateIgnored { .. }
                ));
            }
            _ => panic!("Expected Applied with DuplicateIgnored"),
        }

        // 6. Out-of-order late pending/failed event for rfnd_001 (already processed!)
        // MUST NOT downgrade successfully processed refund!
        let p_rf_stale = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rf_stale_1",
            event_type: PaymentEventType::RefundCreated,
            payment_id: "pay_rfnd_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_001"),
            refund_amount_minor: Some(3000),
            refund_status: Some("pending"),
            payload: &payload,
        };
        let res_stale = store.process_webhook_event(&p_rf_stale).await.unwrap();
        match res_stale {
            WebhookProcessingResult::Applied {
                payment,
                transition,
                ..
            } => {
                assert_eq!(payment.refunded_amount_minor, 3000);
                assert_eq!(payment.state, PaymentState::PartiallyRefunded);
                assert!(matches!(
                    transition,
                    TransitionOutcome::LateEventIgnored { .. }
                ));
            }
            _ => panic!("Expected Applied with LateEventIgnored"),
        }

        // 7. Conflicting refund details: reused refund ID (rfnd_001) with conflicting amount
        let p_rf_conflict_amt = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rf_conflict_1",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_rfnd_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_001"),
            refund_amount_minor: Some(9999), // Conflicts with existing 3000!
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_conflict = store.process_webhook_event(&p_rf_conflict_amt).await;
        assert!(matches!(
            res_conflict,
            Err(WebhookStoreError::Mismatch(_)) | Err(WebhookStoreError::PaymentState(_))
        ));

        // 8. Conflicting refund details: reused refund ID (rfnd_001) for DIFFERENT payment ID
        let p_rf_conflict_pid = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rf_conflict_2",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_different_999",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_001"),
            refund_amount_minor: Some(3000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_pid_conflict = store.process_webhook_event(&p_rf_conflict_pid).await;
        assert!(matches!(
            res_pid_conflict,
            Err(WebhookStoreError::Mismatch(_))
        ));
    }

    #[tokio::test]
    async fn test_duplicate_event_id_with_conflicting_payload_rejected() {
        let store = InMemoryWebhookStore::new();
        let payload_orig = serde_json::json!({"action": "original", "amount": 5000});
        let payload_tampered = serde_json::json!({"action": "tampered", "amount": 5000});

        let p1 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_dup_payload_1",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_dup_1",
            amount_minor: 5000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload_orig,
        };
        store.process_webhook_event(&p1).await.unwrap();

        // Duplicate event ID with exact same payload -> AlreadyProcessed
        let res_same = store.process_webhook_event(&p1).await.unwrap();
        assert!(matches!(
            res_same,
            WebhookProcessingResult::AlreadyProcessed { .. }
        ));

        // Duplicate event ID with conflicting payload, even with same payment ID -> Mismatch!
        let p_tampered = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_dup_payload_1",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_dup_1", // Same payment ID
            amount_minor: 5000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload_tampered, // Different payload!
        };
        let res_tampered = store.process_webhook_event(&p_tampered).await;
        assert!(matches!(res_tampered, Err(WebhookStoreError::Mismatch(_))));

        // Duplicate event ID with conflicting event type -> Mismatch!
        let p_diff_type = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_dup_payload_1",
            event_type: PaymentEventType::PaymentFailed, // Different event type!
            payment_id: "pay_dup_1",
            amount_minor: 5000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload_orig,
        };
        let res_diff_type = store.process_webhook_event(&p_diff_type).await;
        assert!(matches!(res_diff_type, Err(WebhookStoreError::Mismatch(_))));
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
                    refund_id: None,
                    refund_amount_minor: None,
                    refund_status: None,
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
    async fn test_in_memory_mismatch_rejections() {
        let store = InMemoryWebhookStore::new();
        let payload = serde_json::json!({"test": true});

        // 1. Initial capture
        let p1 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_mismatch_001",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_mismatch_100",
            amount_minor: 5000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&p1).await.unwrap();

        // 2. Second event for same payment with different currency -> rejected Mismatch
        let p_bad_curr = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_mismatch_002",
            event_type: PaymentEventType::RefundCreated,
            payment_id: "pay_mismatch_100",
            amount_minor: 5000,
            currency: "USD", // Mismatch!
            refund_id: Some("rfnd_bad_curr"),
            refund_amount_minor: Some(1000),
            refund_status: Some("pending"),
            payload: &payload,
        };
        let res_curr = store.process_webhook_event(&p_bad_curr).await;
        assert!(matches!(res_curr, Err(WebhookStoreError::Mismatch(_))));

        // 3. Second event for same payment with different original amount -> rejected Mismatch
        let p_bad_amt = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_mismatch_003",
            event_type: PaymentEventType::RefundCreated,
            payment_id: "pay_mismatch_100",
            amount_minor: 99999, // Mismatch!
            currency: "INR",
            refund_id: Some("rfnd_bad_amt"),
            refund_amount_minor: Some(1000),
            refund_status: Some("pending"),
            payload: &payload,
        };
        let res_amt = store.process_webhook_event(&p_bad_amt).await;
        assert!(matches!(res_amt, Err(WebhookStoreError::Mismatch(_))));

        // 4. Existing event ID claimed with a different payment_id -> rejected Mismatch
        let p_bad_pid = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_mismatch_001", // Reused event ID
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_different_200", // Different payment ID!
            amount_minor: 5000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        let res_pid = store.process_webhook_event(&p_bad_pid).await;
        assert!(matches!(res_pid, Err(WebhookStoreError::Mismatch(_))));
    }

    #[tokio::test]
    async fn test_in_memory_invalid_transition_persistence() {
        let store = InMemoryWebhookStore::new();
        let payload = serde_json::json!({"test": true});

        // 1. Initial capture for 5000 paise
        let p1 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_inv_001",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_inv_100",
            amount_minor: 5000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&p1).await.unwrap();

        // 2. Over-refund (attempt to refund 10000 paise on 5000 paise payment)
        let p_invalid = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_inv_002",
            event_type: PaymentEventType::RefundCreated,
            payment_id: "pay_inv_100",
            amount_minor: 5000,
            currency: "INR",
            refund_id: Some("rfnd_exceeds_1"),
            refund_amount_minor: Some(10000), // Exceeds balance!
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res = store.process_webhook_event(&p_invalid).await;
        assert!(matches!(res, Err(WebhookStoreError::PaymentState(_))));

        // Inbox record must be recorded as 'failed'
        let inbox = store
            .get_inbox_record("razorpay", "evt_inv_002")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(inbox.status, WebhookStatus::Failed);
        assert!(inbox.error_message.is_some());

        // Payment record must remain Captured (untampered)
        let payment = store
            .get_payment_record("pay_inv_100")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(payment.state, PaymentState::Captured);
        assert_eq!(payment.refunded_amount_minor, 0);
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

        let store = Arc::new(PostgresWebhookStore::new(pool.clone()));
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
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
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

        // 3. Duplicate event ID with conflicting payload -> Mismatch
        let conflicting_payload = serde_json::json!({"tampered": true});
        let conflicting_payload_params = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &unique_event_id,
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &payment_id,
            amount_minor: 7500,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &conflicting_payload,
        };
        let res_payload_conflict = store
            .process_webhook_event(&conflicting_payload_params)
            .await;
        assert!(matches!(
            res_payload_conflict,
            Err(WebhookStoreError::Mismatch(_))
        ));

        // 4. Mismatch rejection on Postgres (bad currency)
        let bad_currency_event_id = format!("evt_pg_curr_{}", uuid::Uuid::new_v4());
        let bad_currency_params = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &bad_currency_event_id,
            event_type: PaymentEventType::RefundCreated,
            payment_id: &payment_id,
            amount_minor: 7500,
            currency: "USD", // Mismatch!
            refund_id: Some("rfnd_pg_bad_curr"),
            refund_amount_minor: Some(1000),
            refund_status: Some("pending"),
            payload: &payload,
        };
        let res_mismatch = store.process_webhook_event(&bad_currency_params).await;
        assert!(matches!(res_mismatch, Err(WebhookStoreError::Mismatch(_))));

        // 5. Invalid transition handling: deadlock-free failure persistence
        let invalid_trans_event_id = format!("evt_pg_inv_{}", uuid::Uuid::new_v4());
        let invalid_params = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &invalid_trans_event_id,
            event_type: PaymentEventType::RefundCreated,
            payment_id: &payment_id,
            amount_minor: 7500,
            currency: "INR",
            refund_id: Some("rfnd_pg_exceeds"),
            refund_amount_minor: Some(99999), // Exceeds balance!
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_inv = store.process_webhook_event(&invalid_params).await;
        assert!(matches!(res_inv, Err(WebhookStoreError::PaymentState(_))));

        let failed_inbox = store
            .get_inbox_record("razorpay", &invalid_trans_event_id)
            .await
            .unwrap()
            .expect("Inbox record for invalid transition must be persisted as failed");
        assert_eq!(failed_inbox.status, WebhookStatus::Failed);

        // 6. Postgres refund lifecycle: pending followed by processed with DIFFERENT event IDs
        let pg_rfnd_id = format!("rfnd_pg_{}", uuid::Uuid::new_v4());
        let evt_rf_pending = format!("evt_rf_pending_{}", uuid::Uuid::new_v4());
        let params_rf_pending = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &evt_rf_pending,
            event_type: PaymentEventType::RefundCreated,
            payment_id: &payment_id,
            amount_minor: 7500,
            currency: "INR",
            refund_id: Some(&pg_rfnd_id),
            refund_amount_minor: Some(2500),
            refund_status: Some("pending"),
            payload: &payload,
        };
        let res_rf_pending = store
            .process_webhook_event(&params_rf_pending)
            .await
            .unwrap();
        match res_rf_pending {
            WebhookProcessingResult::Applied { payment, .. } => {
                assert_eq!(
                    payment.refunded_amount_minor, 0,
                    "Postgres: pending refund must not increment balance"
                );
            }
            _ => panic!("Expected Applied"),
        }

        let evt_rf_proc = format!("evt_rf_proc_{}", uuid::Uuid::new_v4());
        let params_rf_proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &evt_rf_proc,
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &payment_id,
            amount_minor: 7500,
            currency: "INR",
            refund_id: Some(&pg_rfnd_id),
            refund_amount_minor: Some(2500),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_rf_proc = store.process_webhook_event(&params_rf_proc).await.unwrap();
        match res_rf_proc {
            WebhookProcessingResult::Applied { payment, .. } => {
                assert_eq!(
                    payment.refunded_amount_minor, 2500,
                    "Postgres: processed refund must increment balance"
                );
                assert_eq!(payment.state, PaymentState::PartiallyRefunded);
            }
            _ => panic!("Expected Applied"),
        }

        // Duplicate delivery of processed refund with another event ID
        let evt_rf_proc_dup = format!("evt_rf_dup_{}", uuid::Uuid::new_v4());
        let params_rf_dup = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &evt_rf_proc_dup,
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &payment_id,
            amount_minor: 7500,
            currency: "INR",
            refund_id: Some(&pg_rfnd_id),
            refund_amount_minor: Some(2500),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_rf_dup = store.process_webhook_event(&params_rf_dup).await.unwrap();
        match res_rf_dup {
            WebhookProcessingResult::Applied {
                payment,
                transition,
                ..
            } => {
                assert_eq!(
                    payment.refunded_amount_minor, 2500,
                    "Postgres: duplicate refund ID delivery must NOT increment balance again"
                );
                assert!(matches!(
                    transition,
                    TransitionOutcome::DuplicateIgnored { .. }
                ));
            }
            _ => panic!("Expected Applied with DuplicateIgnored"),
        }

        // 7. Concurrent event serialization when payment_records has no row yet
        let empty_payment_id = format!("pay_concurrent_empty_{}", uuid::Uuid::new_v4());
        let mut handles = Vec::new();
        for i in 0..6 {
            let s = Arc::clone(&store);
            let pid = empty_payment_id.clone();
            let eid = format!("evt_conc_empty_{}_{}", i, uuid::Uuid::new_v4());
            let p = payload.clone();
            handles.push(tokio::spawn(async move {
                let params = WebhookEventParams {
                    provider: "razorpay",
                    provider_event_id: &eid,
                    event_type: PaymentEventType::PaymentCaptured,
                    payment_id: &pid,
                    amount_minor: 12000,
                    currency: "INR",
                    refund_id: None,
                    refund_amount_minor: None,
                    refund_status: None,
                    payload: &p,
                };
                s.process_webhook_event(&params).await
            }));
        }

        for h in handles {
            let res = h.await.unwrap();
            assert!(
                res.is_ok(),
                "Concurrent processing on empty payment row must succeed without deadlock"
            );
        }

        // 8. Verify record in database
        let payment = store
            .get_payment_record(&payment_id)
            .await
            .unwrap()
            .expect("Payment record must exist in postgres");
        assert_eq!(payment.state, PaymentState::PartiallyRefunded);
        assert_eq!(payment.amount_minor, 7500);
        assert_eq!(payment.refunded_amount_minor, 2500);
        assert_eq!(payment.currency, "INR");
    }
}
