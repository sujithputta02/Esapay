use esa_core::{EsaResult, PaymentEvent, WorkloadEntity};
use serde_json;
use sqlx::{PgPool, Row};

/// Persistent state store using PostgreSQL

pub struct StateStore {
    pool: PgPool,
}

impl StateStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn init_schema(&self) -> EsaResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS workload_entities (
                workload_id VARCHAR(255) PRIMARY KEY,
                shard_id VARCHAR(255) NOT NULL,
                state JSONB NOT NULL,
                version BIGINT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE TABLE IF NOT EXISTS payment_events (
                event_id VARCHAR(255) PRIMARY KEY,
                event_type VARCHAR(100) NOT NULL,
                event_data JSONB NOT NULL,
                region VARCHAR(50) NOT NULL,
                timestamp TIMESTAMPTZ NOT NULL,
                processed BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE TABLE IF NOT EXISTS action_executions (
                execution_id VARCHAR(255) PRIMARY KEY,
                proposal_id VARCHAR(255) NOT NULL,
                action_type VARCHAR(100) NOT NULL,
                action_data JSONB NOT NULL,
                outcome VARCHAR(50),
                before_metrics JSONB NOT NULL,
                after_metrics JSONB,
                executed_at TIMESTAMPTZ NOT NULL,
                completed_at TIMESTAMPTZ,
                error_message TEXT
            );

            CREATE TABLE IF NOT EXISTS audit_events (
                event_id VARCHAR(255) PRIMARY KEY,
                actor VARCHAR(255) NOT NULL,
                action_type VARCHAR(100) NOT NULL,
                reason TEXT,
                evidence JSONB,
                policy_result JSONB,
                rollback_ref VARCHAR(255),
                timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE INDEX IF NOT EXISTS idx_payment_events_timestamp ON payment_events(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_payment_events_region ON payment_events(region);
            CREATE INDEX IF NOT EXISTS idx_action_executions_timestamp ON action_executions(executed_at DESC);
            CREATE INDEX IF NOT EXISTS idx_audit_events_timestamp ON audit_events(timestamp DESC);
            "#
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn save_workload(&self, workload: &WorkloadEntity) -> EsaResult<()> {
        let state_json = serde_json::to_value(workload)?;

        sqlx::query(
            r#"
            INSERT INTO workload_entities (workload_id, shard_id, state, version, updated_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (workload_id) 
            DO UPDATE SET 
                state = EXCLUDED.state,
                version = EXCLUDED.version,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&workload.workload_id)
        .bind(&workload.shard_id)
        .bind(&state_json)
        .bind(workload.version as i64)
        .bind(workload.updated_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn load_workload(&self, workload_id: &str) -> EsaResult<Option<WorkloadEntity>> {
        let row = sqlx::query(
            r#"
            SELECT state FROM workload_entities WHERE workload_id = $1
            "#,
        )
        .bind(workload_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let state_json: serde_json::Value = row.get("state");
            let workload: WorkloadEntity = serde_json::from_value(state_json)?;
            Ok(Some(workload))
        } else {
            Ok(None)
        }
    }

    pub async fn save_payment_event(&self, event: &PaymentEvent) -> EsaResult<()> {
        let event_json = serde_json::to_value(event)?;

        sqlx::query(
            r#"
            INSERT INTO payment_events (event_id, event_type, event_data, region, timestamp)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (event_id) DO NOTHING
            "#,
        )
        .bind(&event.event_id)
        .bind(format!("{:?}", event.event_type))
        .bind(&event_json)
        .bind(event.region.as_str())
        .bind(event.timestamp)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_recent_events(&self, limit: i64) -> EsaResult<Vec<PaymentEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT event_data FROM payment_events
            ORDER BY timestamp DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();
        for row in rows {
            let event_json: serde_json::Value = row.get("event_data");
            let event: PaymentEvent = serde_json::from_value(event_json)?;
            events.push(event);
        }

        Ok(events)
    }
}
