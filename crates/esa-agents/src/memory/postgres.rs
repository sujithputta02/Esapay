use crate::memory::store::PaymentMemoryStore;
use crate::memory::types::{
    MemoryQuery, MemoryRecord, MemoryScope, MemoryType, OutcomeAccuracy, OutcomeEvaluationRecord,
    VerificationStatus,
};
use crate::payment_context::CanonicalPaymentState;
use crate::provider::{ExecutionScope, InferenceError, InferenceErrorCategory, ProviderKind};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// PostgreSQL-backed `PaymentMemoryStore` using existing ESAPay database infrastructure.
pub struct PostgresPaymentMemoryStore {
    pool: PgPool,
}

impl PostgresPaymentMemoryStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PaymentMemoryStore for PostgresPaymentMemoryStore {
    async fn init_schema(&self) -> Result<(), InferenceError> {
        let stmts = [
            r#"
            CREATE TABLE IF NOT EXISTS esa_memory_records (
                memory_id VARCHAR(128) PRIMARY KEY,
                memory_type VARCHAR(32) NOT NULL,
                scope VARCHAR(32) NOT NULL,
                merchant_id VARCHAR(255),
                provider VARCHAR(64),
                incident_category VARCHAR(128) NOT NULL,
                lifecycle_state VARCHAR(64),
                title VARCHAR(255) NOT NULL,
                summary TEXT NOT NULL,
                resolution TEXT,
                source_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
                verification_status VARCHAR(32) NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                expires_at TIMESTAMPTZ
            );
            "#,
            "CREATE INDEX IF NOT EXISTS idx_esa_memory_scope_merchant ON esa_memory_records(scope, merchant_id);",
            "CREATE INDEX IF NOT EXISTS idx_esa_memory_type_provider ON esa_memory_records(memory_type, provider, incident_category);",
            "CREATE INDEX IF NOT EXISTS idx_esa_memory_lifecycle ON esa_memory_records(lifecycle_state, updated_at DESC);",
            "CREATE INDEX IF NOT EXISTS idx_esa_memory_expires ON esa_memory_records(expires_at);",
            r#"
            CREATE TABLE IF NOT EXISTS esa_experience_evaluations (
                evaluation_id VARCHAR(128) PRIMARY KEY,
                investigation_id VARCHAR(128) NOT NULL,
                merchant_id VARCHAR(255) NOT NULL,
                payment_reference VARCHAR(255) NOT NULL,
                provider VARCHAR(64) NOT NULL,
                recommended_root_cause VARCHAR(128) NOT NULL,
                inference_provider VARCHAR(64) NOT NULL,
                model_id VARCHAR(255) NOT NULL,
                deterministic_fallback_used BOOLEAN NOT NULL DEFAULT FALSE,
                observed_authoritative_state VARCHAR(64) NOT NULL,
                verified_outcome_state VARCHAR(64),
                accuracy VARCHAR(32) NOT NULL,
                promoted_memory_id VARCHAR(128),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                verified_at TIMESTAMPTZ,
                expires_at TIMESTAMPTZ
            );
            "#,
            "ALTER TABLE esa_experience_evaluations ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;",
            "CREATE INDEX IF NOT EXISTS idx_esa_eval_merchant ON esa_experience_evaluations(merchant_id, created_at DESC);",
            "CREATE INDEX IF NOT EXISTS idx_esa_eval_expires ON esa_experience_evaluations(expires_at);",
        ];

        for stmt in stmts {
            sqlx::query(stmt).execute(&self.pool).await.map_err(|e| {
                InferenceError::new(
                    InferenceErrorCategory::UpstreamError,
                    format!("Memory schema initialization error: {}", e),
                )
            })?;
        }
        Ok(())
    }

    async fn store_memory(&self, record: MemoryRecord) -> Result<MemoryRecord, InferenceError> {
        let v = record.sanitize_and_validate()?;
        let source_refs_json = serde_json::to_value(&v.source_refs).unwrap_or_default();

        let res = sqlx::query(
            r#"
            INSERT INTO esa_memory_records (
                memory_id, memory_type, scope, merchant_id, provider,
                incident_category, lifecycle_state, title, summary, resolution,
                source_refs, verification_status, version, created_at, updated_at, expires_at
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15, $16
            )
            ON CONFLICT (memory_id) DO UPDATE SET
                title = EXCLUDED.title,
                summary = EXCLUDED.summary,
                resolution = EXCLUDED.resolution,
                source_refs = EXCLUDED.source_refs,
                verification_status = EXCLUDED.verification_status,
                version = EXCLUDED.version,
                updated_at = EXCLUDED.updated_at,
                expires_at = EXCLUDED.expires_at
            WHERE esa_memory_records.scope = EXCLUDED.scope
              AND esa_memory_records.merchant_id IS NOT DISTINCT FROM EXCLUDED.merchant_id;
            "#,
        )
        .bind(&v.memory_id)
        .bind(v.memory_type.as_str())
        .bind(v.scope.as_str())
        .bind(&v.merchant_id)
        .bind(&v.provider)
        .bind(&v.incident_category)
        .bind(v.lifecycle_state.map(|s| s.as_str()))
        .bind(&v.title)
        .bind(&v.summary)
        .bind(&v.resolution)
        .bind(source_refs_json)
        .bind(v.verification_status.as_str())
        .bind(v.version as i32)
        .bind(v.created_at)
        .bind(v.updated_at)
        .bind(v.expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to persist memory record: {}", e),
            )
        })?;

        if res.rows_affected() == 0 {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Cannot overwrite memory record owned by another scope or merchant",
            ));
        }

        Ok(v)
    }

    async fn query_memories(
        &self,
        query: &MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, InferenceError> {
        let merchant_id = query.scope.normalized_merchant_id()?;
        let limit = query.limit.clamp(1, 50) as i64;
        let types: Vec<String> = query
            .memory_types
            .iter()
            .map(|t| t.as_str().to_string())
            .collect();

        let rows = sqlx::query(
            r#"
            SELECT memory_id, memory_type, scope, merchant_id, provider,
                   incident_category, lifecycle_state, title, summary, resolution,
                   source_refs, verification_status, version, created_at, updated_at, expires_at
            FROM esa_memory_records
            WHERE (expires_at IS NULL OR expires_at > NOW())
              AND (
                  scope = 'system'
                  OR ($1::text IS NOT NULL AND scope = 'merchant' AND merchant_id = $1)
              )
              AND ($2::text[] IS NULL OR cardinality($2::text[]) = 0 OR memory_type = ANY($2::text[]))
              AND ($3::text IS NULL OR provider IS NULL OR LOWER(provider) = LOWER($3))
              AND ($4::text IS NULL OR LOWER(incident_category) = LOWER($4))
              AND ($5::text IS NULL OR lifecycle_state IS NULL OR lifecycle_state = $5)
              AND ($6::timestamptz IS NULL OR updated_at >= $6)
            ORDER BY (CASE WHEN memory_type = 'episodic' THEN 0 ELSE 1 END), updated_at DESC
            LIMIT $7;
            "#,
        )
        .bind(merchant_id)
        .bind(if types.is_empty() { None } else { Some(types) })
        .bind(query.provider.as_deref())
        .bind(query.incident_category.as_deref())
        .bind(query.lifecycle_state.map(|s| s.as_str()))
        .bind(query.since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Memory retrieval query failed: {}", e),
            )
        })?;

        Ok(rows.into_iter().filter_map(row_to_memory_record).collect())
    }

    async fn delete_memory(
        &self,
        scope: &ExecutionScope,
        memory_id: &str,
    ) -> Result<bool, InferenceError> {
        let caller_mid = scope.normalized_merchant_id()?;
        let res = match caller_mid {
            Some(mid) => {
                sqlx::query(
                    "DELETE FROM esa_memory_records WHERE memory_id = $1 AND scope = 'merchant' AND merchant_id = $2;",
                )
                .bind(memory_id)
                .bind(mid)
                .execute(&self.pool)
                .await
            }
            None => {
                sqlx::query(
                    "DELETE FROM esa_memory_records WHERE memory_id = $1 AND scope = 'system';",
                )
                .bind(memory_id)
                .execute(&self.pool)
                .await
            }
        }
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to delete memory: {}", e),
            )
        })?;

        Ok(res.rows_affected() > 0)
    }

    async fn purge_expired(&self) -> Result<usize, InferenceError> {
        let mem_res = sqlx::query(
            "DELETE FROM esa_memory_records WHERE expires_at IS NOT NULL AND expires_at <= NOW();",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to purge expired memories: {}", e),
            )
        })?;

        let eval_res = sqlx::query(
            "DELETE FROM esa_experience_evaluations WHERE expires_at IS NOT NULL AND expires_at <= NOW();",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to purge expired evaluations: {}", e),
            )
        })?;

        Ok((mem_res.rows_affected() + eval_res.rows_affected()) as usize)
    }

    async fn record_evaluation(
        &self,
        eval: OutcomeEvaluationRecord,
    ) -> Result<OutcomeEvaluationRecord, InferenceError> {
        sqlx::query(
            r#"
            INSERT INTO esa_experience_evaluations (
                evaluation_id, investigation_id, merchant_id, payment_reference, provider,
                recommended_root_cause, inference_provider, model_id, deterministic_fallback_used,
                observed_authoritative_state, verified_outcome_state, accuracy,
                promoted_memory_id, created_at, verified_at, expires_at
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9,
                $10, $11, $12,
                $13, $14, $15, $16
            );
            "#,
        )
        .bind(&eval.evaluation_id)
        .bind(&eval.investigation_id)
        .bind(&eval.merchant_id)
        .bind(&eval.payment_reference)
        .bind(&eval.provider)
        .bind(&eval.recommended_root_cause)
        .bind(eval.inference_provider.as_str())
        .bind(&eval.model_id)
        .bind(eval.deterministic_fallback_used)
        .bind(eval.observed_authoritative_state.as_str())
        .bind(eval.verified_outcome_state.map(|s| s.as_str()))
        .bind(eval.accuracy.as_str())
        .bind(&eval.promoted_memory_id)
        .bind(eval.created_at)
        .bind(eval.verified_at)
        .bind(eval.expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to record experience evaluation: {}", e),
            )
        })?;
        Ok(eval)
    }

    async fn get_evaluation(
        &self,
        scope: &ExecutionScope,
        evaluation_id: &str,
    ) -> Result<Option<OutcomeEvaluationRecord>, InferenceError> {
        let caller_mid = scope.normalized_merchant_id()?.ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "System scope cannot implicitly read merchant-scoped evaluations",
            )
        })?;
        let row = sqlx::query(
            r#"
            SELECT evaluation_id, investigation_id, merchant_id, payment_reference, provider,
                   recommended_root_cause, inference_provider, model_id, deterministic_fallback_used,
                   observed_authoritative_state, verified_outcome_state, accuracy,
                   promoted_memory_id, created_at, verified_at, expires_at
            FROM esa_experience_evaluations
            WHERE evaluation_id = $1
              AND (expires_at IS NULL OR expires_at > NOW());
            "#,
        )
        .bind(evaluation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to query evaluation: {}", e),
            )
        })?;

        if let Some(r) = row {
            let ev = row_to_evaluation(r);
            if ev.merchant_id != caller_mid {
                return Err(InferenceError::new(
                    InferenceErrorCategory::Forbidden,
                    "Evaluation does not belong to authenticated merchant",
                ));
            }
            Ok(Some(ev))
        } else {
            Ok(None)
        }
    }

    async fn verify_and_promote_outcome(
        &self,
        scope: &ExecutionScope,
        evaluation_id: &str,
        verified_outcome_state: CanonicalPaymentState,
        accuracy: OutcomeAccuracy,
        verified_resolution: &str,
    ) -> Result<(OutcomeEvaluationRecord, Option<MemoryRecord>), InferenceError> {
        if accuracy == OutcomeAccuracy::Pending {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Cannot promote outcome with Pending accuracy",
            ));
        }
        let caller_mid = scope.normalized_merchant_id()?.ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "System scope cannot verify merchant-scoped evaluations",
            )
        })?;

        let will_promote = matches!(
            accuracy,
            OutcomeAccuracy::Correct | OutcomeAccuracy::PartiallyCorrect
        );
        if will_promote && verified_outcome_state == CanonicalPaymentState::Unknown {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Cannot promote outcome to verified episodic memory with Unknown authoritative state",
            ));
        }
        if will_promote && verified_resolution.trim().is_empty() {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Cannot promote outcome to verified episodic memory without a verified resolution",
            ));
        }

        let mut tx = self.pool.begin().await.map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to begin verification transaction: {}", e),
            )
        })?;

        let row = sqlx::query(
            r#"
            SELECT evaluation_id, investigation_id, merchant_id, payment_reference, provider,
                   recommended_root_cause, inference_provider, model_id, deterministic_fallback_used,
                   observed_authoritative_state, verified_outcome_state, accuracy,
                   promoted_memory_id, created_at, verified_at, expires_at
            FROM esa_experience_evaluations
            WHERE evaluation_id = $1
            FOR UPDATE;
            "#,
        )
        .bind(evaluation_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to lock evaluation row: {}", e),
            )
        })?
        .ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::NotFound,
                "Evaluation record not found",
            )
        })?;

        let mut eval = row_to_evaluation(row);
        if eval.merchant_id != caller_mid {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Evaluation does not belong to authenticated merchant",
            ));
        }

        // Idempotent replay handling
        if eval.verified_at.is_some() {
            if eval.verified_outcome_state == Some(verified_outcome_state)
                && eval.accuracy == accuracy
            {
                let existing_mem = if let Some(ref mem_id) = eval.promoted_memory_id {
                    let mrow = sqlx::query(
                        r#"
                        SELECT memory_id, memory_type, scope, merchant_id, provider,
                               incident_category, lifecycle_state, title, summary, resolution,
                               source_refs, verification_status, version, created_at, updated_at, expires_at
                        FROM esa_memory_records
                        WHERE memory_id = $1 AND scope = 'merchant' AND merchant_id = $2;
                        "#,
                    )
                    .bind(mem_id)
                    .bind(&eval.merchant_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| {
                        InferenceError::new(
                            InferenceErrorCategory::UpstreamError,
                            format!("Failed to load promoted memory: {}", e),
                        )
                    })?;
                    mrow.and_then(row_to_memory_record)
                } else {
                    None
                };
                tx.commit().await.map_err(|e| {
                    InferenceError::new(
                        InferenceErrorCategory::UpstreamError,
                        format!("Failed to commit idempotent verification read: {}", e),
                    )
                })?;
                return Ok((eval, existing_mem));
            }
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Evaluation was already verified with a different outcome",
            ));
        }

        let now = Utc::now();
        let promoted = if will_promote {
            let mem_id = format!("mem_ep_{}", Uuid::new_v4().simple());
            let record = MemoryRecord {
                memory_id: mem_id.clone(),
                memory_type: MemoryType::Episodic,
                scope: MemoryScope::Merchant,
                merchant_id: Some(eval.merchant_id.clone()),
                provider: Some(eval.provider.clone()),
                incident_category: eval.recommended_root_cause.clone(),
                lifecycle_state: Some(verified_outcome_state),
                title: format!(
                    "Verified {} incident ({})",
                    eval.provider, eval.recommended_root_cause
                ),
                summary: format!(
                    "Verified outcome '{}' for {} payment {} (accuracy: {}, model: {})",
                    verified_outcome_state.as_str(),
                    eval.provider,
                    eval.payment_reference,
                    accuracy.as_str(),
                    eval.model_id
                ),
                resolution: Some(verified_resolution.to_string()),
                source_refs: vec![
                    format!("payment:{}", eval.payment_reference),
                    format!("evaluation:{}", eval.evaluation_id),
                ],
                verification_status: VerificationStatus::Verified,
                version: 1,
                created_at: now,
                updated_at: now,
                expires_at: Some(now + chrono::Duration::days(180)),
            }
            .sanitize_and_validate()?;

            let source_refs_json = serde_json::to_value(&record.source_refs).unwrap_or_default();
            sqlx::query(
                r#"
                INSERT INTO esa_memory_records (
                    memory_id, memory_type, scope, merchant_id, provider,
                    incident_category, lifecycle_state, title, summary, resolution,
                    source_refs, verification_status, version, created_at, updated_at, expires_at
                ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8, $9, $10,
                    $11, $12, $13, $14, $15, $16
                );
                "#,
            )
            .bind(&record.memory_id)
            .bind(record.memory_type.as_str())
            .bind(record.scope.as_str())
            .bind(&record.merchant_id)
            .bind(&record.provider)
            .bind(&record.incident_category)
            .bind(record.lifecycle_state.map(|s| s.as_str()))
            .bind(&record.title)
            .bind(&record.summary)
            .bind(&record.resolution)
            .bind(source_refs_json)
            .bind(record.verification_status.as_str())
            .bind(record.version as i32)
            .bind(record.created_at)
            .bind(record.updated_at)
            .bind(record.expires_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                InferenceError::new(
                    InferenceErrorCategory::UpstreamError,
                    format!("Failed to persist promoted episodic memory: {}", e),
                )
            })?;

            eval.promoted_memory_id = Some(mem_id);
            Some(record)
        } else {
            None
        };

        eval.verified_outcome_state = Some(verified_outcome_state);
        eval.accuracy = accuracy;
        eval.verified_at = Some(now);

        sqlx::query(
            r#"
            UPDATE esa_experience_evaluations
            SET verified_outcome_state = $2,
                accuracy = $3,
                promoted_memory_id = $4,
                verified_at = $5
            WHERE evaluation_id = $1;
            "#,
        )
        .bind(&eval.evaluation_id)
        .bind(verified_outcome_state.as_str())
        .bind(accuracy.as_str())
        .bind(&eval.promoted_memory_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to update evaluation record: {}", e),
            )
        })?;

        tx.commit().await.map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to commit outcome verification transaction: {}", e),
            )
        })?;

        Ok((eval, promoted))
    }

    async fn delete_evaluations_for_merchant(
        &self,
        scope: &ExecutionScope,
    ) -> Result<usize, InferenceError> {
        let caller_mid = scope.normalized_merchant_id()?.ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "System scope cannot bulk-delete merchant evaluations",
            )
        })?;
        let res = sqlx::query("DELETE FROM esa_experience_evaluations WHERE merchant_id = $1;")
            .bind(caller_mid)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                InferenceError::new(
                    InferenceErrorCategory::UpstreamError,
                    format!("Failed to delete merchant evaluations: {}", e),
                )
            })?;
        Ok(res.rows_affected() as usize)
    }
}

fn parse_canonical_state(raw: &str) -> CanonicalPaymentState {
    match raw.trim().to_ascii_lowercase().as_str() {
        "created" => CanonicalPaymentState::Created,
        "authorized" => CanonicalPaymentState::Authorized,
        "captured" => CanonicalPaymentState::Captured,
        "failed" => CanonicalPaymentState::Failed,
        "pending" => CanonicalPaymentState::Pending,
        "pending_reconciliation" => CanonicalPaymentState::PendingReconciliation,
        "partially_refunded" => CanonicalPaymentState::PartiallyRefunded,
        "refunded" => CanonicalPaymentState::Refunded,
        _ => CanonicalPaymentState::Unknown,
    }
}

fn row_to_memory_record(r: sqlx::postgres::PgRow) -> Option<MemoryRecord> {
    let mem_type = MemoryType::parse(r.get::<String, _>("memory_type").as_str())?;
    let scope = match r.get::<String, _>("scope").as_str() {
        "system" => MemoryScope::System,
        _ => MemoryScope::Merchant,
    };
    let ver = VerificationStatus::parse(r.get::<String, _>("verification_status").as_str())
        .unwrap_or(VerificationStatus::Unverified);
    let lifecycle_opt: Option<String> = r.get("lifecycle_state");
    let source_refs_val: serde_json::Value = r.get("source_refs");
    let source_refs = serde_json::from_value(source_refs_val).unwrap_or_default();
    let version: i32 = r.get("version");

    Some(MemoryRecord {
        memory_id: r.get("memory_id"),
        memory_type: mem_type,
        scope,
        merchant_id: r.get("merchant_id"),
        provider: r.get("provider"),
        incident_category: r.get("incident_category"),
        lifecycle_state: lifecycle_opt.as_deref().map(parse_canonical_state),
        title: r.get("title"),
        summary: r.get("summary"),
        resolution: r.get("resolution"),
        source_refs,
        verification_status: ver,
        version: version.max(1) as u32,
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        expires_at: r.get("expires_at"),
    })
}

fn row_to_evaluation(r: sqlx::postgres::PgRow) -> OutcomeEvaluationRecord {
    let inf_prov = match r.get::<String, _>("inference_provider").as_str() {
        "hf_zerogpu" => ProviderKind::HfZerogpu,
        "deterministic_fallback" => ProviderKind::DeterministicFallback,
        _ => ProviderKind::Ollama,
    };
    let verified_opt: Option<String> = r.get("verified_outcome_state");
    let acc_str: String = r.get("accuracy");
    let created_at: DateTime<Utc> = r.get("created_at");
    let verified_at: Option<DateTime<Utc>> = r.get("verified_at");
    let expires_at: Option<DateTime<Utc>> = r.get("expires_at");

    OutcomeEvaluationRecord {
        evaluation_id: r.get("evaluation_id"),
        investigation_id: r.get("investigation_id"),
        merchant_id: r.get("merchant_id"),
        payment_reference: r.get("payment_reference"),
        provider: r.get("provider"),
        recommended_root_cause: r.get("recommended_root_cause"),
        inference_provider: inf_prov,
        model_id: r.get("model_id"),
        deterministic_fallback_used: r.get("deterministic_fallback_used"),
        observed_authoritative_state: parse_canonical_state(
            r.get::<String, _>("observed_authoritative_state").as_str(),
        ),
        verified_outcome_state: verified_opt.as_deref().map(parse_canonical_state),
        accuracy: OutcomeAccuracy::parse(&acc_str).unwrap_or(OutcomeAccuracy::Pending),
        promoted_memory_id: r.get("promoted_memory_id"),
        created_at,
        verified_at,
        expires_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::seed_system_knowledge;
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    async fn test_postgres_memory_tenant_isolation_conflicts_and_atomic_promotion() {
        let db_url = match std::env::var("DATABASE_URL") {
            Ok(v) if !v.trim().is_empty() => v,
            _ => return, // PostgreSQL integration tests require DATABASE_URL (enabled in CI)
        };

        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&db_url)
            .await
            .expect("DATABASE_URL was set but connecting to PostgreSQL failed");

        let store = PostgresPaymentMemoryStore::new(pool.clone());
        store.init_schema().await.unwrap();
        seed_system_knowledge(&store).await.unwrap();

        let suffix = Uuid::new_v4().simple().to_string();
        let m_a = format!("pg_merchant_a_{}", &suffix[..8]);
        let m_b = format!("pg_merchant_b_{}", &suffix[..8]);
        let scope_a = ExecutionScope::MerchantScoped {
            merchant_id: m_a.clone(),
        };
        let scope_b = ExecutionScope::MerchantScoped {
            merchant_id: m_b.clone(),
        };
        let shared_mem_id = format!("pg_shared_{}", &suffix[..8]);

        let now = Utc::now();
        let rec_a = MemoryRecord {
            memory_id: shared_mem_id.clone(),
            memory_type: MemoryType::Episodic,
            scope: MemoryScope::Merchant,
            merchant_id: Some(m_a.clone()),
            provider: Some("razorpay".to_string()),
            incident_category: "AUTHORIZATION_NOT_CAPTURED".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Authorized),
            title: "PG Merchant A incident".to_string(),
            summary: "Authorized without capture".to_string(),
            resolution: Some("Verified capture".to_string()),
            source_refs: vec!["payment:pay_pg_a".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: Some(now + chrono::Duration::days(30)),
        };
        store.store_memory(rec_a).await.unwrap();

        // 1. Cross-merchant overwrite with same memory_id must fail with Forbidden
        let rec_b_conflict = MemoryRecord {
            memory_id: shared_mem_id.clone(),
            memory_type: MemoryType::Episodic,
            scope: MemoryScope::Merchant,
            merchant_id: Some(m_b.clone()),
            provider: Some("razorpay".to_string()),
            incident_category: "PAYMENT_FAILED_AT_PROVIDER".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Failed),
            title: "PG Merchant B overwrite attempt".to_string(),
            summary: "Attempted overwrite".to_string(),
            resolution: None,
            source_refs: vec!["payment:pay_pg_b".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: Some(now + chrono::Duration::days(30)),
        };
        let conflict_err = store.store_memory(rec_b_conflict).await.unwrap_err();
        assert_eq!(conflict_err.category, InferenceErrorCategory::Forbidden);

        // 2. Tenant-isolated retrieval: Merchant B cannot read Merchant A's episodic memory
        let res_b = store
            .query_memories(&MemoryQuery {
                scope: scope_b.clone(),
                memory_types: vec![MemoryType::Episodic],
                provider: None,
                incident_category: None,
                lifecycle_state: None,
                since: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert!(!res_b.iter().any(|r| r.memory_id == shared_mem_id));

        // 3. Evaluation ownership & System-scope access denial
        let eval = OutcomeEvaluationRecord::new_pending(
            format!("inv_pg_{}", &suffix[..8]),
            m_a.clone(),
            "pay_pg_a".to_string(),
            "razorpay".to_string(),
            "AUTHORIZATION_NOT_CAPTURED".to_string(),
            ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            true,
            CanonicalPaymentState::Authorized,
        );
        let eval_id = eval.evaluation_id.clone();
        store.record_evaluation(eval).await.unwrap();

        assert_eq!(
            store
                .get_evaluation(&scope_b, &eval_id)
                .await
                .unwrap_err()
                .category,
            InferenceErrorCategory::Forbidden
        );
        assert_eq!(
            store
                .get_evaluation(&ExecutionScope::System, &eval_id)
                .await
                .unwrap_err()
                .category,
            InferenceErrorCategory::Forbidden
        );

        // 4. Atomic verified promotion & idempotency
        let (ev1, mem1) = store
            .verify_and_promote_outcome(
                &scope_a,
                &eval_id,
                CanonicalPaymentState::Captured,
                OutcomeAccuracy::Correct,
                "Captured in gateway settlement",
            )
            .await
            .unwrap();
        let promoted_id = ev1.promoted_memory_id.clone().unwrap();
        assert_eq!(mem1.unwrap().memory_id, promoted_id);

        let (ev2, mem2) = store
            .verify_and_promote_outcome(
                &scope_a,
                &eval_id,
                CanonicalPaymentState::Captured,
                OutcomeAccuracy::Correct,
                "Captured in gateway settlement",
            )
            .await
            .unwrap();
        assert_eq!(
            ev2.promoted_memory_id.as_deref(),
            Some(promoted_id.as_str())
        );
        assert_eq!(mem2.unwrap().memory_id, promoted_id);

        // Cleanup test records
        let _ = store.delete_memory(&scope_a, &shared_mem_id).await;
        let _ = store.delete_memory(&scope_a, &promoted_id).await;
        let _ = store.delete_evaluations_for_merchant(&scope_a).await;
    }
}
