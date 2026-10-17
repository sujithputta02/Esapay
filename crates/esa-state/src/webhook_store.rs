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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundRecord {
    pub provider: String,
    pub refund_id: String,
    pub payment_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub status: String,
    pub applied_to_balance: bool,
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

    /// Retrieve a refund record by provider and refund ID
    async fn get_refund_record(
        &self,
        _provider: &str,
        _refund_id: &str,
    ) -> Result<Option<RefundRecord>, WebhookStoreError> {
        Ok(None)
    }
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
                provider VARCHAR(64) NOT NULL,
                refund_id VARCHAR(255) NOT NULL,
                payment_id VARCHAR(255) NOT NULL,
                amount_minor BIGINT NOT NULL,
                currency VARCHAR(10) NOT NULL,
                status VARCHAR(50) NOT NULL,
                applied_to_balance BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (provider, refund_id)
            );
            "#,
            r#"
            DO $$
            BEGIN
                IF EXISTS (
                    SELECT 1 FROM information_schema.table_constraints tc
                    JOIN information_schema.key_column_usage kcu 
                      ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema
                    WHERE tc.table_name = 'payment_refunds' 
                      AND tc.constraint_type = 'PRIMARY KEY'
                    GROUP BY tc.constraint_name
                    HAVING COUNT(*) = 1 AND MAX(kcu.column_name) = 'refund_id'
                ) THEN
                    ALTER TABLE payment_refunds DROP CONSTRAINT payment_refunds_pkey;
                    ALTER TABLE payment_refunds ADD PRIMARY KEY (provider, refund_id);
                END IF;

                IF NOT EXISTS (
                    SELECT 1 FROM information_schema.columns 
                    WHERE table_name = 'payment_refunds' AND column_name = 'applied_to_balance'
                ) THEN
                    ALTER TABLE payment_refunds ADD COLUMN applied_to_balance BOOLEAN NOT NULL DEFAULT FALSE;
                END IF;
            END $$;
            "#,
            "ALTER TABLE payment_records ADD COLUMN IF NOT EXISTS refunds JSONB NOT NULL DEFAULT '[]'::jsonb;",
            "CREATE INDEX IF NOT EXISTS idx_webhook_inbox_provider_event ON webhook_inbox(provider, provider_event_id);",
            "CREATE INDEX IF NOT EXISTS idx_webhook_inbox_payment_id ON webhook_inbox(payment_id);",
            "CREATE INDEX IF NOT EXISTS idx_payment_records_provider ON payment_records(provider);",
            "CREATE INDEX IF NOT EXISTS idx_payment_refunds_payment_id ON payment_refunds(payment_id);",
            "CREATE INDEX IF NOT EXISTS idx_payment_refunds_provider_payment ON payment_refunds(provider, payment_id);",
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

        // 2A. If a refund ID is present, serialize concurrent webhook events for the same (provider, refund_id)
        // using a PostgreSQL transaction-scoped advisory lock BEFORE checking whether the refund row exists.
        // This eliminates the race condition where concurrent deliveries for different payment IDs attempt to
        // claim the same refund ID simultaneously before either transaction inserts the row.
        let mut existing_refund_row = None;
        if let Some(r_id) = refund_id {
            let clean_r_id = r_id.trim();
            if !clean_r_id.is_empty() {
                sqlx::query(
                    "SELECT pg_advisory_xact_lock(hashtext('refund:' || $1), hashtext($2));",
                )
                .bind(provider)
                .bind(clean_r_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

                let existing_refund = sqlx::query(
                    r#"
                    SELECT payment_id, amount_minor, currency, status, applied_to_balance
                    FROM payment_refunds
                    WHERE provider = $1 AND refund_id = $2
                    FOR UPDATE;
                    "#,
                )
                .bind(provider)
                .bind(clean_r_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

                if let Some(ref rf_row) = existing_refund {
                    let rf_pid: String = rf_row.get("payment_id");
                    let rf_amt: i64 = rf_row.get("amount_minor");
                    let rf_curr: String = rf_row.get("currency");

                    if rf_pid != payment_id {
                        let err_msg = format!(
                            "Refund '{}' payment ID mismatch: existing '{}', received '{}'",
                            clean_r_id, rf_pid, payment_id
                        );
                        let _ = tx.rollback().await;
                        return Err(WebhookStoreError::Mismatch(err_msg));
                    }

                    if rf_curr != currency {
                        let err_msg = format!(
                            "Refund '{}' currency mismatch: existing '{}', received '{}'",
                            clean_r_id, rf_curr, currency
                        );
                        let _ = tx.rollback().await;
                        return Err(WebhookStoreError::Mismatch(err_msg));
                    }

                    if let Some(ramt) = refund_amount_minor {
                        if (rf_amt as u64) != ramt {
                            let err_msg = format!(
                                "Refund '{}' amount mismatch: existing {} minor units, received {} minor units",
                                clean_r_id, rf_amt, ramt
                            );
                            let _ = tx.rollback().await;
                            return Err(WebhookStoreError::Mismatch(err_msg));
                        }
                    }
                }

                existing_refund_row = existing_refund;
            }
        }

        // 2B. Serialize concurrent webhook events for the same (provider, payment_id)
        // using a PostgreSQL transaction-scoped advisory lock.
        // This guarantees that when payment_records has no row yet, concurrent events
        // for the same payment are strictly serialized and cannot race to insert uncoordinated rows.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('payment:' || $1), hashtext($2));")
            .bind(provider)
            .bind(payment_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

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

        // 7. Write refund record into payment_refunds from authoritative state machine outcome
        if let Some(r_id) = refund_id {
            let clean_r_id = r_id.trim();
            if !clean_r_id.is_empty() {
                if let Some(rf_entity) = payment_entity
                    .refunds
                    .iter()
                    .find(|r| r.refund_id == clean_r_id)
                {
                    if existing_refund_row.is_some() {
                        let update_res = sqlx::query(
                            r#"
                            UPDATE payment_refunds
                            SET status = $1, applied_to_balance = $2, updated_at = NOW()
                            WHERE provider = $3 AND refund_id = $4 AND payment_id = $5 AND currency = $6;
                            "#,
                        )
                        .bind(&rf_entity.status)
                        .bind(rf_entity.applied_to_balance)
                        .bind(provider)
                        .bind(clean_r_id)
                        .bind(payment_id)
                        .bind(currency)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

                        if update_res.rows_affected() == 0 {
                            let _ = tx.rollback().await;
                            return Err(WebhookStoreError::Mismatch(format!(
                                "Failed to update refund '{}': identity mismatch against existing record",
                                clean_r_id
                            )));
                        }
                    } else {
                        let insert_res = sqlx::query(
                            r#"
                            INSERT INTO payment_refunds (
                                provider, refund_id, payment_id, amount_minor, currency, status, applied_to_balance, created_at, updated_at
                            )
                            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW());
                            "#,
                        )
                        .bind(provider)
                        .bind(clean_r_id)
                        .bind(payment_id)
                        .bind(rf_entity.amount_minor as i64)
                        .bind(&rf_entity.currency)
                        .bind(&rf_entity.status)
                        .bind(rf_entity.applied_to_balance)
                        .execute(&mut *tx)
                        .await;

                        if let Err(e) = insert_res {
                            let _ = tx.rollback().await;
                            if let Some(db_err) = e.as_database_error() {
                                if db_err.code().as_deref() == Some("23505") {
                                    return Err(WebhookStoreError::Mismatch(format!(
                                        "Refund '{}:{}' conflict: already claimed by another record",
                                        provider, clean_r_id
                                    )));
                                }
                            }
                            return Err(WebhookStoreError::Database(e.to_string()));
                        }
                    }
                }
            }
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

    async fn get_refund_record(
        &self,
        provider: &str,
        refund_id: &str,
    ) -> Result<Option<RefundRecord>, WebhookStoreError> {
        let row = sqlx::query(
            r#"
            SELECT provider, refund_id, payment_id, amount_minor, currency, status, applied_to_balance
            FROM payment_refunds
            WHERE provider = $1 AND refund_id = $2;
            "#,
        )
        .bind(provider)
        .bind(refund_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| WebhookStoreError::Database(e.to_string()))?;

        if let Some(r) = row {
            let amt: i64 = r.get("amount_minor");
            Ok(Some(RefundRecord {
                provider: r.get("provider"),
                refund_id: r.get("refund_id"),
                payment_id: r.get("payment_id"),
                amount_minor: amt as u64,
                currency: r.get("currency"),
                status: r.get("status"),
                applied_to_balance: r.get("applied_to_balance"),
            }))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone)]
pub struct InMemoryRefundEntry {
    pub provider: String,
    pub refund_id: String,
    pub payment_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub status: String,
    pub applied_to_balance: bool,
}

/// Thread-safe in-memory implementation of WebhookStore for unit testing.
/// Replicates exact same unique constraint, concurrency coordination,
/// row locking, and rollback semantics without needing PostgreSQL running.
#[derive(Clone, Default)]
pub struct InMemoryWebhookStore {
    inbox: Arc<parking_lot::Mutex<HashMap<(String, String), WebhookInboxRecord>>>,
    payments: Arc<parking_lot::Mutex<HashMap<String, PaymentEntity>>>,
    refunds: Arc<parking_lot::Mutex<HashMap<(String, String), InMemoryRefundEntry>>>,
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

        // Check refund ID consistency across store scoped by (provider, refund_id)
        let mut created_refund_reservation = false;
        if let Some(r_id) = refund_id {
            let clean_r_id = r_id.trim();
            if !clean_r_id.is_empty() {
                let refund_key = (provider.to_string(), clean_r_id.to_string());
                let mut refunds_guard = self.refunds.lock();
                if let Some(entry) = refunds_guard.get(&refund_key) {
                    if entry.payment_id != payment_id {
                        let err_msg = format!(
                            "Refund '{}' payment ID mismatch: existing '{}', received '{}'",
                            clean_r_id, entry.payment_id, payment_id
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
                            clean_r_id, entry.currency, currency
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
                                clean_r_id, entry.amount_minor, ramt
                            );
                            let mut inbox_guard = self.inbox.lock();
                            if let Some(record) = inbox_guard.get_mut(&key) {
                                record.status = WebhookStatus::Failed;
                                record.error_message = Some(err_msg.clone());
                            }
                            return Err(WebhookStoreError::Mismatch(err_msg));
                        }
                    }
                } else if event_type != PaymentEventType::RefundSpeedChanged {
                    // Atomically reserve the (provider, refund_id) to serialize concurrent requests for different payments
                    refunds_guard.insert(
                        refund_key.clone(),
                        InMemoryRefundEntry {
                            provider: provider.to_string(),
                            refund_id: clean_r_id.to_string(),
                            payment_id: payment_id.to_string(),
                            amount_minor: refund_amount_minor.unwrap_or(amount_minor),
                            currency: currency.to_string(),
                            status: "pending".to_string(),
                            applied_to_balance: false,
                        },
                    );
                    created_refund_reservation = true;
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
                if created_refund_reservation {
                    if let Some(r_id) = refund_id {
                        let clean_r_id = r_id.trim();
                        let refund_key = (provider.to_string(), clean_r_id.to_string());
                        let mut refunds_guard = self.refunds.lock();
                        refunds_guard.remove(&refund_key);
                    }
                }
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

        // Update refund tracker from authoritative payment_entity
        if let Some(r_id) = refund_id {
            let clean_r_id = r_id.trim();
            if !clean_r_id.is_empty() {
                let refund_key = (provider.to_string(), clean_r_id.to_string());
                let mut refunds_guard = self.refunds.lock();
                if let Some(rf_entity) = payment_entity
                    .refunds
                    .iter()
                    .find(|r| r.refund_id == clean_r_id)
                {
                    refunds_guard.insert(
                        refund_key,
                        InMemoryRefundEntry {
                            provider: provider.to_string(),
                            refund_id: clean_r_id.to_string(),
                            payment_id: payment_id.to_string(),
                            amount_minor: rf_entity.amount_minor,
                            currency: rf_entity.currency.clone(),
                            status: rf_entity.status.clone(),
                            applied_to_balance: rf_entity.applied_to_balance,
                        },
                    );
                } else if created_refund_reservation {
                    refunds_guard.remove(&refund_key);
                }
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

    async fn get_refund_record(
        &self,
        provider: &str,
        refund_id: &str,
    ) -> Result<Option<RefundRecord>, WebhookStoreError> {
        let refunds_guard = self.refunds.lock();
        Ok(refunds_guard
            .get(&(provider.to_string(), refund_id.to_string()))
            .map(|e| RefundRecord {
                provider: e.provider.clone(),
                refund_id: e.refund_id.clone(),
                payment_id: e.payment_id.clone(),
                amount_minor: e.amount_minor,
                currency: e.currency.clone(),
                status: e.status.clone(),
                applied_to_balance: e.applied_to_balance,
            }))
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

    async fn get_test_postgres_pool() -> Option<PgPool> {
        let db_url = match std::env::var("DATABASE_URL") {
            Ok(url) if !url.is_empty() && url != "memory" => url,
            _ => return None,
        };
        let pool = PgPool::connect(&db_url)
            .await
            .expect("DATABASE_URL was specified but connecting to PostgreSQL failed");
        PostgresWebhookStore::init_schema(&pool)
            .await
            .expect("Schema init must succeed");
        Some(pool)
    }

    #[tokio::test]
    async fn test_in_memory_refund_provider_scoping_and_collision() {
        let store = InMemoryWebhookStore::new();
        let payload = serde_json::json!({"test": true});

        // Capture payment 1 on razorpay
        let p1 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rzp_pay_1",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_rzp_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&p1).await.unwrap();

        // Capture payment 2 on stripe
        let p2 = WebhookEventParams {
            provider: "stripe",
            provider_event_id: "evt_str_pay_1",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_str_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&p2).await.unwrap();

        // Refund under razorpay with shared refund ID "rfnd_shared_001"
        let r1 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rzp_ref_1",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_rzp_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_shared_001"),
            refund_amount_minor: Some(2000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&r1).await.unwrap();

        // Refund under stripe with SAME refund ID "rfnd_shared_001"
        let r2 = WebhookEventParams {
            provider: "stripe",
            provider_event_id: "evt_str_ref_1",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_str_1",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_shared_001"),
            refund_amount_minor: Some(3000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&r2).await.unwrap();

        // Assert both refunds exist independently without collision
        let ref_rzp = store
            .get_refund_record("razorpay", "rfnd_shared_001")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ref_rzp.payment_id, "pay_rzp_1");
        assert_eq!(ref_rzp.amount_minor, 2000);
        assert_eq!(ref_rzp.status, "processed");
        assert!(ref_rzp.applied_to_balance);

        let ref_str = store
            .get_refund_record("stripe", "rfnd_shared_001")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ref_str.payment_id, "pay_str_1");
        assert_eq!(ref_str.amount_minor, 3000);
        assert_eq!(ref_str.status, "processed");
        assert!(ref_str.applied_to_balance);

        // Attempting to claim the razorpay refund ID with a different payment ID must fail
        let r_conflict = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_rzp_ref_conflict",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_str_1", // Different payment!
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_shared_001"),
            refund_amount_minor: Some(2000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_conflict = store.process_webhook_event(&r_conflict).await;
        assert!(matches!(res_conflict, Err(WebhookStoreError::Mismatch(_))));
    }

    #[tokio::test]
    async fn test_in_memory_refund_lifecycle_failed_then_stale_pending_then_processed() {
        let store = InMemoryWebhookStore::new();
        let payload = serde_json::json!({"test": true});

        // 1. Initial capture
        let p_cap = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_life_cap",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_life_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&p_cap).await.unwrap();

        // 2. Refund fails on gateway
        let p_fail = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_life_fail",
            event_type: PaymentEventType::RefundFailed,
            payment_id: "pay_life_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_life_01"),
            refund_amount_minor: Some(2500),
            refund_status: Some("failed"),
            payload: &payload,
        };
        store.process_webhook_event(&p_fail).await.unwrap();

        let pay = store
            .get_payment_record("pay_life_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pay.refunded_amount_minor, 0);
        assert_eq!(pay.state, PaymentState::Captured);
        let rf = store
            .get_refund_record("razorpay", "rfnd_life_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf.status, "failed");
        assert!(!rf.applied_to_balance);

        // 3. Stale pending event arrives late -> ignored, must NOT downgrade failed to pending
        let p_stale_pending = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_life_pending",
            event_type: PaymentEventType::RefundCreated,
            payment_id: "pay_life_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_life_01"),
            refund_amount_minor: Some(2500),
            refund_status: Some("pending"),
            payload: &payload,
        };
        let res_pending = store.process_webhook_event(&p_stale_pending).await.unwrap();
        match res_pending {
            WebhookProcessingResult::Applied { transition, .. } => {
                assert!(matches!(
                    transition,
                    TransitionOutcome::LateEventIgnored { .. }
                ));
            }
            _ => panic!("Expected Applied with LateEventIgnored"),
        }

        let rf_after_stale = store
            .get_refund_record("razorpay", "rfnd_life_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            rf_after_stale.status, "failed",
            "Stale pending event must not downgrade failed refund"
        );
        assert!(!rf_after_stale.applied_to_balance);

        // 4. Reprocessing succeeds: processed event arrives
        let p_proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_life_proc",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_life_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_life_01"),
            refund_amount_minor: Some(2500),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&p_proc).await.unwrap();

        let pay_proc = store
            .get_payment_record("pay_life_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pay_proc.refunded_amount_minor, 2500);
        assert_eq!(pay_proc.state, PaymentState::PartiallyRefunded);
        let rf_proc = store
            .get_refund_record("razorpay", "rfnd_life_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf_proc.status, "processed");
        assert!(rf_proc.applied_to_balance);
    }

    #[tokio::test]
    async fn test_in_memory_refund_speed_changed_lifecycle() {
        let store = InMemoryWebhookStore::new();
        let payload = serde_json::json!({"test": true});

        let p_cap = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_spd_cap",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_spd_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&p_cap).await.unwrap();

        // 1. refund.speed_changed arrives before processed (metadata-only, raw status might say processed)
        let p_spd_1 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_spd_1",
            event_type: PaymentEventType::RefundSpeedChanged,
            payment_id: "pay_spd_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_spd_01"),
            refund_amount_minor: Some(3000),
            refund_status: Some("processed"), // Raw status from payload
            payload: &payload,
        };
        store.process_webhook_event(&p_spd_1).await.unwrap();

        let pay_1 = store
            .get_payment_record("pay_spd_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            pay_1.refunded_amount_minor, 0,
            "speed_changed must not modify monetary balance"
        );
        assert_eq!(pay_1.state, PaymentState::Captured);
        assert!(
            pay_1.refunds.is_empty(),
            "speed_changed for an unknown refund must NOT create a refund entity"
        );
        let rf_1 = store
            .get_refund_record("razorpay", "rfnd_spd_01")
            .await
            .unwrap();
        assert!(
            rf_1.is_none(),
            "speed_changed for an unknown refund must NOT create a refund record"
        );

        // 2. refund.processed arrives
        let p_proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_spd_proc",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_spd_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_spd_01"),
            refund_amount_minor: Some(3000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&p_proc).await.unwrap();

        let pay_2 = store
            .get_payment_record("pay_spd_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pay_2.refunded_amount_minor, 3000);
        assert_eq!(pay_2.state, PaymentState::PartiallyRefunded);
        let rf_2 = store
            .get_refund_record("razorpay", "rfnd_spd_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf_2.status, "processed");
        assert!(rf_2.applied_to_balance);

        // 3. refund.speed_changed arrives after processed
        let p_spd_2 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_spd_2",
            event_type: PaymentEventType::RefundSpeedChanged,
            payment_id: "pay_spd_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_spd_01"),
            refund_amount_minor: Some(3000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&p_spd_2).await.unwrap();

        let pay_3 = store
            .get_payment_record("pay_spd_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            pay_3.refunded_amount_minor, 3000,
            "Subsequent speed_changed must not double-apply money"
        );
        let rf_3 = store
            .get_refund_record("razorpay", "rfnd_spd_01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf_3.status, "processed");
        assert!(rf_3.applied_to_balance);
    }

    #[tokio::test]
    async fn test_in_memory_refund_conflicting_details_rejection() {
        let store = InMemoryWebhookStore::new();
        let payload = serde_json::json!({"test": true});

        let p_cap = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_conf_cap",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_conf_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&p_cap).await.unwrap();

        let p_ref = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_conf_ref_1",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_conf_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_conf_01"),
            refund_amount_minor: Some(2000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&p_ref).await.unwrap();

        // 1. Conflicting currency
        let p_bad_curr = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_conf_bad_curr",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_conf_01",
            amount_minor: 10000,
            currency: "USD", // Mismatch!
            refund_id: Some("rfnd_conf_01"),
            refund_amount_minor: Some(2000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        assert!(matches!(
            store.process_webhook_event(&p_bad_curr).await,
            Err(WebhookStoreError::Mismatch(_))
        ));

        // 2. Conflicting amount
        let p_bad_amt = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_conf_bad_amt",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_conf_01",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_conf_01"),
            refund_amount_minor: Some(4000), // Mismatch!
            refund_status: Some("processed"),
            payload: &payload,
        };
        assert!(matches!(
            store.process_webhook_event(&p_bad_amt).await,
            Err(WebhookStoreError::Mismatch(_))
        ));

        // 3. Conflicting payment ID
        let p_bad_pid = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_conf_bad_pid",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_other_99", // Mismatch!
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_conf_01"),
            refund_amount_minor: Some(2000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        assert!(matches!(
            store.process_webhook_event(&p_bad_pid).await,
            Err(WebhookStoreError::Mismatch(_))
        ));
    }

    #[tokio::test]
    async fn test_postgres_refund_concurrency_two_payments_same_refund_id() {
        let pool = match get_test_postgres_pool().await {
            Some(p) => p,
            None => return,
        };
        let store = Arc::new(PostgresWebhookStore::new(pool));
        let payload = serde_json::json!({"concurrency_test": true});

        let pay_a_id = format!("pay_conc_a_{}", uuid::Uuid::new_v4());
        let pay_b_id = format!("pay_conc_b_{}", uuid::Uuid::new_v4());
        let shared_refund_id = format!("rfnd_conc_shared_{}", uuid::Uuid::new_v4());

        // Capture payment A
        let cap_a = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_cap_a_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &pay_a_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&cap_a).await.unwrap();

        // Capture payment B
        let cap_b = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_cap_b_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &pay_b_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&cap_b).await.unwrap();

        // Concurrently attempt to process the exact same refund ID for pay_a and pay_b
        let store_a = Arc::clone(&store);
        let pid_a = pay_a_id.clone();
        let r_id_a = shared_refund_id.clone();
        let evt_a = format!("evt_ref_a_{}", uuid::Uuid::new_v4());
        let p_a = payload.clone();
        let handle_a = tokio::spawn(async move {
            let params = WebhookEventParams {
                provider: "razorpay",
                provider_event_id: &evt_a,
                event_type: PaymentEventType::RefundProcessed,
                payment_id: &pid_a,
                amount_minor: 10000,
                currency: "INR",
                refund_id: Some(&r_id_a),
                refund_amount_minor: Some(3000),
                refund_status: Some("processed"),
                payload: &p_a,
            };
            store_a.process_webhook_event(&params).await
        });

        let store_b = Arc::clone(&store);
        let pid_b = pay_b_id.clone();
        let r_id_b = shared_refund_id.clone();
        let evt_b = format!("evt_ref_b_{}", uuid::Uuid::new_v4());
        let p_b = payload.clone();
        let handle_b = tokio::spawn(async move {
            let params = WebhookEventParams {
                provider: "razorpay",
                provider_event_id: &evt_b,
                event_type: PaymentEventType::RefundProcessed,
                payment_id: &pid_b,
                amount_minor: 10000,
                currency: "INR",
                refund_id: Some(&r_id_b),
                refund_amount_minor: Some(3000),
                refund_status: Some("processed"),
                payload: &p_b,
            };
            store_b.process_webhook_event(&params).await
        });

        let res_a = handle_a.await.unwrap();
        let res_b = handle_b.await.unwrap();

        // Exactly one transaction must win, and the other must be rejected with Mismatch!
        let (winner, loser) = match (&res_a, &res_b) {
            (Ok(_), Err(WebhookStoreError::Mismatch(_))) => (&pay_a_id, &pay_b_id),
            (Err(WebhookStoreError::Mismatch(_)), Ok(_)) => (&pay_b_id, &pay_a_id),
            _ => panic!(
                "Expected exactly one winner and one Mismatch error. Got res_a: {:?}, res_b: {:?}",
                res_a, res_b
            ),
        };

        // Verify winner's balance is updated
        let pay_winner = store.get_payment_record(winner).await.unwrap().unwrap();
        assert_eq!(pay_winner.refunded_amount_minor, 3000);
        assert_eq!(pay_winner.state, PaymentState::PartiallyRefunded);

        // Verify loser's balance was NOT modified
        let pay_loser = store.get_payment_record(loser).await.unwrap().unwrap();
        assert_eq!(
            pay_loser.refunded_amount_minor, 0,
            "Losing payment balance must not be modified"
        );
        assert_eq!(pay_loser.state, PaymentState::Captured);
        assert!(
            pay_loser.refunds.is_empty(),
            "Losing payment must have no refunds recorded"
        );

        // Verify database payment_refunds table has exactly 1 row pointing to winner
        let rf_db = store
            .get_refund_record("razorpay", &shared_refund_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf_db.payment_id, *winner);
        assert_eq!(rf_db.amount_minor, 3000);
        assert_eq!(rf_db.status, "processed");
        assert!(rf_db.applied_to_balance);
    }

    #[tokio::test]
    async fn test_postgres_refund_provider_scoping() {
        let pool = match get_test_postgres_pool().await {
            Some(p) => p,
            None => return,
        };
        let store = PostgresWebhookStore::new(pool);
        let payload = serde_json::json!({"scoping_test": true});

        let rzp_pay_id = format!("pay_rzp_sc_{}", uuid::Uuid::new_v4());
        let str_pay_id = format!("pay_str_sc_{}", uuid::Uuid::new_v4());
        let shared_rfnd_id = format!("rfnd_shared_sc_{}", uuid::Uuid::new_v4());

        // Capture payment on razorpay
        let cap_rzp = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_rzp_cap_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &rzp_pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&cap_rzp).await.unwrap();

        // Capture payment on stripe
        let cap_str = WebhookEventParams {
            provider: "stripe",
            provider_event_id: &format!("evt_str_cap_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &str_pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&cap_str).await.unwrap();

        // Refund under razorpay
        let ref_rzp = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_rzp_ref_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &rzp_pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&shared_rfnd_id),
            refund_amount_minor: Some(2500),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&ref_rzp).await.unwrap();

        // Refund under stripe with the exact same refund ID string
        let ref_str = WebhookEventParams {
            provider: "stripe",
            provider_event_id: &format!("evt_str_ref_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &str_pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&shared_rfnd_id),
            refund_amount_minor: Some(4000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&ref_str).await.unwrap();

        // Verify both exist independently in payment_refunds
        let rzp_rec = store
            .get_refund_record("razorpay", &shared_rfnd_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rzp_rec.payment_id, rzp_pay_id);
        assert_eq!(rzp_rec.amount_minor, 2500);

        let str_rec = store
            .get_refund_record("stripe", &shared_rfnd_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(str_rec.payment_id, str_pay_id);
        assert_eq!(str_rec.amount_minor, 4000);

        // Attempting to reuse razorpay's refund ID on a different payment must fail
        let ref_mismatch = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_rzp_mis_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &str_pay_id, // Different payment!
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&shared_rfnd_id),
            refund_amount_minor: Some(2500),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_mis = store.process_webhook_event(&ref_mismatch).await;
        assert!(matches!(res_mis, Err(WebhookStoreError::Mismatch(_))));
    }

    #[tokio::test]
    async fn test_postgres_refund_failed_then_stale_pending_and_reprocessed() {
        let pool = match get_test_postgres_pool().await {
            Some(p) => p,
            None => return,
        };
        let store = PostgresWebhookStore::new(pool);
        let payload = serde_json::json!({"fail_test": true});

        let pay_id = format!("pay_fail_test_{}", uuid::Uuid::new_v4());
        let rfnd_id = format!("rfnd_fail_test_{}", uuid::Uuid::new_v4());

        // Capture payment
        let cap = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_cap_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&cap).await.unwrap();

        // 1. Deliver refund.failed
        let ref_failed = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_fail_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundFailed,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(3000),
            refund_status: Some("failed"),
            payload: &payload,
        };
        store.process_webhook_event(&ref_failed).await.unwrap();

        let pay1 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(pay1.refunded_amount_minor, 0);
        assert_eq!(pay1.state, PaymentState::Captured);
        let rf1 = store
            .get_refund_record("razorpay", &rfnd_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf1.status, "failed");
        assert!(!rf1.applied_to_balance);

        // 2. Deliver stale refund.created (pending) -> ignored, must preserve failed status in database
        let ref_pending = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_pend_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundCreated,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(3000),
            refund_status: Some("pending"),
            payload: &payload,
        };
        let res_pending = store.process_webhook_event(&ref_pending).await.unwrap();
        match res_pending {
            WebhookProcessingResult::Applied { transition, .. } => {
                assert!(matches!(
                    transition,
                    TransitionOutcome::LateEventIgnored { .. }
                ));
            }
            _ => panic!("Expected Applied with LateEventIgnored"),
        }

        let rf2 = store
            .get_refund_record("razorpay", &rfnd_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            rf2.status, "failed",
            "Postgres: stale pending must not overwrite failed status"
        );
        assert!(!rf2.applied_to_balance);
        let pay2 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(pay2.refunded_amount_minor, 0);

        // 3. Deliver refund.processed (reprocessing succeeded)
        let ref_proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_proc_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(3000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&ref_proc).await.unwrap();

        let rf3 = store
            .get_refund_record("razorpay", &rfnd_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf3.status, "processed");
        assert!(rf3.applied_to_balance);
        let pay3 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(pay3.refunded_amount_minor, 3000);
        assert_eq!(pay3.state, PaymentState::PartiallyRefunded);
    }

    #[tokio::test]
    async fn test_postgres_refund_speed_changed_before_and_after_processed() {
        let pool = match get_test_postgres_pool().await {
            Some(p) => p,
            None => return,
        };
        let store = PostgresWebhookStore::new(pool);
        let payload = serde_json::json!({"speed_test": true});

        let pay_id = format!("pay_spd_test_{}", uuid::Uuid::new_v4());
        let rfnd_id = format!("rfnd_spd_test_{}", uuid::Uuid::new_v4());

        // Capture payment
        let cap = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_cap_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&cap).await.unwrap();

        // 1. refund.speed_changed arrives first
        let spd_1 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_spd_1_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundSpeedChanged,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(4000),
            refund_status: Some("processed"), // Raw status from payload
            payload: &payload,
        };
        store.process_webhook_event(&spd_1).await.unwrap();

        let pay1 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(
            pay1.refunded_amount_minor, 0,
            "Postgres: speed_changed must not modify balance"
        );
        assert!(
            pay1.refunds.is_empty(),
            "Postgres: speed_changed for unknown refund must not create refund in payment record"
        );
        let rf1 = store.get_refund_record("razorpay", &rfnd_id).await.unwrap();
        assert!(
            rf1.is_none(),
            "Postgres: speed_changed for unknown refund must not create a refund record"
        );

        // 2. refund.processed arrives
        let proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_proc_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(4000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&proc).await.unwrap();

        let pay2 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(pay2.refunded_amount_minor, 4000);
        let rf2 = store
            .get_refund_record("razorpay", &rfnd_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf2.status, "processed");
        assert!(rf2.applied_to_balance);

        // 3. refund.speed_changed arrives after processed
        let spd_2 = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_spd_2_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundSpeedChanged,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(4000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&spd_2).await.unwrap();

        let pay3 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(
            pay3.refunded_amount_minor, 4000,
            "Postgres: subsequent speed_changed must not double-apply balance"
        );
        let rf3 = store
            .get_refund_record("razorpay", &rfnd_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf3.status, "processed");
        assert!(rf3.applied_to_balance);
    }

    #[tokio::test]
    async fn test_postgres_refund_conflicts_and_transaction_rollback() {
        let pool = match get_test_postgres_pool().await {
            Some(p) => p,
            None => return,
        };
        let store = PostgresWebhookStore::new(pool);
        let payload = serde_json::json!({"rollback_test": true});

        let pay_a_id = format!("pay_rb_a_{}", uuid::Uuid::new_v4());
        let pay_b_id = format!("pay_rb_b_{}", uuid::Uuid::new_v4());
        let rfnd_id = format!("rfnd_rb_{}", uuid::Uuid::new_v4());

        // Capture payment A
        let cap_a = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_cap_a_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &pay_a_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&cap_a).await.unwrap();

        // Capture payment B
        let cap_b = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_cap_b_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &pay_b_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&cap_b).await.unwrap();

        // Process initial refund on Payment A
        let ref_a = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_ref_a_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &pay_a_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(2000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&ref_a).await.unwrap();

        // Test 1: Conflict on payment ID: deliver event for Payment B claiming the same refund ID
        let conflict_pid = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_conf_pid_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &pay_b_id, // Conflict!
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(2000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_pid = store.process_webhook_event(&conflict_pid).await;
        assert!(matches!(res_pid, Err(WebhookStoreError::Mismatch(_))));

        // Verify transaction rollback: Payment B was NOT partially updated
        let pay_b = store.get_payment_record(&pay_b_id).await.unwrap().unwrap();
        assert_eq!(pay_b.refunded_amount_minor, 0);
        assert_eq!(pay_b.state, PaymentState::Captured);
        assert!(pay_b.refunds.is_empty());

        // Test 2: Conflict on currency for existing refund
        let conflict_curr = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_conf_curr_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &pay_a_id,
            amount_minor: 10000,
            currency: "USD", // Mismatch!
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(2000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_curr = store.process_webhook_event(&conflict_curr).await;
        assert!(matches!(res_curr, Err(WebhookStoreError::Mismatch(_))));

        // Test 3: Conflict on amount for existing refund
        let conflict_amt = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_conf_amt_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &pay_a_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_id),
            refund_amount_minor: Some(5000), // Mismatch!
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res_amt = store.process_webhook_event(&conflict_amt).await;
        assert!(matches!(res_amt, Err(WebhookStoreError::Mismatch(_))));

        // Verify Payment A and refund ledger remain pristine
        let pay_a = store.get_payment_record(&pay_a_id).await.unwrap().unwrap();
        assert_eq!(pay_a.refunded_amount_minor, 2000);
        let rf_rec = store
            .get_refund_record("razorpay", &rfnd_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf_rec.amount_minor, 2000);
        assert_eq!(rf_rec.currency, "INR");
        assert_eq!(rf_rec.payment_id, pay_a_id);
    }

    #[tokio::test]
    async fn test_in_memory_contradictory_refund_event_status_rejected() {
        let store = InMemoryWebhookStore::new();
        let payload = serde_json::json!({"test": true});

        let p_cap = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_cap_contra",
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: "pay_contra_mem",
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&p_cap).await.unwrap();

        // 1. refund.processed with status failed -> rejected
        let p_proc_failed = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_proc_fail_1",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_contra_mem",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_contra_mem_1"),
            refund_amount_minor: Some(2500),
            refund_status: Some("failed"),
            payload: &payload,
        };
        let res1 = store.process_webhook_event(&p_proc_failed).await;
        assert!(matches!(
            res1,
            Err(WebhookStoreError::PaymentState(
                PaymentError::ContradictoryRefundStatus { .. }
            ))
        ));
        let pay1 = store
            .get_payment_record("pay_contra_mem")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pay1.refunded_amount_minor, 0);
        assert!(pay1.refunds.is_empty());
        assert!(store
            .get_refund_record("razorpay", "rfnd_contra_mem_1")
            .await
            .unwrap()
            .is_none());

        // 2. refund.failed with status processed -> rejected
        let p_fail_proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_fail_proc_1",
            event_type: PaymentEventType::RefundFailed,
            payment_id: "pay_contra_mem",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_contra_mem_2"),
            refund_amount_minor: Some(2500),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res2 = store.process_webhook_event(&p_fail_proc).await;
        assert!(matches!(
            res2,
            Err(WebhookStoreError::PaymentState(
                PaymentError::ContradictoryRefundStatus { .. }
            ))
        ));
        let pay2 = store
            .get_payment_record("pay_contra_mem")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pay2.refunded_amount_minor, 0);
        assert!(pay2.refunds.is_empty());
        assert!(store
            .get_refund_record("razorpay", "rfnd_contra_mem_2")
            .await
            .unwrap()
            .is_none());

        // 3. Valid refund.created with status "processed" (instant refund)
        let p_inst = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_inst_cre_1",
            event_type: PaymentEventType::RefundCreated,
            payment_id: "pay_contra_mem",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_contra_mem_3"),
            refund_amount_minor: Some(4000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&p_inst).await.unwrap();

        let pay3 = store
            .get_payment_record("pay_contra_mem")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pay3.refunded_amount_minor, 4000);
        assert_eq!(pay3.state, PaymentState::PartiallyRefunded);
        let rf3 = store
            .get_refund_record("razorpay", "rfnd_contra_mem_3")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf3.status, "processed");
        assert!(rf3.applied_to_balance);

        // 4. Subsequent refund.processed for the instant refund is ignored as duplicate without double counting
        let p_inst_proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_inst_proc_1",
            event_type: PaymentEventType::RefundProcessed,
            payment_id: "pay_contra_mem",
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some("rfnd_contra_mem_3"),
            refund_amount_minor: Some(4000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&p_inst_proc).await.unwrap();

        let pay4 = store
            .get_payment_record("pay_contra_mem")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            pay4.refunded_amount_minor, 4000,
            "Subsequent refund.processed must not double-apply balance"
        );
    }

    #[tokio::test]
    async fn test_postgres_contradictory_refund_event_status_rejected() {
        let pool = match get_test_postgres_pool().await {
            Some(p) => p,
            None => return,
        };
        let store = PostgresWebhookStore::new(pool);
        let pay_id = format!("pay_contra_{}", uuid::Uuid::new_v4());
        let rfnd_1 = format!("rfnd_contra_1_{}", uuid::Uuid::new_v4());
        let rfnd_2 = format!("rfnd_contra_2_{}", uuid::Uuid::new_v4());
        let rfnd_3 = format!("rfnd_inst_{}", uuid::Uuid::new_v4());
        let payload = serde_json::json!({"test": true});

        let cap = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_cap_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::PaymentCaptured,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        store.process_webhook_event(&cap).await.unwrap();

        // 1. refund.processed with status failed -> rejected
        let proc_failed = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_proc_fail_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_1),
            refund_amount_minor: Some(2500),
            refund_status: Some("failed"),
            payload: &payload,
        };
        let res1 = store.process_webhook_event(&proc_failed).await;
        assert!(matches!(
            res1,
            Err(WebhookStoreError::PaymentState(
                PaymentError::ContradictoryRefundStatus { .. }
            ))
        ));
        let pay1 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(pay1.refunded_amount_minor, 0);
        assert!(pay1.refunds.is_empty());
        assert!(store
            .get_refund_record("razorpay", &rfnd_1)
            .await
            .unwrap()
            .is_none());

        // 2. refund.failed with status processed -> rejected
        let fail_proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_fail_proc_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundFailed,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_2),
            refund_amount_minor: Some(2500),
            refund_status: Some("processed"),
            payload: &payload,
        };
        let res2 = store.process_webhook_event(&fail_proc).await;
        assert!(matches!(
            res2,
            Err(WebhookStoreError::PaymentState(
                PaymentError::ContradictoryRefundStatus { .. }
            ))
        ));
        let pay2 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(pay2.refunded_amount_minor, 0);
        assert!(pay2.refunds.is_empty());
        assert!(store
            .get_refund_record("razorpay", &rfnd_2)
            .await
            .unwrap()
            .is_none());

        // 3. Valid refund.created with status "processed" (instant refund)
        let inst = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_inst_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundCreated,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_3),
            refund_amount_minor: Some(4000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&inst).await.unwrap();

        let pay3 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(pay3.refunded_amount_minor, 4000);
        assert_eq!(pay3.state, PaymentState::PartiallyRefunded);
        let rf3 = store
            .get_refund_record("razorpay", &rfnd_3)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rf3.status, "processed");
        assert!(rf3.applied_to_balance);

        // 4. Subsequent refund.processed for the instant refund is ignored as duplicate without double counting
        let inst_proc = WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &format!("evt_inst_proc_{}", uuid::Uuid::new_v4()),
            event_type: PaymentEventType::RefundProcessed,
            payment_id: &pay_id,
            amount_minor: 10000,
            currency: "INR",
            refund_id: Some(&rfnd_3),
            refund_amount_minor: Some(4000),
            refund_status: Some("processed"),
            payload: &payload,
        };
        store.process_webhook_event(&inst_proc).await.unwrap();

        let pay4 = store.get_payment_record(&pay_id).await.unwrap().unwrap();
        assert_eq!(
            pay4.refunded_amount_minor, 4000,
            "Subsequent refund.processed must not double-apply balance"
        );
    }
}
