use crate::memory::store::PaymentMemoryStore;
use crate::memory::types::{
    AuthoritativeOutcomeProof, MemoryProvenance, MemoryQuery, MemoryRecord, MemoryScope,
    MemoryType, OutcomeAccuracy, OutcomeEvaluationRecord, ResolutionAuditEvent, ResolutionStatus,
    TrustedResolutionRecord, VerificationStatus,
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
                supporting_evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
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
            "ALTER TABLE esa_experience_evaluations ADD COLUMN IF NOT EXISTS supporting_evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb;",
            "ALTER TABLE esa_memory_records ADD COLUMN IF NOT EXISTS provenance JSONB;",
            "CREATE INDEX IF NOT EXISTS idx_esa_eval_merchant ON esa_experience_evaluations(merchant_id, created_at DESC);",
            "CREATE INDEX IF NOT EXISTS idx_esa_eval_expires ON esa_experience_evaluations(expires_at);",
            r#"
            CREATE TABLE IF NOT EXISTS esa_trusted_resolutions (
                resolution_id VARCHAR(128) PRIMARY KEY,
                investigation_id VARCHAR(128) NOT NULL,
                evaluation_id VARCHAR(128) NOT NULL UNIQUE,
                merchant_id VARCHAR(255) NOT NULL,
                payment_reference VARCHAR(255) NOT NULL,
                provider VARCHAR(64) NOT NULL,
                proposed_root_cause VARCHAR(128) NOT NULL,
                status VARCHAR(32) NOT NULL,
                accuracy VARCHAR(32) NOT NULL,
                authoritative_state VARCHAR(64),
                verification_method VARCHAR(64),
                evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
                evidence_provenance JSONB NOT NULL DEFAULT '[]'::jsonb,
                missing_evidence JSONB NOT NULL DEFAULT '[]'::jsonb,
                verification_summary TEXT,
                rejection_reason TEXT,
                promoted_memory_id VARCHAR(128),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                verified_at TIMESTAMPTZ,
                expires_at TIMESTAMPTZ
            );
            "#,
            "CREATE INDEX IF NOT EXISTS idx_esa_res_merchant_status ON esa_trusted_resolutions(merchant_id, status, updated_at DESC);",
            "CREATE INDEX IF NOT EXISTS idx_esa_res_payment ON esa_trusted_resolutions(merchant_id, payment_reference);",
            r#"
            CREATE TABLE IF NOT EXISTS esa_resolution_audit_log (
                audit_id VARCHAR(128) PRIMARY KEY,
                resolution_id VARCHAR(128) NOT NULL,
                evaluation_id VARCHAR(128) NOT NULL,
                merchant_id VARCHAR(255) NOT NULL,
                from_status VARCHAR(32),
                to_status VARCHAR(32) NOT NULL,
                actor VARCHAR(128) NOT NULL,
                verification_method VARCHAR(64) NOT NULL,
                reason TEXT NOT NULL,
                evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
                evidence_provenance JSONB NOT NULL DEFAULT '[]'::jsonb,
                promoted_memory_id VARCHAR(128),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            "#,
            "CREATE INDEX IF NOT EXISTS idx_esa_res_audit_res ON esa_resolution_audit_log(resolution_id, created_at ASC);",
            "CREATE INDEX IF NOT EXISTS idx_esa_res_audit_merchant ON esa_resolution_audit_log(merchant_id, created_at DESC);",
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
        let provenance_json = v
            .provenance
            .as_ref()
            .and_then(|p| serde_json::to_value(p).ok());

        let res = sqlx::query(
            r#"
            INSERT INTO esa_memory_records (
                memory_id, memory_type, scope, merchant_id, provider,
                incident_category, lifecycle_state, title, summary, resolution,
                source_refs, verification_status, version, created_at, updated_at, expires_at, provenance
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15, $16, $17
            )
            ON CONFLICT (memory_id) DO UPDATE SET
                title = EXCLUDED.title,
                summary = EXCLUDED.summary,
                resolution = EXCLUDED.resolution,
                source_refs = EXCLUDED.source_refs,
                verification_status = EXCLUDED.verification_status,
                version = EXCLUDED.version,
                updated_at = EXCLUDED.updated_at,
                expires_at = EXCLUDED.expires_at,
                provenance = COALESCE(esa_memory_records.provenance, EXCLUDED.provenance)
            WHERE esa_memory_records.scope = EXCLUDED.scope
              AND esa_memory_records.merchant_id IS NOT DISTINCT FROM EXCLUDED.merchant_id
              AND NOT (esa_memory_records.memory_type = 'episodic' AND esa_memory_records.verification_status = 'verified');
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
        .bind(provenance_json)
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
                "Cannot overwrite memory record owned by another scope/merchant or immutable verified episodic memory",
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
                   source_refs, verification_status, version, created_at, updated_at, expires_at, provenance
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
        let evidence_json =
            serde_json::to_value(&eval.supporting_evidence_refs).unwrap_or_default();
        sqlx::query(
            r#"
            INSERT INTO esa_experience_evaluations (
                evaluation_id, investigation_id, merchant_id, payment_reference, provider,
                recommended_root_cause, supporting_evidence_refs, inference_provider, model_id, deterministic_fallback_used,
                observed_authoritative_state, verified_outcome_state, accuracy,
                promoted_memory_id, created_at, verified_at, expires_at
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10,
                $11, $12, $13,
                $14, $15, $16, $17
            );
            "#,
        )
        .bind(&eval.evaluation_id)
        .bind(&eval.investigation_id)
        .bind(&eval.merchant_id)
        .bind(&eval.payment_reference)
        .bind(&eval.provider)
        .bind(&eval.recommended_root_cause)
        .bind(evidence_json)
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
                   recommended_root_cause, supporting_evidence_refs, inference_provider, model_id, deterministic_fallback_used,
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
        proof: AuthoritativeOutcomeProof,
    ) -> Result<(OutcomeEvaluationRecord, Option<MemoryRecord>), InferenceError> {
        let caller_mid = scope.normalized_merchant_id()?.ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "System scope cannot verify merchant-scoped evaluations",
            )
        })?;
        if proof.merchant_id() != caller_mid {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Authoritative proof merchant_id does not match caller scope",
            ));
        }

        let verified_outcome_state = proof.authoritative_state();
        let accuracy = proof.accuracy();
        if accuracy == OutcomeAccuracy::Pending {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Cannot promote outcome with Pending accuracy",
            ));
        }

        // Only OutcomeAccuracy::Correct may create a verified episodic memory;
        // PartiallyCorrect and Incorrect remain evaluations without episodic promotion.
        let will_promote = accuracy == OutcomeAccuracy::Correct;

        let mut tx = self.pool.begin().await.map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to begin verification transaction: {}", e),
            )
        })?;

        let row = sqlx::query(
            r#"
            SELECT evaluation_id, investigation_id, merchant_id, payment_reference, provider,
                   recommended_root_cause, supporting_evidence_refs, inference_provider, model_id, deterministic_fallback_used,
                   observed_authoritative_state, verified_outcome_state, accuracy,
                   promoted_memory_id, created_at, verified_at, expires_at
            FROM esa_experience_evaluations
            WHERE evaluation_id = $1
            FOR UPDATE;
            "#,
        )
        .bind(proof.evaluation_id())
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
        let now = Utc::now();
        if eval.is_expired(now) {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Cannot verify or promote expired evaluation record",
            ));
        }
        if eval.payment_reference != proof.payment_reference() {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Authoritative proof payment_reference does not match evaluation",
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
                               source_refs, verification_status, version, created_at, updated_at, expires_at, provenance
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
        let existing_res_row = sqlx::query(
            r#"
            SELECT resolution_id, status
            FROM esa_trusted_resolutions
            WHERE evaluation_id = $1 AND merchant_id = $2
            FOR UPDATE;
            "#,
        )
        .bind(&eval.evaluation_id)
        .bind(&eval.merchant_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to query trusted resolution: {}", e),
            )
        })?;
        let (resolution_id, prev_status) = match existing_res_row {
            Some(ref r) => (
                r.get::<String, _>("resolution_id"),
                ResolutionStatus::parse(r.get::<String, _>("status").as_str())
                    .unwrap_or(ResolutionStatus::Pending),
            ),
            None => (
                format!("res_{}", Uuid::new_v4().simple()),
                ResolutionStatus::Pending,
            ),
        };

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
                resolution: Some(proof.verified_resolution().to_string()),
                source_refs: vec![
                    format!("payment:{}", eval.payment_reference),
                    format!("evaluation:{}", eval.evaluation_id),
                    format!("resolution:{}", resolution_id),
                ],
                verification_status: VerificationStatus::Verified,
                version: 1,
                created_at: now,
                updated_at: now,
                expires_at: Some(now + chrono::Duration::days(180)),
                provenance: Some(MemoryProvenance {
                    evaluation_id: eval.evaluation_id.clone(),
                    resolution_id: resolution_id.clone(),
                    merchant_id: eval.merchant_id.clone(),
                    payment_reference: eval.payment_reference.clone(),
                    provider: eval.provider.clone(),
                    verified_at: now,
                    verification_method: proof.verification_method().to_string(),
                    supporting_evidence_refs: proof.evidence_refs().to_vec(),
                    evidence_provenance: proof.evidence_provenance().to_vec(),
                    promotion_decision: "promoted_verified_correct".to_string(),
                }),
            }
            .sanitize_and_validate()?;

            let source_refs_json = serde_json::to_value(&record.source_refs).unwrap_or_default();
            let provenance_json = record
                .provenance
                .as_ref()
                .and_then(|p| serde_json::to_value(p).ok());
            sqlx::query(
                r#"
                INSERT INTO esa_memory_records (
                    memory_id, memory_type, scope, merchant_id, provider,
                    incident_category, lifecycle_state, title, summary, resolution,
                    source_refs, verification_status, version, created_at, updated_at, expires_at, provenance
                ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8, $9, $10,
                    $11, $12, $13, $14, $15, $16, $17
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
            .bind(provenance_json)
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

        let target_status = if accuracy == OutcomeAccuracy::Incorrect {
            ResolutionStatus::Rejected
        } else {
            ResolutionStatus::Verified
        };
        let ev_refs_json = serde_json::to_value(proof.evidence_refs()).unwrap_or_default();
        let ev_prov_json = serde_json::to_value(proof.evidence_provenance()).unwrap_or_default();
        let empty_json = serde_json::json!([]);
        let rejection_reason: Option<&str> = if target_status == ResolutionStatus::Rejected {
            Some(proof.verified_resolution())
        } else {
            None
        };

        sqlx::query(
            r#"
            INSERT INTO esa_trusted_resolutions (
                resolution_id, investigation_id, evaluation_id, merchant_id,
                payment_reference, provider, proposed_root_cause, status, accuracy,
                authoritative_state, verification_method, evidence_refs, evidence_provenance,
                missing_evidence, verification_summary, rejection_reason, promoted_memory_id,
                created_at, updated_at, verified_at, expires_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8, $9,
                $10, $11, $12, $13,
                $14, $15, $16, $17,
                $18, $19, $20, $21
            )
            ON CONFLICT (evaluation_id) DO UPDATE SET
                status = EXCLUDED.status,
                accuracy = EXCLUDED.accuracy,
                authoritative_state = EXCLUDED.authoritative_state,
                verification_method = EXCLUDED.verification_method,
                evidence_refs = EXCLUDED.evidence_refs,
                evidence_provenance = EXCLUDED.evidence_provenance,
                missing_evidence = EXCLUDED.missing_evidence,
                verification_summary = EXCLUDED.verification_summary,
                rejection_reason = EXCLUDED.rejection_reason,
                promoted_memory_id = EXCLUDED.promoted_memory_id,
                updated_at = EXCLUDED.updated_at,
                verified_at = EXCLUDED.verified_at;
            "#,
        )
        .bind(&resolution_id)
        .bind(&eval.investigation_id)
        .bind(&eval.evaluation_id)
        .bind(&eval.merchant_id)
        .bind(&eval.payment_reference)
        .bind(&eval.provider)
        .bind(&eval.recommended_root_cause)
        .bind(target_status.as_str())
        .bind(accuracy.as_str())
        .bind(verified_outcome_state.as_str())
        .bind(proof.verification_method())
        .bind(&ev_refs_json)
        .bind(&ev_prov_json)
        .bind(&empty_json)
        .bind(proof.verified_resolution())
        .bind(rejection_reason)
        .bind(&eval.promoted_memory_id)
        .bind(eval.created_at)
        .bind(now)
        .bind(now)
        .bind(eval.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to persist verified trusted resolution: {}", e),
            )
        })?;

        sqlx::query(
            r#"
            INSERT INTO esa_resolution_audit_log (
                audit_id, resolution_id, evaluation_id, merchant_id,
                from_status, to_status, actor, verification_method, reason,
                evidence_refs, evidence_provenance, promoted_memory_id, created_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8, $9,
                $10, $11, $12, $13
            );
            "#,
        )
        .bind(format!("raud_{}", Uuid::new_v4().simple()))
        .bind(&resolution_id)
        .bind(&eval.evaluation_id)
        .bind(&eval.merchant_id)
        .bind(prev_status.as_str())
        .bind(target_status.as_str())
        .bind("system:deterministic_verifier")
        .bind(proof.verification_method())
        .bind(proof.verified_resolution())
        .bind(&ev_refs_json)
        .bind(&ev_prov_json)
        .bind(&eval.promoted_memory_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to persist resolution audit event: {}", e),
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

    async fn record_resolution(
        &self,
        resolution: TrustedResolutionRecord,
        initial_audit: ResolutionAuditEvent,
    ) -> Result<TrustedResolutionRecord, InferenceError> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to begin resolution transaction: {}", e),
            )
        })?;

        let ev_refs = serde_json::to_value(&resolution.evidence_refs).unwrap_or_default();
        let ev_prov = serde_json::to_value(&resolution.evidence_provenance).unwrap_or_default();
        let missing_ev = serde_json::to_value(&resolution.missing_evidence).unwrap_or_default();

        sqlx::query(
            r#"
            INSERT INTO esa_trusted_resolutions (
                resolution_id, investigation_id, evaluation_id, merchant_id,
                payment_reference, provider, proposed_root_cause, status, accuracy,
                authoritative_state, verification_method, evidence_refs, evidence_provenance,
                missing_evidence, verification_summary, rejection_reason, promoted_memory_id,
                created_at, updated_at, verified_at, expires_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8, $9,
                $10, $11, $12, $13,
                $14, $15, $16, $17,
                $18, $19, $20, $21
            );
            "#,
        )
        .bind(&resolution.resolution_id)
        .bind(&resolution.investigation_id)
        .bind(&resolution.evaluation_id)
        .bind(&resolution.merchant_id)
        .bind(&resolution.payment_reference)
        .bind(&resolution.provider)
        .bind(&resolution.proposed_root_cause)
        .bind(resolution.status.as_str())
        .bind(resolution.accuracy.as_str())
        .bind(resolution.authoritative_state.map(|s| s.as_str()))
        .bind(&resolution.verification_method)
        .bind(ev_refs)
        .bind(ev_prov)
        .bind(missing_ev)
        .bind(&resolution.verification_summary)
        .bind(&resolution.rejection_reason)
        .bind(&resolution.promoted_memory_id)
        .bind(resolution.created_at)
        .bind(resolution.updated_at)
        .bind(resolution.verified_at)
        .bind(resolution.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to record trusted resolution: {}", e),
            )
        })?;

        let audit_refs = serde_json::to_value(&initial_audit.evidence_refs).unwrap_or_default();
        let audit_prov =
            serde_json::to_value(&initial_audit.evidence_provenance).unwrap_or_default();
        sqlx::query(
            r#"
            INSERT INTO esa_resolution_audit_log (
                audit_id, resolution_id, evaluation_id, merchant_id,
                from_status, to_status, actor, verification_method, reason,
                evidence_refs, evidence_provenance, promoted_memory_id, created_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8, $9,
                $10, $11, $12, $13
            );
            "#,
        )
        .bind(&initial_audit.audit_id)
        .bind(&initial_audit.resolution_id)
        .bind(&initial_audit.evaluation_id)
        .bind(&initial_audit.merchant_id)
        .bind(initial_audit.from_status.map(|s| s.as_str()))
        .bind(initial_audit.to_status.as_str())
        .bind(&initial_audit.actor)
        .bind(&initial_audit.verification_method)
        .bind(&initial_audit.reason)
        .bind(audit_refs)
        .bind(audit_prov)
        .bind(&initial_audit.promoted_memory_id)
        .bind(initial_audit.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to record initial resolution audit: {}", e),
            )
        })?;

        tx.commit().await.map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to commit resolution creation: {}", e),
            )
        })?;

        Ok(resolution)
    }

    async fn get_resolution(
        &self,
        scope: &ExecutionScope,
        resolution_id: &str,
    ) -> Result<Option<(TrustedResolutionRecord, Vec<ResolutionAuditEvent>)>, InferenceError> {
        let caller_mid = scope.normalized_merchant_id()?.ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "System scope cannot read merchant-scoped resolutions",
            )
        })?;
        let row = sqlx::query(
            r#"
            SELECT resolution_id, investigation_id, evaluation_id, merchant_id,
                   payment_reference, provider, proposed_root_cause, status, accuracy,
                   authoritative_state, verification_method, evidence_refs, evidence_provenance,
                   missing_evidence, verification_summary, rejection_reason, promoted_memory_id,
                   created_at, updated_at, verified_at, expires_at
            FROM esa_trusted_resolutions
            WHERE resolution_id = $1;
            "#,
        )
        .bind(resolution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to load trusted resolution: {}", e),
            )
        })?;

        let Some(r) = row else {
            return Ok(None);
        };
        let res = row_to_resolution(r);
        if res.merchant_id != caller_mid {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Trusted resolution does not belong to authenticated merchant",
            ));
        }

        let audit_rows = sqlx::query(
            r#"
            SELECT audit_id, resolution_id, evaluation_id, merchant_id,
                   from_status, to_status, actor, verification_method, reason,
                   evidence_refs, evidence_provenance, promoted_memory_id, created_at
            FROM esa_resolution_audit_log
            WHERE resolution_id = $1 AND merchant_id = $2
            ORDER BY created_at ASC;
            "#,
        )
        .bind(resolution_id)
        .bind(caller_mid)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to load resolution audit log: {}", e),
            )
        })?;

        let audits = audit_rows
            .into_iter()
            .map(row_to_resolution_audit)
            .collect();
        Ok(Some((res, audits)))
    }

    async fn list_resolutions(
        &self,
        scope: &ExecutionScope,
        status: Option<ResolutionStatus>,
        payment_reference: Option<&str>,
        limit: usize,
    ) -> Result<Vec<TrustedResolutionRecord>, InferenceError> {
        let caller_mid = scope.normalized_merchant_id()?.ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "System scope cannot list merchant-scoped resolutions",
            )
        })?;
        let lim = limit.clamp(1, 100) as i64;
        let clean_pref = payment_reference.map(str::trim).filter(|s| !s.is_empty());
        let rows = sqlx::query(
            r#"
            SELECT resolution_id, investigation_id, evaluation_id, merchant_id,
                   payment_reference, provider, proposed_root_cause, status, accuracy,
                   authoritative_state, verification_method, evidence_refs, evidence_provenance,
                   missing_evidence, verification_summary, rejection_reason, promoted_memory_id,
                   created_at, updated_at, verified_at, expires_at
            FROM esa_trusted_resolutions
            WHERE merchant_id = $1
              AND ($2::text IS NULL OR status = $2)
              AND ($3::text IS NULL OR payment_reference = $3)
            ORDER BY updated_at DESC
            LIMIT $4;
            "#,
        )
        .bind(caller_mid)
        .bind(status.map(|s| s.as_str()))
        .bind(clean_pref)
        .bind(lim)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to list trusted resolutions: {}", e),
            )
        })?;
        Ok(rows.into_iter().map(row_to_resolution).collect())
    }

    async fn record_resolution_pending_or_expired(
        &self,
        scope: &ExecutionScope,
        resolution_id: &str,
        new_status: ResolutionStatus,
        reason: &str,
        missing_evidence: Vec<String>,
    ) -> Result<TrustedResolutionRecord, InferenceError> {
        if !matches!(
            new_status,
            ResolutionStatus::Pending | ResolutionStatus::UnderReview | ResolutionStatus::Expired
        ) {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Non-proof transition cannot mark a resolution as Verified or Rejected",
            ));
        }
        let caller_mid = scope.normalized_merchant_id()?.ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "System scope cannot update merchant-scoped resolutions",
            )
        })?;

        let mut tx = self.pool.begin().await.map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to begin resolution update transaction: {}", e),
            )
        })?;

        let row = sqlx::query(
            r#"
            SELECT resolution_id, investigation_id, evaluation_id, merchant_id,
                   payment_reference, provider, proposed_root_cause, status, accuracy,
                   authoritative_state, verification_method, evidence_refs, evidence_provenance,
                   missing_evidence, verification_summary, rejection_reason, promoted_memory_id,
                   created_at, updated_at, verified_at, expires_at
            FROM esa_trusted_resolutions
            WHERE resolution_id = $1
            FOR UPDATE;
            "#,
        )
        .bind(resolution_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to lock trusted resolution: {}", e),
            )
        })?;

        let Some(r) = row else {
            return Err(InferenceError::new(
                InferenceErrorCategory::NotFound,
                "Trusted resolution record not found",
            ));
        };
        let mut res = row_to_resolution(r);
        if res.merchant_id != caller_mid {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Trusted resolution does not belong to authenticated merchant",
            ));
        }
        if res.status.is_terminal() {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                format!(
                    "Cannot transition terminal resolution '{}' from '{}'",
                    resolution_id,
                    res.status.as_str()
                ),
            ));
        }

        let prev_status = res.status;
        let now = Utc::now();
        res.status = new_status;
        res.missing_evidence = missing_evidence.clone();
        res.updated_at = now;
        let missing_json = serde_json::to_value(&missing_evidence).unwrap_or_default();

        sqlx::query(
            r#"
            UPDATE esa_trusted_resolutions
            SET status = $2, missing_evidence = $3, updated_at = $4
            WHERE resolution_id = $1;
            "#,
        )
        .bind(resolution_id)
        .bind(new_status.as_str())
        .bind(missing_json)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to update trusted resolution: {}", e),
            )
        })?;

        let ev_refs_json = serde_json::to_value(&res.evidence_refs).unwrap_or_default();
        let ev_prov_json = serde_json::to_value(&res.evidence_provenance).unwrap_or_default();
        sqlx::query(
            r#"
            INSERT INTO esa_resolution_audit_log (
                audit_id, resolution_id, evaluation_id, merchant_id,
                from_status, to_status, actor, verification_method, reason,
                evidence_refs, evidence_provenance, promoted_memory_id, created_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8, $9,
                $10, $11, $12, $13
            );
            "#,
        )
        .bind(format!("raud_{}", Uuid::new_v4().simple()))
        .bind(resolution_id)
        .bind(&res.evaluation_id)
        .bind(&res.merchant_id)
        .bind(prev_status.as_str())
        .bind(new_status.as_str())
        .bind("system:deterministic_verifier")
        .bind("deterministic_webhook_store_v1")
        .bind(reason)
        .bind(ev_refs_json)
        .bind(ev_prov_json)
        .bind(Option::<String>::None)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to persist resolution audit event: {}", e),
            )
        })?;

        tx.commit().await.map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to commit resolution update: {}", e),
            )
        })?;

        Ok(res)
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
    let provenance: Option<MemoryProvenance> = r
        .try_get::<Option<serde_json::Value>, _>("provenance")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok());
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
        provenance,
    })
}

fn row_to_resolution(r: sqlx::postgres::PgRow) -> TrustedResolutionRecord {
    let status = ResolutionStatus::parse(r.get::<String, _>("status").as_str())
        .unwrap_or(ResolutionStatus::Pending);
    let accuracy = OutcomeAccuracy::parse(r.get::<String, _>("accuracy").as_str())
        .unwrap_or(OutcomeAccuracy::Pending);
    let auth_state: Option<String> = r.get("authoritative_state");
    let evidence_refs: Vec<String> = r
        .try_get::<serde_json::Value, _>("evidence_refs")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let evidence_provenance: Vec<String> = r
        .try_get::<serde_json::Value, _>("evidence_provenance")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let missing_evidence: Vec<String> = r
        .try_get::<serde_json::Value, _>("missing_evidence")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    TrustedResolutionRecord {
        resolution_id: r.get("resolution_id"),
        investigation_id: r.get("investigation_id"),
        evaluation_id: r.get("evaluation_id"),
        merchant_id: r.get("merchant_id"),
        payment_reference: r.get("payment_reference"),
        provider: r.get("provider"),
        proposed_root_cause: r.get("proposed_root_cause"),
        status,
        accuracy,
        authoritative_state: auth_state.as_deref().map(parse_canonical_state),
        verification_method: r.get("verification_method"),
        evidence_refs,
        evidence_provenance,
        missing_evidence,
        verification_summary: r.get("verification_summary"),
        rejection_reason: r.get("rejection_reason"),
        promoted_memory_id: r.get("promoted_memory_id"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        verified_at: r.get("verified_at"),
        expires_at: r.get("expires_at"),
    }
}

fn row_to_resolution_audit(r: sqlx::postgres::PgRow) -> ResolutionAuditEvent {
    let from_status: Option<String> = r.get("from_status");
    let to_status = ResolutionStatus::parse(r.get::<String, _>("to_status").as_str())
        .unwrap_or(ResolutionStatus::Pending);
    let evidence_refs: Vec<String> = r
        .try_get::<serde_json::Value, _>("evidence_refs")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let evidence_provenance: Vec<String> = r
        .try_get::<serde_json::Value, _>("evidence_provenance")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    ResolutionAuditEvent {
        audit_id: r.get("audit_id"),
        resolution_id: r.get("resolution_id"),
        evaluation_id: r.get("evaluation_id"),
        merchant_id: r.get("merchant_id"),
        from_status: from_status.as_deref().and_then(ResolutionStatus::parse),
        to_status,
        actor: r.get("actor"),
        verification_method: r.get("verification_method"),
        reason: r.get("reason"),
        evidence_refs,
        evidence_provenance,
        promoted_memory_id: r.get("promoted_memory_id"),
        created_at: r.get("created_at"),
    }
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

    let evidence_val: serde_json::Value = r
        .try_get("supporting_evidence_refs")
        .unwrap_or_else(|_| serde_json::json!([]));
    let supporting_evidence_refs: Vec<String> =
        serde_json::from_value(evidence_val).unwrap_or_default();

    OutcomeEvaluationRecord {
        evaluation_id: r.get("evaluation_id"),
        investigation_id: r.get("investigation_id"),
        merchant_id: r.get("merchant_id"),
        payment_reference: r.get("payment_reference"),
        provider: r.get("provider"),
        recommended_root_cause: r.get("recommended_root_cause"),
        supporting_evidence_refs,
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
            provenance: None,
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
            provenance: None,
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

        // 4. Atomic verified promotion via WebhookStore boundary & diagnosis-level checks
        use esa_state::WebhookStore;
        esa_state::PostgresWebhookStore::init_schema(&pool)
            .await
            .unwrap();
        let wh_store = esa_state::PostgresWebhookStore::new(pool.clone());

        let pay_ref = format!("pay_pg_{}", &suffix[..8]);
        let tx_id = format!("tx_pg_{}", &suffix[..8]);
        let attempt_cap = esa_core::provider::PaymentAttemptRecord {
            transaction_id: tx_id.clone(),
            merchant_id: m_a.clone(),
            provider: "razorpay".to_string(),
            provider_order_id: None,
            payment_reference: Some(pay_ref.clone()),
            amount_minor: 10000,
            currency: "INR".to_string(),
            state: "captured".to_string(),
            created_at: now,
            updated_at: now,
        };
        wh_store.record_payment_attempt(&attempt_cap).await.unwrap();

        let entity_cap = esa_core::payment_state::PaymentEntity {
            payment_id: pay_ref.clone(),
            state: esa_core::payment_state::PaymentState::Captured,
            amount_minor: 10000,
            currency: "INR".to_string(),
            refunded_amount_minor: 0,
            events_seen: vec![],
            refunds: vec![],
            created_at: now,
            updated_at: now,
        };

        // Update eval's payment_reference to match unique test payment
        let eval_unique = OutcomeEvaluationRecord::new_pending(
            format!("inv_pg_u_{}", &suffix[..8]),
            m_a.clone(),
            pay_ref.clone(),
            "razorpay".to_string(),
            "AUTHORIZATION_NOT_CAPTURED".to_string(),
            ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            true,
            CanonicalPaymentState::Authorized,
        );
        let eval_unique_id = eval_unique.evaluation_id.clone();
        store.record_evaluation(eval_unique).await.unwrap();

        // Unsupported diagnosis (DUPLICATE_WEBHOOK_SUPPRESSED without duplicate webhook) stays pending even when Captured
        let eval_unsupported = OutcomeEvaluationRecord::new_pending(
            format!("inv_pg_unsup_{}", &suffix[..8]),
            m_a.clone(),
            pay_ref.clone(),
            "razorpay".to_string(),
            "DUPLICATE_WEBHOOK_SUPPRESSED".to_string(),
            ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec!["webhook:evt_missing_dup".to_string()]);
        let eval_unsup_id = eval_unsupported.evaluation_id.clone();
        store.record_evaluation(eval_unsupported).await.unwrap();
        let unsup_err = crate::memory::verify_outcome_from_webhook_store(
            &wh_store,
            &store,
            &scope_a,
            &eval_unsup_id,
        )
        .await
        .unwrap_err();
        assert_eq!(unsup_err.category, InferenceErrorCategory::InvalidResponse);
        assert_eq!(
            store
                .get_evaluation(&scope_a, &eval_unsup_id)
                .await
                .unwrap()
                .unwrap()
                .accuracy,
            OutcomeAccuracy::Pending
        );

        // PartiallyCorrect (AUTHORIZATION_NOT_CAPTURED -> Captured) updates evaluation in Postgres without promoting episodic memory
        let (ev_part, mem_part) = crate::memory::verify_outcome_from_webhook_store(
            &wh_store,
            &store,
            &scope_a,
            &eval_unique_id,
        )
        .await
        .unwrap();
        assert_eq!(ev_part.accuracy, OutcomeAccuracy::PartiallyCorrect);
        assert!(ev_part.promoted_memory_id.is_none());
        assert!(mem_part.is_none());

        // Correct (HEALTHY_COMPLETED -> Captured) promotes exactly once in Postgres and is idempotent
        let eval_corr = OutcomeEvaluationRecord::new_pending(
            format!("inv_pg_corr_{}", &suffix[..8]),
            m_a.clone(),
            pay_ref.clone(),
            "razorpay".to_string(),
            "HEALTHY_COMPLETED".to_string(),
            ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        );
        let eval_corr_id = eval_corr.evaluation_id.clone();
        store.record_evaluation(eval_corr).await.unwrap();

        let (ev1, mem1) = crate::memory::verify_outcome_from_webhook_store(
            &wh_store,
            &store,
            &scope_a,
            &eval_corr_id,
        )
        .await
        .unwrap();
        assert_eq!(ev1.accuracy, OutcomeAccuracy::Correct);
        let promoted_id = ev1.promoted_memory_id.clone().unwrap();
        assert_eq!(mem1.unwrap().memory_id, promoted_id);

        let (ev2, mem2) = crate::memory::verify_outcome_from_webhook_store(
            &wh_store,
            &store,
            &scope_a,
            &eval_corr_id,
        )
        .await
        .unwrap();
        assert_eq!(
            ev2.promoted_memory_id.as_deref(),
            Some(promoted_id.as_str())
        );
        assert_eq!(mem2.unwrap().memory_id, promoted_id);

        // Empty webhook inbox cannot verify WEBHOOK_DELAYED_OR_MISSING in Postgres
        let eval_empty_wh = OutcomeEvaluationRecord::new_pending(
            format!("inv_pg_ewh_{}", &suffix[..8]),
            m_a.clone(),
            pay_ref.clone(),
            "razorpay".to_string(),
            "WEBHOOK_DELAYED_OR_MISSING".to_string(),
            ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec![format!("attempt:{}", tx_id)]);
        let eval_ewh_id = eval_empty_wh.evaluation_id.clone();
        store.record_evaluation(eval_empty_wh).await.unwrap();
        assert_eq!(
            crate::memory::verify_outcome_from_webhook_store(
                &wh_store,
                &store,
                &scope_a,
                &eval_ewh_id,
            )
            .await
            .unwrap_err()
            .category,
            InferenceErrorCategory::InvalidResponse
        );

        // Provider mismatch between evaluation and authoritative attempt is rejected in Postgres
        let eval_prov_mm = OutcomeEvaluationRecord::new_pending(
            format!("inv_pg_pmm_{}", &suffix[..8]),
            m_a.clone(),
            pay_ref.clone(),
            "cashfree".to_string(),
            "HEALTHY_COMPLETED".to_string(),
            ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        );
        let eval_pmm_id = eval_prov_mm.evaluation_id.clone();
        store.record_evaluation(eval_prov_mm).await.unwrap();
        assert_eq!(
            crate::memory::verify_outcome_from_webhook_store(
                &wh_store,
                &store,
                &scope_a,
                &eval_pmm_id,
            )
            .await
            .unwrap_err()
            .category,
            InferenceErrorCategory::InvalidResponse
        );

        // 5. Expired evaluation verification rejection & physical purge
        let mut exp_eval = OutcomeEvaluationRecord::new_pending(
            format!("inv_pg_exp_{}", &suffix[..8]),
            m_a.clone(),
            pay_ref.clone(),
            "razorpay".to_string(),
            "HEALTHY_COMPLETED".to_string(),
            ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            true,
            CanonicalPaymentState::Authorized,
        );
        let exp_proof = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_a,
            &exp_eval,
            &attempt_cap,
            Some(&entity_cap),
            &[],
            &[],
        )
        .unwrap();
        exp_eval.expires_at = Some(Utc::now() - chrono::Duration::seconds(10));
        let exp_eval_id = exp_eval.evaluation_id.clone();
        store.record_evaluation(exp_eval).await.unwrap();

        let exp_err = store
            .verify_and_promote_outcome(&scope_a, exp_proof)
            .await
            .unwrap_err();
        assert_eq!(exp_err.category, InferenceErrorCategory::InvalidResponse);

        // Physical purge deletes expired evaluation row from PostgreSQL
        let purged = store.purge_expired().await.unwrap();
        assert!(purged >= 1);
        let row_after_purge: Option<(String,)> = sqlx::query_as(
            "SELECT evaluation_id FROM esa_experience_evaluations WHERE evaluation_id = $1;",
        )
        .bind(&exp_eval_id)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(row_after_purge.is_none());

        // Cleanup test records
        let _ = store.delete_memory(&scope_a, &shared_mem_id).await;
        let _ = store.delete_memory(&scope_a, &promoted_id).await;
        let _ = store.delete_evaluations_for_merchant(&scope_a).await;
    }

    #[tokio::test]
    async fn test_postgres_trusted_resolution_atomic_promotion_immutability_and_isolation() {
        use esa_state::WebhookStore;
        let Ok(db_url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let pool = PgPool::connect(&db_url).await.unwrap();
        esa_state::PostgresWebhookStore::init_schema(&pool)
            .await
            .unwrap();
        let wh_store = esa_state::PostgresWebhookStore::new(pool.clone());
        let mem_store = PostgresPaymentMemoryStore::new(pool.clone());
        mem_store.init_schema().await.unwrap();

        let suffix = &Uuid::new_v4().simple().to_string()[..10];
        let m_a = format!("merch_p4_pg_a_{}", suffix);
        let m_b = format!("merch_p4_pg_b_{}", suffix);
        let scope_a = ExecutionScope::MerchantScoped {
            merchant_id: m_a.clone(),
        };
        let scope_b = ExecutionScope::MerchantScoped {
            merchant_id: m_b.clone(),
        };
        let tx_id = format!("tx_p4_pg_{}", suffix);
        let pay_id = format!("pay_p4_pg_{}", suffix);
        let evt_id = format!("evt_p4_pg_{}", suffix);
        let now = Utc::now();

        wh_store
            .record_payment_attempt(&esa_core::provider::PaymentAttemptRecord {
                transaction_id: tx_id,
                merchant_id: m_a.clone(),
                provider: "razorpay".to_string(),
                provider_order_id: None,
                payment_reference: Some(pay_id.clone()),
                amount_minor: 45000,
                currency: "INR".to_string(),
                state: "created".to_string(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();

        let payload = serde_json::json!({ "event": "payment.captured" });
        let params = esa_state::WebhookEventParams {
            provider: "razorpay",
            provider_event_id: &evt_id,
            event_type: esa_core::PaymentEventType::PaymentCaptured,
            payment_id: &pay_id,
            amount_minor: 45000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        wh_store.process_webhook_event(&params).await.unwrap();

        let eval = OutcomeEvaluationRecord::new_pending(
            format!("inv_p4_pg_{}", suffix),
            m_a.clone(),
            pay_id.clone(),
            "razorpay".to_string(),
            "DUPLICATE_WEBHOOK_SUPPRESSED".to_string(),
            ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec![format!("webhook:{}", evt_id)]);
        let eval = mem_store.record_evaluation(eval).await.unwrap();
        let res = TrustedResolutionRecord::from_evaluation(&eval);
        let res = mem_store
            .record_resolution(
                res.clone(),
                ResolutionAuditEvent {
                    audit_id: format!("raud_pg_{}", Uuid::new_v4().simple()),
                    resolution_id: res.resolution_id.clone(),
                    evaluation_id: eval.evaluation_id.clone(),
                    merchant_id: m_a.clone(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system:payment_intelligence".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "Initial pending resolution".to_string(),
                    evidence_refs: eval.supporting_evidence_refs.clone(),
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();

        // 1. Fail closed before duplicate delivery
        let (pending_res, _, pending_mem) = crate::memory::verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_a,
            &res.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(pending_res.status, ResolutionStatus::Pending);
        assert!(pending_mem.is_none());

        // 2. Deliver duplicate webhook -> durable audit proves duplicate suppression
        wh_store.process_webhook_event(&params).await.unwrap();

        let (verified_res, audits, promoted_mem) =
            crate::memory::verify_resolution_from_webhook_store(
                &wh_store,
                &mem_store,
                &scope_a,
                &res.resolution_id,
            )
            .await
            .unwrap();
        assert_eq!(verified_res.status, ResolutionStatus::Verified);
        assert_eq!(verified_res.accuracy, OutcomeAccuracy::Correct);
        assert!(audits.len() >= 3);
        let mem = promoted_mem.expect("Verified Correct outcome must promote atomically");
        let prov = mem
            .provenance
            .clone()
            .expect("Provenance must be persisted");
        assert_eq!(prov.resolution_id, res.resolution_id);
        assert_eq!(prov.evaluation_id, eval.evaluation_id);

        // 3. Idempotent re-verification returns same result without duplicating memory
        let (idem_res, _, idem_mem) = crate::memory::verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_a,
            &res.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(idem_res.promoted_memory_id, verified_res.promoted_memory_id);
        assert_eq!(idem_mem.unwrap().memory_id, mem.memory_id);

        // 4. Promoted verified episodic memory is immutable in PostgreSQL
        let mut tampered = mem.clone();
        tampered.summary = "Tampered summary".to_string();
        let immut_err = mem_store.store_memory(tampered).await.unwrap_err();
        assert_eq!(immut_err.category, InferenceErrorCategory::Forbidden);

        // 5. Cross-merchant resolution access is Forbidden
        let cross_err = mem_store
            .get_resolution(&scope_b, &res.resolution_id)
            .await
            .unwrap_err();
        assert_eq!(cross_err.category, InferenceErrorCategory::Forbidden);

        let _ = mem_store.delete_memory(&scope_a, &mem.memory_id).await;
        let _ = mem_store.delete_evaluations_for_merchant(&scope_a).await;
    }
}
