use crate::memory::types::{
    AuthoritativeOutcomeProof, MemoryProvenance, MemoryQuery, MemoryRecord, MemoryScope,
    MemoryType, OutcomeAccuracy, OutcomeEvaluationRecord, ResolutionAuditEvent, ResolutionStatus,
    TrustedResolutionRecord, VerificationStatus,
};
use crate::provider::{ExecutionScope, InferenceError, InferenceErrorCategory};
use async_trait::async_trait;
use chrono::Utc;
use esa_state::WebhookStore;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

#[async_trait]
pub trait PaymentMemoryStore: Send + Sync {
    async fn init_schema(&self) -> Result<(), InferenceError> {
        Ok(())
    }

    async fn store_memory(&self, record: MemoryRecord) -> Result<MemoryRecord, InferenceError>;

    async fn query_memories(
        &self,
        query: &MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, InferenceError>;

    async fn delete_memory(
        &self,
        scope: &ExecutionScope,
        memory_id: &str,
    ) -> Result<bool, InferenceError>;

    async fn purge_expired(&self) -> Result<usize, InferenceError>;

    async fn record_evaluation(
        &self,
        eval: OutcomeEvaluationRecord,
    ) -> Result<OutcomeEvaluationRecord, InferenceError>;

    async fn get_evaluation(
        &self,
        scope: &ExecutionScope,
        evaluation_id: &str,
    ) -> Result<Option<OutcomeEvaluationRecord>, InferenceError>;

    async fn verify_and_promote_outcome(
        &self,
        scope: &ExecutionScope,
        proof: AuthoritativeOutcomeProof,
    ) -> Result<(OutcomeEvaluationRecord, Option<MemoryRecord>), InferenceError>;

    async fn delete_evaluations_for_merchant(
        &self,
        scope: &ExecutionScope,
    ) -> Result<usize, InferenceError>;

    async fn record_resolution(
        &self,
        resolution: TrustedResolutionRecord,
        initial_audit: ResolutionAuditEvent,
    ) -> Result<TrustedResolutionRecord, InferenceError>;

    async fn get_resolution(
        &self,
        scope: &ExecutionScope,
        resolution_id: &str,
    ) -> Result<Option<(TrustedResolutionRecord, Vec<ResolutionAuditEvent>)>, InferenceError>;

    async fn list_resolutions(
        &self,
        scope: &ExecutionScope,
        status: Option<ResolutionStatus>,
        payment_reference: Option<&str>,
        limit: usize,
    ) -> Result<Vec<TrustedResolutionRecord>, InferenceError>;

    async fn record_resolution_pending_or_expired(
        &self,
        scope: &ExecutionScope,
        resolution_id: &str,
        new_status: ResolutionStatus,
        reason: &str,
        missing_evidence: Vec<String>,
    ) -> Result<TrustedResolutionRecord, InferenceError>;
}

/// Trusted crate-internal verification boundary: fetches authoritative payment, webhook, delivery, and refund
/// records from `WebhookStore`, verifies merchant ownership and diagnosis, and promotes only if `Correct`.
#[allow(dead_code)]
pub(crate) async fn verify_outcome_from_webhook_store(
    webhook_store: &dyn WebhookStore,
    memory_store: &dyn PaymentMemoryStore,
    scope: &ExecutionScope,
    evaluation_id: &str,
) -> Result<(OutcomeEvaluationRecord, Option<MemoryRecord>), InferenceError> {
    let eval = memory_store
        .get_evaluation(scope, evaluation_id)
        .await?
        .ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::NotFound,
                "Evaluation record not found or expired",
            )
        })?;

    let attempts = webhook_store
        .list_payment_attempts_for_merchant(Some(&eval.merchant_id), 200)
        .await
        .map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to load authoritative payment attempts: {}", e),
            )
        })?;

    let attempt = attempts
        .into_iter()
        .find(|a| {
            a.payment_reference.as_deref() == Some(eval.payment_reference.as_str())
                || a.transaction_id == eval.payment_reference
        })
        .ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::NotFound,
                "Authoritative payment attempt not found for evaluation",
            )
        })?;

    let payment_entity = if let Some(ref pref) = attempt.payment_reference {
        webhook_store.get_payment_record(pref).await.map_err(|e| {
            InferenceError::new(
                InferenceErrorCategory::UpstreamError,
                format!("Failed to load authoritative payment record: {}", e),
            )
        })?
    } else {
        None
    };

    let inbox_events = if let Some(ref pref) = attempt.payment_reference {
        webhook_store
            .list_inbox_events_for_payment(pref, 50)
            .await
            .map_err(|e| {
                InferenceError::new(
                    InferenceErrorCategory::UpstreamError,
                    format!("Failed to load authoritative webhook inbox events: {}", e),
                )
            })?
    } else {
        Vec::new()
    };

    let delivery_audits = if let Some(ref pref) = attempt.payment_reference {
        webhook_store
            .list_webhook_delivery_audits_for_payment(&eval.merchant_id, pref, 100)
            .await
            .map_err(|e| {
                InferenceError::new(
                    InferenceErrorCategory::UpstreamError,
                    format!(
                        "Failed to load authoritative webhook delivery audits: {}",
                        e
                    ),
                )
            })?
    } else {
        Vec::new()
    };

    let refunds = if let Some(ref pref) = attempt.payment_reference {
        webhook_store
            .list_refunds_for_payment(&attempt.provider, pref)
            .await
            .map_err(|e| {
                InferenceError::new(
                    InferenceErrorCategory::UpstreamError,
                    format!("Failed to load authoritative refund records: {}", e),
                )
            })?
    } else {
        Vec::new()
    };

    let proof = AuthoritativeOutcomeProof::from_authoritative_records_with_deliveries(
        scope,
        &eval,
        &attempt,
        payment_entity.as_ref(),
        &inbox_events,
        &delivery_audits,
        &refunds,
    )?;

    memory_store.verify_and_promote_outcome(scope, proof).await
}

/// Authorized server-side workflow to deterministically verify a `TrustedResolutionRecord`
/// against ESAPay's authoritative `WebhookStore`. Never accepts caller-supplied outcomes or proofs.
pub async fn verify_resolution_from_webhook_store(
    webhook_store: &dyn WebhookStore,
    memory_store: &dyn PaymentMemoryStore,
    scope: &ExecutionScope,
    resolution_id: &str,
) -> Result<
    (
        TrustedResolutionRecord,
        Vec<ResolutionAuditEvent>,
        Option<MemoryRecord>,
    ),
    InferenceError,
> {
    let (resolution, audits) = memory_store
        .get_resolution(scope, resolution_id)
        .await?
        .ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::NotFound,
                "Trusted resolution record not found",
            )
        })?;

    if resolution.status.is_terminal() {
        let mem = if let Some(ref mid) = resolution.promoted_memory_id {
            let mems = memory_store
                .query_memories(&MemoryQuery {
                    scope: scope.clone(),
                    memory_types: vec![MemoryType::Episodic],
                    provider: None,
                    incident_category: None,
                    lifecycle_state: None,
                    since: None,
                    limit: 50,
                })
                .await?;
            mems.into_iter().find(|m| &m.memory_id == mid)
        } else {
            None
        };
        return Ok((resolution, audits, mem));
    }

    if resolution.is_expired(Utc::now()) {
        let updated = memory_store
            .record_resolution_pending_or_expired(
                scope,
                resolution_id,
                ResolutionStatus::Expired,
                "Resolution exceeded its permitted verification period",
                vec!["verification_period_expired".to_string()],
            )
            .await?;
        let (final_res, final_audits) = memory_store
            .get_resolution(scope, resolution_id)
            .await?
            .unwrap_or((updated, audits));
        return Ok((final_res, final_audits, None));
    }

    match verify_outcome_from_webhook_store(
        webhook_store,
        memory_store,
        scope,
        &resolution.evaluation_id,
    )
    .await
    {
        Ok((_, promoted_mem)) => {
            let (final_res, final_audits) = memory_store
                .get_resolution(scope, resolution_id)
                .await?
                .ok_or_else(|| {
                    InferenceError::new(
                        InferenceErrorCategory::NotFound,
                        "Trusted resolution disappeared after verification",
                    )
                })?;
            Ok((final_res, final_audits, promoted_mem))
        }
        Err(err) if err.category == InferenceErrorCategory::InvalidResponse => {
            let updated = memory_store
                .record_resolution_pending_or_expired(
                    scope,
                    resolution_id,
                    ResolutionStatus::Pending,
                    &err.message,
                    vec![err.message.clone()],
                )
                .await?;
            let (final_res, final_audits) = memory_store
                .get_resolution(scope, resolution_id)
                .await?
                .unwrap_or((updated, audits));
            Ok((final_res, final_audits, None))
        }
        Err(err) => Err(err),
    }
}

/// Thread-safe in-memory `PaymentMemoryStore` enforcing identical tenant isolation and retention rules.
pub struct InMemoryPaymentMemoryStore {
    memories: Mutex<HashMap<String, MemoryRecord>>,
    evaluations: Mutex<HashMap<String, OutcomeEvaluationRecord>>,
    resolutions: Mutex<HashMap<String, TrustedResolutionRecord>>,
    resolution_audits: Mutex<HashMap<String, Vec<ResolutionAuditEvent>>>,
}

impl Default for InMemoryPaymentMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryPaymentMemoryStore {
    pub fn new() -> Self {
        Self {
            memories: Mutex::new(HashMap::new()),
            evaluations: Mutex::new(HashMap::new()),
            resolutions: Mutex::new(HashMap::new()),
            resolution_audits: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    pub fn insert_test_memory(&self, record: MemoryRecord) {
        self.memories
            .lock()
            .unwrap()
            .insert(record.memory_id.clone(), record);
    }
}

#[async_trait]
impl PaymentMemoryStore for InMemoryPaymentMemoryStore {
    async fn store_memory(&self, record: MemoryRecord) -> Result<MemoryRecord, InferenceError> {
        let validated = record.sanitize_and_validate()?;
        if validated.memory_type == MemoryType::Episodic {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Episodic memories cannot be created or modified directly through generic store_memory(); must be promoted through deterministic verification",
            ));
        }
        let mut guard = self.memories.lock().unwrap();
        if let Some(existing) = guard.get(&validated.memory_id) {
            if existing.scope != validated.scope || existing.merchant_id != validated.merchant_id {
                return Err(InferenceError::new(
                    InferenceErrorCategory::Forbidden,
                    "Cannot overwrite memory record owned by another scope or merchant",
                ));
            }
            if existing.memory_type == MemoryType::Episodic
                && existing.verification_status == VerificationStatus::Verified
            {
                return Err(InferenceError::new(
                    InferenceErrorCategory::Forbidden,
                    "Promoted verified episodic memories are immutable",
                ));
            }
        }
        guard.insert(validated.memory_id.clone(), validated.clone());
        Ok(validated)
    }

    async fn query_memories(
        &self,
        query: &MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, InferenceError> {
        let merchant_id = query.scope.normalized_merchant_id()?;
        let now = Utc::now();
        let limit = query.limit.clamp(1, 50);

        let guard = self.memories.lock().unwrap();
        let mut results: Vec<MemoryRecord> = guard
            .values()
            .filter(|m| !m.is_expired(now))
            .filter(|m| match (m.scope, merchant_id) {
                // Approved System Semantic/Procedural knowledge is readable in both System and Merchant scopes
                (MemoryScope::System, _) => true,
                // Merchant Episodic memory is strictly isolated to matching merchant_id (never cross-merchant!)
                (MemoryScope::Merchant, Some(mid)) => m.merchant_id.as_deref() == Some(mid),
                // System scope query returns only System records (never leaks another merchant's episodes)
                (MemoryScope::Merchant, None) => false,
            })
            .filter(|m| {
                query.memory_types.is_empty() || query.memory_types.contains(&m.memory_type)
            })
            .filter(|m| match &query.provider {
                Some(p) => m
                    .provider
                    .as_deref()
                    .is_none_or(|mp| mp.eq_ignore_ascii_case(p)),
                None => true,
            })
            .filter(|m| match &query.incident_category {
                Some(cat) => m.incident_category.eq_ignore_ascii_case(cat),
                None => true,
            })
            .filter(|m| match query.lifecycle_state {
                Some(st) => m.lifecycle_state.is_none_or(|ms| ms == st),
                None => true,
            })
            .filter(|m| match query.since {
                Some(since) => m.updated_at >= since,
                None => true,
            })
            .cloned()
            .collect();

        // Prioritize merchant episodic matches first, then newest updated_at
        results.sort_by(|a, b| {
            let a_ep = matches!(a.memory_type, MemoryType::Episodic);
            let b_ep = matches!(b.memory_type, MemoryType::Episodic);
            b_ep.cmp(&a_ep)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });
        results.truncate(limit);
        Ok(results)
    }

    async fn delete_memory(
        &self,
        scope: &ExecutionScope,
        memory_id: &str,
    ) -> Result<bool, InferenceError> {
        let caller_mid = scope.normalized_merchant_id()?;
        let mut guard = self.memories.lock().unwrap();
        if let Some(existing) = guard.get(memory_id) {
            if existing.memory_type == MemoryType::Episodic
                && existing.verification_status == VerificationStatus::Verified
            {
                return Err(InferenceError::new(
                    InferenceErrorCategory::Forbidden,
                    "Verified episodic memories are immutable and cannot be deleted",
                ));
            }
            match (existing.scope, caller_mid) {
                (MemoryScope::Merchant, Some(mid))
                    if existing.merchant_id.as_deref() == Some(mid) =>
                {
                    guard.remove(memory_id);
                    return Ok(true);
                }
                (MemoryScope::System, None) => {
                    guard.remove(memory_id);
                    return Ok(true);
                }
                _ => {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::Forbidden,
                        "Caller cannot delete memory outside its tenant scope",
                    ));
                }
            }
        }
        Ok(false)
    }

    async fn purge_expired(&self) -> Result<usize, InferenceError> {
        let now = Utc::now();
        let mut mem_guard = self.memories.lock().unwrap();
        let mem_before = mem_guard.len();
        mem_guard.retain(|_, m| !m.is_expired(now));
        let mem_purged = mem_before.saturating_sub(mem_guard.len());
        drop(mem_guard);

        let mut eval_guard = self.evaluations.lock().unwrap();
        let eval_before = eval_guard.len();
        eval_guard.retain(|_, e| !e.is_expired(now));
        let eval_purged = eval_before.saturating_sub(eval_guard.len());

        Ok(mem_purged + eval_purged)
    }

    async fn record_evaluation(
        &self,
        eval: OutcomeEvaluationRecord,
    ) -> Result<OutcomeEvaluationRecord, InferenceError> {
        let mut guard = self.evaluations.lock().unwrap();
        guard.insert(eval.evaluation_id.clone(), eval.clone());
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
        let now = Utc::now();
        let guard = self.evaluations.lock().unwrap();
        match guard.get(evaluation_id) {
            Some(ev) => {
                if ev.merchant_id != caller_mid {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::Forbidden,
                        "Evaluation does not belong to authenticated merchant",
                    ));
                }
                if ev.is_expired(now) {
                    return Ok(None);
                }
                Ok(Some(ev.clone()))
            }
            None => Ok(None),
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

        let mut eval_guard = self.evaluations.lock().unwrap();
        let eval = eval_guard.get_mut(proof.evaluation_id()).ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::NotFound,
                "Evaluation record not found",
            )
        })?;
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

        let verified_outcome_state = proof.authoritative_state();
        let accuracy = proof.accuracy();
        if accuracy == OutcomeAccuracy::Pending {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Cannot promote outcome with Pending accuracy",
            ));
        }

        // Idempotency: repeated verification with same outcome returns existing record without duplicating memory
        if eval.verified_at.is_some() {
            if eval.verified_outcome_state == Some(verified_outcome_state)
                && eval.accuracy == accuracy
            {
                let existing_mem = eval.promoted_memory_id.as_ref().and_then(|mid| {
                    let mem_guard = self.memories.lock().unwrap();
                    mem_guard.get(mid).cloned()
                });
                return Ok((eval.clone(), existing_mem));
            }
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Evaluation was already verified with a different outcome",
            ));
        }

        // Only OutcomeAccuracy::Correct may create a verified episodic memory;
        // PartiallyCorrect and Incorrect remain evaluations without episodic promotion.
        let mut res_guard = self.resolutions.lock().unwrap();
        let existing_res_id = res_guard
            .values()
            .find(|r| r.evaluation_id == eval.evaluation_id)
            .map(|r| r.resolution_id.clone());
        let resolution_id =
            existing_res_id.unwrap_or_else(|| format!("res_{}", Uuid::new_v4().simple()));

        let will_promote = accuracy == OutcomeAccuracy::Correct;
        let promoted = if will_promote {
            let mem_id = format!("mem_ep_{}", Uuid::new_v4().simple());
            let record = MemoryRecord {
                memory_id: mem_id,
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
            Some(record)
        } else {
            None
        };

        if let Some(ref rec) = promoted {
            let mut mem_guard = self.memories.lock().unwrap();
            mem_guard.insert(rec.memory_id.clone(), rec.clone());
            eval.promoted_memory_id = Some(rec.memory_id.clone());
        }

        eval.verified_outcome_state = Some(verified_outcome_state);
        eval.accuracy = accuracy;
        eval.verified_at = Some(now);
        let updated_eval = eval.clone();

        let target_status = if accuracy == OutcomeAccuracy::Incorrect {
            ResolutionStatus::Rejected
        } else {
            ResolutionStatus::Verified
        };
        let res_entry =
            res_guard
                .entry(resolution_id.clone())
                .or_insert_with(|| TrustedResolutionRecord {
                    resolution_id: resolution_id.clone(),
                    investigation_id: updated_eval.investigation_id.clone(),
                    evaluation_id: updated_eval.evaluation_id.clone(),
                    merchant_id: updated_eval.merchant_id.clone(),
                    payment_reference: updated_eval.payment_reference.clone(),
                    provider: updated_eval.provider.clone(),
                    proposed_root_cause: updated_eval.recommended_root_cause.clone(),
                    status: ResolutionStatus::Pending,
                    accuracy: OutcomeAccuracy::Pending,
                    authoritative_state: Some(updated_eval.observed_authoritative_state),
                    verification_method: None,
                    evidence_refs: updated_eval.supporting_evidence_refs.clone(),
                    evidence_provenance: Vec::new(),
                    missing_evidence: Vec::new(),
                    verification_summary: None,
                    rejection_reason: None,
                    promoted_memory_id: None,
                    created_at: updated_eval.created_at,
                    updated_at: now,
                    verified_at: None,
                    expires_at: updated_eval.expires_at,
                });
        let prev_status = res_entry.status;
        res_entry.status = target_status;
        res_entry.accuracy = accuracy;
        res_entry.authoritative_state = Some(verified_outcome_state);
        res_entry.verification_method = Some(proof.verification_method().to_string());
        res_entry.evidence_refs = proof.evidence_refs().to_vec();
        res_entry.evidence_provenance = proof.evidence_provenance().to_vec();
        res_entry.missing_evidence.clear();
        res_entry.verification_summary = Some(proof.verified_resolution().to_string());
        res_entry.rejection_reason = if target_status == ResolutionStatus::Rejected {
            Some(proof.verified_resolution().to_string())
        } else {
            None
        };
        res_entry.promoted_memory_id = updated_eval.promoted_memory_id.clone();
        res_entry.updated_at = now;
        res_entry.verified_at = Some(now);
        drop(res_guard);

        let mut audit_guard = self.resolution_audits.lock().unwrap();
        audit_guard
            .entry(resolution_id.clone())
            .or_default()
            .push(ResolutionAuditEvent {
                audit_id: format!("raud_{}", Uuid::new_v4().simple()),
                resolution_id,
                evaluation_id: updated_eval.evaluation_id.clone(),
                merchant_id: updated_eval.merchant_id.clone(),
                from_status: Some(prev_status),
                to_status: target_status,
                actor: "system:deterministic_verifier".to_string(),
                verification_method: proof.verification_method().to_string(),
                reason: proof.verified_resolution().to_string(),
                evidence_refs: proof.evidence_refs().to_vec(),
                evidence_provenance: proof.evidence_provenance().to_vec(),
                promoted_memory_id: updated_eval.promoted_memory_id.clone(),
                created_at: now,
            });
        drop(audit_guard);
        drop(eval_guard);

        Ok((updated_eval, promoted))
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
        let mut guard = self.evaluations.lock().unwrap();
        let before = guard.len();
        guard.retain(|_, ev| ev.merchant_id != caller_mid);
        Ok(before.saturating_sub(guard.len()))
    }

    async fn record_resolution(
        &self,
        resolution: TrustedResolutionRecord,
        initial_audit: ResolutionAuditEvent,
    ) -> Result<TrustedResolutionRecord, InferenceError> {
        let mut res_guard = self.resolutions.lock().unwrap();
        res_guard.insert(resolution.resolution_id.clone(), resolution.clone());
        drop(res_guard);
        let mut audit_guard = self.resolution_audits.lock().unwrap();
        audit_guard
            .entry(resolution.resolution_id.clone())
            .or_default()
            .push(initial_audit);
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
        let res_guard = self.resolutions.lock().unwrap();
        let Some(res) = res_guard.get(resolution_id) else {
            return Ok(None);
        };
        if res.merchant_id != caller_mid {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Trusted resolution does not belong to authenticated merchant",
            ));
        }
        let cloned_res = res.clone();
        drop(res_guard);
        let audit_guard = self.resolution_audits.lock().unwrap();
        let audits = audit_guard.get(resolution_id).cloned().unwrap_or_default();
        Ok(Some((cloned_res, audits)))
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
        let lim = limit.clamp(1, 100);
        let clean_pref = payment_reference.map(str::trim).filter(|s| !s.is_empty());
        let res_guard = self.resolutions.lock().unwrap();
        let mut items: Vec<TrustedResolutionRecord> = res_guard
            .values()
            .filter(|r| r.merchant_id == caller_mid)
            .filter(|r| status.is_none_or(|st| r.status == st))
            .filter(|r| clean_pref.is_none_or(|p| r.payment_reference == p))
            .cloned()
            .collect();
        items.sort_by_key(|b| std::cmp::Reverse(b.updated_at));
        items.truncate(lim);
        Ok(items)
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
        let mut res_guard = self.resolutions.lock().unwrap();
        let res = res_guard.get_mut(resolution_id).ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::NotFound,
                "Trusted resolution record not found",
            )
        })?;
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
        res.missing_evidence = missing_evidence;
        res.updated_at = now;
        let updated = res.clone();
        drop(res_guard);

        let mut audit_guard = self.resolution_audits.lock().unwrap();
        audit_guard
            .entry(resolution_id.to_string())
            .or_default()
            .push(ResolutionAuditEvent {
                audit_id: format!("raud_{}", Uuid::new_v4().simple()),
                resolution_id: resolution_id.to_string(),
                evaluation_id: updated.evaluation_id.clone(),
                merchant_id: updated.merchant_id.clone(),
                from_status: Some(prev_status),
                to_status: new_status,
                actor: "system:deterministic_verifier".to_string(),
                verification_method: "deterministic_webhook_store_v1".to_string(),
                reason: reason.to_string(),
                evidence_refs: updated.evidence_refs.clone(),
                evidence_provenance: updated.evidence_provenance.clone(),
                promoted_memory_id: None,
                created_at: now,
            });
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{seed_system_knowledge, MemoryRetriever, WorkingMemory};
    use crate::payment_context::{build_payment_intelligence_context, CanonicalPaymentState};
    use esa_core::provider::PaymentAttemptRecord;
    use std::sync::Arc;

    fn scope_for(merchant_id: &str) -> ExecutionScope {
        ExecutionScope::MerchantScoped {
            merchant_id: merchant_id.to_string(),
        }
    }

    fn sample_context(merchant_id: &str) -> crate::payment_context::PaymentIntelligenceContext {
        let now = Utc::now();
        let attempt = PaymentAttemptRecord {
            transaction_id: "tx_mem_1".to_string(),
            merchant_id: merchant_id.to_string(),
            provider: "razorpay".to_string(),
            provider_order_id: Some("order_mem_1".to_string()),
            payment_reference: Some("pay_mem_1".to_string()),
            amount_minor: 10000,
            currency: "INR".to_string(),
            state: "authorized".to_string(),
            created_at: now,
            updated_at: now,
        };
        build_payment_intelligence_context(
            &scope_for(merchant_id),
            &attempt,
            None,
            &[],
            &[],
            None,
            None,
        )
        .unwrap()
    }

    fn make_episodic(id: &str, merchant_id: &str, verified: VerificationStatus) -> MemoryRecord {
        let now = Utc::now();
        MemoryRecord {
            memory_id: id.to_string(),
            memory_type: MemoryType::Episodic,
            scope: MemoryScope::Merchant,
            merchant_id: Some(merchant_id.to_string()),
            provider: Some("razorpay".to_string()),
            incident_category: "AUTHORIZATION_NOT_CAPTURED".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Authorized),
            title: format!("Authorized payment incident for {}", merchant_id),
            summary: format!("Verified incident for {}", merchant_id),
            resolution: Some("Check capture webhook".to_string()),
            source_refs: vec!["payment:pay_mem_1".to_string()],
            verification_status: verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: Some(now + chrono::Duration::days(30)),
            provenance: None,
        }
    }

    #[test]
    fn test_working_memory_isolation_and_clearing() {
        let ctx_a = sample_context("merchant_a");
        let mut wm = WorkingMemory::new(&ctx_a).unwrap();
        wm.record_step("step1", "Observed authorized state");
        assert!(wm.active);
        assert!(!wm.observed_evidence.is_empty());
        assert_eq!(wm.step_notes.len(), 1);

        wm.clear();
        assert!(!wm.active);
        assert!(wm.observed_evidence.is_empty());
        assert!(wm.step_notes.is_empty());
        wm.record_step("step2", "Should not be recorded");
        assert!(wm.step_notes.is_empty());
    }

    #[tokio::test]
    async fn test_episodic_persistence_and_unverified_rejected() {
        let store = InMemoryPaymentMemoryStore::new();
        // 1. Generic store_memory strictly rejects Episodic memory creation even if marked Verified
        let valid = make_episodic("mem_ep_1", "merchant_a", VerificationStatus::Verified);
        let err_valid = store.store_memory(valid).await.unwrap_err();
        assert_eq!(err_valid.category, InferenceErrorCategory::Forbidden);

        // 2. Unverified episodic memory is rejected by sanitize_and_validate
        let unverified = make_episodic("mem_ep_2", "merchant_a", VerificationStatus::Unverified);
        let err = store.store_memory(unverified).await.unwrap_err();
        assert_eq!(err.category, InferenceErrorCategory::InvalidResponse);
    }

    #[tokio::test]
    async fn test_semantic_provenance_and_procedural_versioning() {
        let store = InMemoryPaymentMemoryStore::new();
        let inserted = seed_system_knowledge(&store).await.unwrap();
        assert!(inserted >= 4);

        let res = store
            .query_memories(&MemoryQuery {
                scope: ExecutionScope::System,
                memory_types: vec![MemoryType::Semantic, MemoryType::Procedural],
                provider: None,
                incident_category: None,
                lifecycle_state: None,
                since: None,
                limit: 20,
            })
            .await
            .unwrap();

        let semantic: Vec<_> = res
            .iter()
            .filter(|r| r.memory_type == MemoryType::Semantic)
            .collect();
        let procedural: Vec<_> = res
            .iter()
            .filter(|r| r.memory_type == MemoryType::Procedural)
            .collect();

        assert!(!semantic.is_empty());
        assert!(semantic.iter().all(|r| !r.source_refs.is_empty()));
        assert!(!procedural.is_empty());
        assert!(procedural.iter().all(|r| r.version >= 1));
    }

    #[tokio::test]
    async fn test_merchant_a_vs_merchant_b_isolation() {
        let store = InMemoryPaymentMemoryStore::new();
        store.insert_test_memory(make_episodic(
            "mem_a",
            "merchant_a",
            VerificationStatus::Verified,
        ));
        store.insert_test_memory(make_episodic(
            "mem_b",
            "merchant_b",
            VerificationStatus::Verified,
        ));

        let q_a = store
            .query_memories(&MemoryQuery {
                scope: scope_for("merchant_a"),
                memory_types: vec![MemoryType::Episodic],
                provider: None,
                incident_category: None,
                lifecycle_state: None,
                since: None,
                limit: 10,
            })
            .await
            .unwrap();

        assert_eq!(q_a.len(), 1);
        assert_eq!(q_a[0].merchant_id.as_deref(), Some("merchant_a"));
    }

    #[tokio::test]
    async fn test_system_memory_cannot_hold_merchant_records() {
        let store = InMemoryPaymentMemoryStore::new();
        let mut bad = make_episodic("mem_bad_sys", "merchant_a", VerificationStatus::Verified);
        bad.scope = MemoryScope::System;
        let err = store.store_memory(bad).await.unwrap_err();
        assert_eq!(err.category, InferenceErrorCategory::InvalidScope);

        let mut bad_semantic =
            make_episodic("mem_bad_sem", "merchant_a", VerificationStatus::Verified);
        bad_semantic.memory_type = MemoryType::Semantic;
        let err2 = store.store_memory(bad_semantic).await.unwrap_err();
        assert_eq!(err2.category, InferenceErrorCategory::InvalidScope);
    }

    #[tokio::test]
    async fn test_invalid_scope_rejected() {
        let store = InMemoryPaymentMemoryStore::new();
        let err = store
            .query_memories(&MemoryQuery {
                scope: ExecutionScope::MerchantScoped {
                    merchant_id: "   ".to_string(),
                },
                memory_types: vec![MemoryType::Episodic],
                provider: None,
                incident_category: None,
                lifecycle_state: None,
                since: None,
                limit: 10,
            })
            .await
            .unwrap_err();
        assert_eq!(err.category, InferenceErrorCategory::InvalidScope);
    }

    #[tokio::test]
    async fn test_expired_memory_filtered_and_purged() {
        let store = InMemoryPaymentMemoryStore::new();
        let mut expired = make_episodic("mem_exp", "merchant_a", VerificationStatus::Verified);
        expired.expires_at = Some(Utc::now() - chrono::Duration::minutes(5));
        store.insert_test_memory(expired);

        let q = store
            .query_memories(&MemoryQuery {
                scope: scope_for("merchant_a"),
                memory_types: vec![MemoryType::Episodic],
                provider: None,
                incident_category: None,
                lifecycle_state: None,
                since: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert!(q.is_empty());

        let purged = store.purge_expired().await.unwrap();
        assert_eq!(purged, 1);
    }

    #[tokio::test]
    async fn test_deletion_and_retention() {
        let store = InMemoryPaymentMemoryStore::new();
        store.insert_test_memory(make_episodic(
            "mem_del_1",
            "merchant_a",
            VerificationStatus::Verified,
        ));

        // Merchant B cannot delete Merchant A's memory
        let err_b = store
            .delete_memory(&scope_for("merchant_b"), "mem_del_1")
            .await
            .unwrap_err();
        assert_eq!(err_b.category, InferenceErrorCategory::Forbidden);

        // Promoted verified episodic memory is immutable and cannot be deleted by Merchant A either
        let err_a = store
            .delete_memory(&scope_for("merchant_a"), "mem_del_1")
            .await
            .unwrap_err();
        assert_eq!(err_a.category, InferenceErrorCategory::Forbidden);

        // System semantic memory can be deleted by system scope, but not by a merchant
        let sys_mem = MemoryRecord {
            memory_id: "mem_sys_1".to_string(),
            memory_type: MemoryType::Semantic,
            scope: MemoryScope::System,
            merchant_id: None,
            provider: None,
            incident_category: "PAYMENT_FAILURE".to_string(),
            lifecycle_state: None,
            title: "System failure guidance".to_string(),
            summary: "Guidance on handling failures".to_string(),
            resolution: None,
            source_refs: vec![],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            expires_at: None,
            provenance: None,
        };
        store.store_memory(sys_mem).await.unwrap();

        let err_sys_by_merchant = store
            .delete_memory(&scope_for("merchant_a"), "mem_sys_1")
            .await
            .unwrap_err();
        assert_eq!(
            err_sys_by_merchant.category,
            InferenceErrorCategory::Forbidden
        );

        let deleted_sys = store
            .delete_memory(&ExecutionScope::System, "mem_sys_1")
            .await
            .unwrap();
        assert!(deleted_sys);
    }

    #[tokio::test]
    async fn test_retriever_uses_merchant_episodic_and_system_knowledge() {
        let store = Arc::new(InMemoryPaymentMemoryStore::new());
        seed_system_knowledge(store.as_ref()).await.unwrap();
        store.insert_test_memory(make_episodic(
            "mem_ret_a",
            "merchant_a",
            VerificationStatus::Verified,
        ));
        store.insert_test_memory(make_episodic(
            "mem_ret_b",
            "merchant_b",
            VerificationStatus::Verified,
        ));

        let retriever = MemoryRetriever::new(store);
        let ctx_a = sample_context("merchant_a");
        let retrieved = retriever.retrieve_for_context(&ctx_a, 10).await.unwrap();
        assert!(retrieved.iter().any(|m| m.memory_id == "mem_ret_a"));
        assert!(!retrieved.iter().any(|m| m.memory_id == "mem_ret_b"));
        assert!(retrieved
            .iter()
            .any(|m| m.memory_type == MemoryType::Semantic
                || m.memory_type == MemoryType::Procedural));
    }

    #[tokio::test]
    async fn test_same_id_collision_and_cross_merchant_overwrite_rejected() {
        let store = InMemoryPaymentMemoryStore::new();
        let rec_a = make_episodic("shared_mem_id", "merchant_a", VerificationStatus::Verified);
        store.insert_test_memory(rec_a);

        // Merchant B trying to overwrite Merchant A's memory_id is rejected
        let rec_b = make_episodic("shared_mem_id", "merchant_b", VerificationStatus::Verified);
        let err = store.store_memory(rec_b).await.unwrap_err();
        assert_eq!(err.category, InferenceErrorCategory::Forbidden);

        // Merchant A's original record remains intact
        let res_a = store
            .query_memories(&MemoryQuery {
                scope: scope_for("merchant_a"),
                memory_types: vec![MemoryType::Episodic],
                provider: None,
                incident_category: None,
                lifecycle_state: None,
                since: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(res_a.len(), 1);
        assert_eq!(res_a[0].merchant_id.as_deref(), Some("merchant_a"));
    }

    #[tokio::test]
    async fn test_evaluation_ownership_system_scope_denied_and_promotion_idempotency() {
        use crate::payment_context::CanonicalPaymentState;
        use esa_core::payment_state::{PaymentEntity, PaymentState};
        use esa_state::InMemoryWebhookStore;

        let store = InMemoryPaymentMemoryStore::new();
        let wh_store = InMemoryWebhookStore::new();
        let now = Utc::now();

        let eval = OutcomeEvaluationRecord::new_pending(
            "inv_1".to_string(),
            "merchant_a".to_string(),
            "pay_1".to_string(),
            "razorpay".to_string(),
            "AUTHORIZATION_NOT_CAPTURED".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            true,
            CanonicalPaymentState::Authorized,
        );
        let eval_id = eval.evaluation_id.clone();
        store.record_evaluation(eval.clone()).await.unwrap();

        let pending_attempt = PaymentAttemptRecord {
            transaction_id: "tx_1".to_string(),
            merchant_id: "merchant_a".to_string(),
            provider: "razorpay".to_string(),
            provider_order_id: Some("order_1".to_string()),
            payment_reference: Some("pay_1".to_string()),
            amount_minor: 10000,
            currency: "INR".to_string(),
            state: "authorized".to_string(),
            created_at: now,
            updated_at: now,
        };
        wh_store
            .record_payment_attempt(&pending_attempt)
            .await
            .unwrap();

        // Cross-merchant read rejected
        let err_b = store
            .get_evaluation(&scope_for("merchant_b"), &eval_id)
            .await
            .unwrap_err();
        assert_eq!(err_b.category, InferenceErrorCategory::Forbidden);

        // System scope does NOT implicitly grant cross-merchant evaluation access
        let err_sys = store
            .get_evaluation(&ExecutionScope::System, &eval_id)
            .await
            .unwrap_err();
        assert_eq!(err_sys.category, InferenceErrorCategory::Forbidden);

        // Non-terminal authoritative payment state keeps evaluation pending (cannot promote)
        let err_pending = verify_outcome_from_webhook_store(
            &wh_store,
            &store,
            &scope_for("merchant_a"),
            &eval_id,
        )
        .await
        .unwrap_err();
        assert_eq!(
            err_pending.category,
            InferenceErrorCategory::InvalidResponse
        );

        // Authoritative attempt transitions to captured
        let captured_attempt = PaymentAttemptRecord {
            state: "captured".to_string(),
            ..pending_attempt.clone()
        };
        let captured_entity = PaymentEntity {
            payment_id: "pay_1".to_string(),
            state: PaymentState::Captured,
            amount_minor: 10000,
            currency: "INR".to_string(),
            refunded_amount_minor: 0,
            events_seen: vec![],
            refunds: vec![],
            created_at: now,
            updated_at: now,
        };

        // PartiallyCorrect (AUTHORIZATION_NOT_CAPTURED -> Captured) updates evaluation but creates NO episodic memory
        let proof_partial = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval,
            &captured_attempt,
            Some(&captured_entity),
            &[],
            &[],
        )
        .unwrap();
        let (ev_part, mem_part) = store
            .verify_and_promote_outcome(&scope_for("merchant_a"), proof_partial)
            .await
            .unwrap();
        assert_eq!(ev_part.accuracy, OutcomeAccuracy::PartiallyCorrect);
        assert!(ev_part.promoted_memory_id.is_none());
        assert!(mem_part.is_none());

        // Correct evaluation (HEALTHY_COMPLETED -> Captured) promotes exactly 1 episodic memory
        let eval_correct = OutcomeEvaluationRecord::new_pending(
            "inv_corr".to_string(),
            "merchant_a".to_string(),
            "pay_1".to_string(),
            "razorpay".to_string(),
            "HEALTHY_COMPLETED".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            true,
            CanonicalPaymentState::Captured,
        );
        let eval_corr_id = eval_correct.evaluation_id.clone();
        store.record_evaluation(eval_correct.clone()).await.unwrap();

        let proof1 = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval_correct,
            &captured_attempt,
            Some(&captured_entity),
            &[],
            &[],
        )
        .unwrap();
        let (ev1, mem1) = store
            .verify_and_promote_outcome(&scope_for("merchant_a"), proof1.clone())
            .await
            .unwrap();
        assert_eq!(ev1.accuracy, OutcomeAccuracy::Correct);
        assert!(mem1.is_some());
        assert!(ev1.promoted_memory_id.is_some());

        // Repeated verification with same authoritative outcome is idempotent and creates NO duplicate memory
        let (ev2, mem2) = store
            .verify_and_promote_outcome(&scope_for("merchant_a"), proof1)
            .await
            .unwrap();
        assert_eq!(ev2.promoted_memory_id, ev1.promoted_memory_id);
        assert_eq!(mem2.unwrap().memory_id, mem1.unwrap().memory_id);

        let all_a = store
            .query_memories(&MemoryQuery {
                scope: scope_for("merchant_a"),
                memory_types: vec![MemoryType::Episodic],
                provider: None,
                incident_category: None,
                lifecycle_state: None,
                since: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(all_a.len(), 1);

        // Conflicting authoritative outcome re-verification is rejected
        let failed_entity = PaymentEntity {
            state: PaymentState::Failed,
            ..captured_entity
        };
        let proof_conflict = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval_correct,
            &captured_attempt,
            Some(&failed_entity),
            &[],
            &[],
        )
        .unwrap();
        let err_conflict = store
            .verify_and_promote_outcome(&scope_for("merchant_a"), proof_conflict)
            .await
            .unwrap_err();
        assert_eq!(
            err_conflict.category,
            InferenceErrorCategory::InvalidResponse
        );
        let _ = eval_corr_id;
    }

    #[tokio::test]
    async fn test_expired_evaluation_cannot_be_verified_even_directly() {
        use crate::payment_context::CanonicalPaymentState;
        use esa_core::payment_state::{PaymentEntity, PaymentState};

        let store = InMemoryPaymentMemoryStore::new();
        let now = Utc::now();
        let mut eval = OutcomeEvaluationRecord::new_pending(
            "inv_exp_dir".to_string(),
            "merchant_a".to_string(),
            "pay_exp_dir".to_string(),
            "razorpay".to_string(),
            "HEALTHY_COMPLETED".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            true,
            CanonicalPaymentState::Authorized,
        );
        let attempt = PaymentAttemptRecord {
            transaction_id: "tx_exp_dir".to_string(),
            merchant_id: "merchant_a".to_string(),
            provider: "razorpay".to_string(),
            provider_order_id: None,
            payment_reference: Some("pay_exp_dir".to_string()),
            amount_minor: 10000,
            currency: "INR".to_string(),
            state: "captured".to_string(),
            created_at: now,
            updated_at: now,
        };
        let entity = PaymentEntity {
            payment_id: "pay_exp_dir".to_string(),
            state: PaymentState::Captured,
            amount_minor: 10000,
            currency: "INR".to_string(),
            refunded_amount_minor: 0,
            events_seen: vec![],
            refunds: vec![],
            created_at: now,
            updated_at: now,
        };
        // Construct proof before expiry, then store an already-expired evaluation
        let proof = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval,
            &attempt,
            Some(&entity),
            &[],
            &[],
        )
        .unwrap();
        eval.expires_at = Some(now - chrono::Duration::seconds(5));
        store.record_evaluation(eval).await.unwrap();

        // Direct verify_and_promote_outcome rejects expired evaluation before any promotion
        let err = store
            .verify_and_promote_outcome(&scope_for("merchant_a"), proof)
            .await
            .unwrap_err();
        assert_eq!(err.category, InferenceErrorCategory::InvalidResponse);
    }

    #[tokio::test]
    async fn test_evaluation_retention_purge_and_merchant_deletion() {
        let store = InMemoryPaymentMemoryStore::new();
        let mut expired_eval = OutcomeEvaluationRecord::new_pending(
            "inv_exp".to_string(),
            "merchant_a".to_string(),
            "pay_exp".to_string(),
            "razorpay".to_string(),
            "AUTHORIZATION_NOT_CAPTURED".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            true,
            CanonicalPaymentState::Authorized,
        );
        expired_eval.expires_at = Some(Utc::now() - chrono::Duration::minutes(1));
        let exp_id = expired_eval.evaluation_id.clone();
        store.record_evaluation(expired_eval).await.unwrap();

        // Expired evaluation is hidden on get and purged by purge_expired
        assert!(store
            .get_evaluation(&scope_for("merchant_a"), &exp_id)
            .await
            .unwrap()
            .is_none());
        assert_eq!(store.purge_expired().await.unwrap(), 1);

        // Active evaluation deleted by merchant_a only
        let active_eval = OutcomeEvaluationRecord::new_pending(
            "inv_act".to_string(),
            "merchant_a".to_string(),
            "pay_act".to_string(),
            "razorpay".to_string(),
            "AUTHORIZATION_NOT_CAPTURED".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            true,
            CanonicalPaymentState::Authorized,
        );
        store.record_evaluation(active_eval).await.unwrap();
        assert_eq!(
            store
                .delete_evaluations_for_merchant(&scope_for("merchant_b"))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .delete_evaluations_for_merchant(&scope_for("merchant_a"))
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn test_diagnosis_level_verification_requires_specific_evidence_not_terminal_state() {
        use esa_core::events::PaymentEventType;
        use esa_state::{InMemoryWebhookStore, WebhookEventParams};

        let mem_store = InMemoryPaymentMemoryStore::new();
        let wh_store = InMemoryWebhookStore::new();
        let now = Utc::now();

        // Seed a Captured payment attempt + entity in WebhookStore (terminal state = Captured)
        wh_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_diag_1".to_string(),
                merchant_id: "merchant_a".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_diag_1".to_string()),
                payment_reference: Some("pay_diag_1".to_string()),
                amount_minor: 50000,
                currency: "INR".to_string(),
                state: "captured".to_string(),
                created_at: now - chrono::Duration::minutes(5),
                updated_at: now,
            })
            .await
            .unwrap();
        let payload = serde_json::json!({"event": "payment.captured"});
        wh_store
            .process_webhook_event(&WebhookEventParams {
                provider: "razorpay",
                provider_event_id: "evt_cap_once",
                event_type: PaymentEventType::PaymentCaptured,
                payment_id: "pay_diag_1",
                amount_minor: 50000,
                currency: "INR",
                refund_id: None,
                refund_amount_minor: None,
                refund_status: None,
                payload: &payload,
            })
            .await
            .unwrap();

        // Diagnosis claims DUPLICATE_WEBHOOK_SUPPRESSED even though NO duplicate webhook exists!
        let eval_dup = OutcomeEvaluationRecord::new_pending(
            "inv_diag_1".to_string(),
            "merchant_a".to_string(),
            "pay_diag_1".to_string(),
            "razorpay".to_string(),
            "DUPLICATE_WEBHOOK_SUPPRESSED".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec!["webhook:evt_cap_once".to_string()]);
        let eval_id = eval_dup.evaluation_id.clone();
        mem_store.record_evaluation(eval_dup).await.unwrap();

        // Terminal Captured state alone MUST NOT verify DUPLICATE_WEBHOOK_SUPPRESSED
        let err = verify_outcome_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &eval_id,
        )
        .await
        .unwrap_err();
        assert_eq!(err.category, InferenceErrorCategory::InvalidResponse);

        // Evaluation stays Pending and zero episodic memories were created
        let still_pending = mem_store
            .get_evaluation(&scope_for("merchant_a"), &eval_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(still_pending.accuracy, OutcomeAccuracy::Pending);
        assert!(still_pending.promoted_memory_id.is_none());

        // Diagnosis claiming REFUND_RESERVATION_PENDING fails when no pending reservation exists
        let eval_rf = OutcomeEvaluationRecord::new_pending(
            "inv_diag_2".to_string(),
            "merchant_a".to_string(),
            "pay_diag_1".to_string(),
            "razorpay".to_string(),
            "REFUND_RESERVATION_PENDING".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec!["refund:rfnd_diag_1".to_string()]);
        let eval_rf_id = eval_rf.evaluation_id.clone();
        mem_store.record_evaluation(eval_rf).await.unwrap();
        let err_no_rf = verify_outcome_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &eval_rf_id,
        )
        .await
        .unwrap_err();
        assert_eq!(err_no_rf.category, InferenceErrorCategory::InvalidResponse);

        // Now create an authoritative pending refund reservation in WebhookStore
        wh_store
            .reserve_refund("razorpay", "pay_diag_1", "rfnd_diag_1", 10000, "INR")
            .await
            .unwrap();

        // With actual pending reservation record present, verification succeeds and promotes once
        let (verified, promoted) = verify_outcome_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &eval_rf_id,
        )
        .await
        .unwrap();
        assert_eq!(verified.accuracy, OutcomeAccuracy::Correct);
        assert!(promoted.is_some());
    }

    #[tokio::test]
    async fn test_strict_diagnosis_verification_edges_and_incorrect_no_promotion() {
        use esa_core::events::PaymentEventType;
        use esa_core::payment_state::{PaymentEntity, PaymentState};
        use esa_state::{InMemoryWebhookStore, WebhookEventParams};

        let mem_store = InMemoryPaymentMemoryStore::new();
        let wh_store = InMemoryWebhookStore::new();
        let now = Utc::now();

        // 1. Empty webhook inbox alone cannot verify WEBHOOK_DELAYED_OR_MISSING
        wh_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_empty_wh".to_string(),
                merchant_id: "merchant_a".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: None,
                payment_reference: Some("pay_empty_wh".to_string()),
                amount_minor: 10000,
                currency: "INR".to_string(),
                state: "captured".to_string(),
                created_at: now - chrono::Duration::minutes(10),
                updated_at: now,
            })
            .await
            .unwrap();
        let eval_empty_wh = OutcomeEvaluationRecord::new_pending(
            "inv_empty_wh".to_string(),
            "merchant_a".to_string(),
            "pay_empty_wh".to_string(),
            "razorpay".to_string(),
            "WEBHOOK_DELAYED_OR_MISSING".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec!["attempt:tx_empty_wh".to_string()]);
        let eval_empty_id = eval_empty_wh.evaluation_id.clone();
        mem_store.record_evaluation(eval_empty_wh).await.unwrap();
        let err_empty = verify_outcome_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &eval_empty_id,
        )
        .await
        .unwrap_err();
        assert_eq!(err_empty.category, InferenceErrorCategory::InvalidResponse);

        // 2. Failed payment WITHOUT provider-originated failure event keeps PAYMENT_FAILED_AT_PROVIDER Pending
        wh_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_fail_no_ev".to_string(),
                merchant_id: "merchant_a".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: None,
                payment_reference: Some("pay_fail_no_ev".to_string()),
                amount_minor: 10000,
                currency: "INR".to_string(),
                state: "failed".to_string(),
                created_at: now - chrono::Duration::minutes(5),
                updated_at: now,
            })
            .await
            .unwrap();
        let eval_fail_no_ev = OutcomeEvaluationRecord::new_pending(
            "inv_fail_no_ev".to_string(),
            "merchant_a".to_string(),
            "pay_fail_no_ev".to_string(),
            "razorpay".to_string(),
            "PAYMENT_FAILED_AT_PROVIDER".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Failed,
        )
        .with_evidence_refs(vec!["state:failed".to_string()]);
        let eval_fail_no_ev_id = eval_fail_no_ev.evaluation_id.clone();
        mem_store.record_evaluation(eval_fail_no_ev).await.unwrap();
        let err_no_ev = verify_outcome_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &eval_fail_no_ev_id,
        )
        .await
        .unwrap_err();
        assert_eq!(err_no_ev.category, InferenceErrorCategory::InvalidResponse);

        // 3. Failed payment WITH durable provider payment.failed event promotes PAYMENT_FAILED_AT_PROVIDER
        let fail_payload = serde_json::json!({"event": "payment.failed"});
        wh_store
            .process_webhook_event(&WebhookEventParams {
                provider: "razorpay",
                provider_event_id: "evt_prov_fail_1",
                event_type: PaymentEventType::PaymentFailed,
                payment_id: "pay_fail_no_ev",
                amount_minor: 10000,
                currency: "INR",
                refund_id: None,
                refund_amount_minor: None,
                refund_status: None,
                payload: &fail_payload,
            })
            .await
            .unwrap();
        let eval_fail_with_ev = OutcomeEvaluationRecord::new_pending(
            "inv_fail_with_ev".to_string(),
            "merchant_a".to_string(),
            "pay_fail_no_ev".to_string(),
            "razorpay".to_string(),
            "PAYMENT_FAILED_AT_PROVIDER".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Failed,
        )
        .with_evidence_refs(vec!["webhook:evt_prov_fail_1".to_string()]);
        let eval_fail_with_ev_id = eval_fail_with_ev.evaluation_id.clone();
        mem_store
            .record_evaluation(eval_fail_with_ev)
            .await
            .unwrap();
        let (ev_fail_ok, mem_fail_ok) = verify_outcome_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &eval_fail_with_ev_id,
        )
        .await
        .unwrap();
        assert_eq!(ev_fail_ok.accuracy, OutcomeAccuracy::Correct);
        assert!(mem_fail_ok.is_some());

        // 4. Incorrect evaluation (HEALTHY_COMPLETED when payment Failed) creates NO episodic memory
        let eval_incorrect = OutcomeEvaluationRecord::new_pending(
            "inv_incorrect".to_string(),
            "merchant_a".to_string(),
            "pay_fail_no_ev".to_string(),
            "razorpay".to_string(),
            "HEALTHY_COMPLETED".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Failed,
        );
        let eval_inc_id = eval_incorrect.evaluation_id.clone();
        mem_store.record_evaluation(eval_incorrect).await.unwrap();
        let (ev_inc, mem_inc) = verify_outcome_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &eval_inc_id,
        )
        .await
        .unwrap();
        assert_eq!(ev_inc.accuracy, OutcomeAccuracy::Incorrect);
        assert!(mem_inc.is_none());

        // 5. PROVIDER_STATE_DISCREPANCY rejects non-matching/non-conflicting evidence refs
        let attempt_disc = PaymentAttemptRecord {
            transaction_id: "tx_disc".to_string(),
            merchant_id: "merchant_a".to_string(),
            provider: "razorpay".to_string(),
            provider_order_id: None,
            payment_reference: Some("pay_disc".to_string()),
            amount_minor: 10000,
            currency: "INR".to_string(),
            state: "authorized".to_string(),
            created_at: now,
            updated_at: now,
        };
        let entity_disc = PaymentEntity {
            payment_id: "pay_disc".to_string(),
            state: PaymentState::Captured,
            amount_minor: 10000,
            currency: "INR".to_string(),
            refunded_amount_minor: 0,
            events_seen: vec![],
            refunds: vec![],
            created_at: now,
            updated_at: now,
        };
        let bad_disc_eval = OutcomeEvaluationRecord::new_pending(
            "inv_bad_disc".to_string(),
            "merchant_a".to_string(),
            "pay_disc".to_string(),
            "razorpay".to_string(),
            "PROVIDER_STATE_DISCREPANCY".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec![
            "state:captured".to_string(),
            "attempt_state:captured".to_string(), // non-conflicting!
        ]);
        let err_bad_disc = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &bad_disc_eval,
            &attempt_disc,
            Some(&entity_disc),
            &[],
            &[],
        )
        .unwrap_err();
        assert_eq!(
            err_bad_disc.category,
            InferenceErrorCategory::InvalidResponse
        );
    }

    #[tokio::test]
    async fn test_evidence_provenance_refund_failed_prompt_refund_empty_refs_and_discrepancy() {
        use esa_core::payment_state::{PaymentEntity, PaymentState};
        use esa_state::{WebhookInboxRecord, WebhookStatus};

        let now = Utc::now();

        // 1. Failed attempt + processed RefundFailed webhook MUST NOT verify PAYMENT_FAILED_AT_PROVIDER
        let attempt_fail = PaymentAttemptRecord {
            transaction_id: "tx_rf_fail".to_string(),
            merchant_id: "merchant_a".to_string(),
            provider: "razorpay".to_string(),
            provider_order_id: None,
            payment_reference: Some("pay_rf_fail".to_string()),
            amount_minor: 10000,
            currency: "INR".to_string(),
            state: "failed".to_string(),
            created_at: now - chrono::Duration::hours(2),
            updated_at: now,
        };
        let wh_refund_failed = vec![WebhookInboxRecord {
            id: 1,
            provider: "razorpay".to_string(),
            provider_event_id: "evt_rf_fail".to_string(),
            event_type: "RefundFailed".to_string(),
            payment_id: Some("pay_rf_fail".to_string()),
            refund_id: Some("rfnd_1".to_string()),
            payload: serde_json::json!({"event": "refund.failed"}),
            status: WebhookStatus::Processed,
            error_message: None,
            received_at: now,
            processed_at: Some(now),
        }];
        let eval_pay_fail = OutcomeEvaluationRecord::new_pending(
            "inv_rf_fail".to_string(),
            "merchant_a".to_string(),
            "pay_rf_fail".to_string(),
            "razorpay".to_string(),
            "PAYMENT_FAILED_AT_PROVIDER".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Failed,
        )
        .with_evidence_refs(vec!["webhook:evt_rf_fail".to_string()]);
        let err_rf_fail = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval_pay_fail,
            &attempt_fail,
            None,
            &wh_refund_failed,
            &[],
        )
        .unwrap_err();
        assert_eq!(
            err_rf_fail.category,
            InferenceErrorCategory::InvalidResponse
        );

        // 2. Empty supporting_evidence_refs NEVER satisfies PAYMENT_FAILED_AT_PROVIDER even with valid PaymentFailed event
        let wh_pay_failed = vec![WebhookInboxRecord {
            id: 2,
            provider: "razorpay".to_string(),
            provider_event_id: "evt_pay_fail_ok".to_string(),
            event_type: "PaymentFailed".to_string(),
            payment_id: Some("pay_rf_fail".to_string()),
            refund_id: None,
            payload: serde_json::json!({"event": "payment.failed"}),
            status: WebhookStatus::Processed,
            error_message: None,
            received_at: now,
            processed_at: Some(now),
        }];
        let eval_empty_refs = OutcomeEvaluationRecord::new_pending(
            "inv_empty_refs".to_string(),
            "merchant_a".to_string(),
            "pay_rf_fail".to_string(),
            "razorpay".to_string(),
            "PAYMENT_FAILED_AT_PROVIDER".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Failed,
        ); // empty supporting_evidence_refs!
        let err_empty_refs = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval_empty_refs,
            &attempt_fail,
            None,
            &wh_pay_failed,
            &[],
        )
        .unwrap_err();
        assert_eq!(
            err_empty_refs.category,
            InferenceErrorCategory::InvalidResponse
        );

        // 3. Promptly delivered RefundProcessed webhook arriving 2h after attempt.created_at MUST NOT verify WEBHOOK_DELAYED_OR_MISSING
        let attempt_old = PaymentAttemptRecord {
            transaction_id: "tx_old".to_string(),
            merchant_id: "merchant_a".to_string(),
            provider: "razorpay".to_string(),
            provider_order_id: None,
            payment_reference: Some("pay_old".to_string()),
            amount_minor: 50000,
            currency: "INR".to_string(),
            state: "captured".to_string(),
            created_at: now - chrono::Duration::hours(2),
            updated_at: now,
        };
        let wh_prompt_refund = vec![WebhookInboxRecord {
            id: 3,
            provider: "razorpay".to_string(),
            provider_event_id: "evt_rf_prompt".to_string(),
            event_type: "RefundProcessed".to_string(),
            payment_id: Some("pay_old".to_string()),
            refund_id: Some("rfnd_prompt".to_string()),
            payload: serde_json::json!({
                "occurred_at": (now - chrono::Duration::seconds(5)).to_rfc3339()
            }),
            status: WebhookStatus::Processed,
            error_message: None,
            received_at: now,
            processed_at: Some(now),
        }];
        let eval_prompt_rf = OutcomeEvaluationRecord::new_pending(
            "inv_prompt_rf".to_string(),
            "merchant_a".to_string(),
            "pay_old".to_string(),
            "razorpay".to_string(),
            "WEBHOOK_DELAYED_OR_MISSING".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec!["webhook:evt_rf_prompt".to_string()]);
        let err_prompt_rf = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval_prompt_rf,
            &attempt_old,
            None,
            &wh_prompt_refund,
            &[],
        )
        .unwrap_err();
        assert_eq!(
            err_prompt_rf.category,
            InferenceErrorCategory::InvalidResponse
        );

        // 4. Payment webhook WITHOUT trustworthy occurred_at in payload fails closed (Pending)
        let wh_no_ts = vec![WebhookInboxRecord {
            id: 4,
            provider: "razorpay".to_string(),
            provider_event_id: "evt_no_ts".to_string(),
            event_type: "PaymentCaptured".to_string(),
            payment_id: Some("pay_old".to_string()),
            refund_id: None,
            payload: serde_json::json!({"event": "payment.captured"}),
            status: WebhookStatus::Processed,
            error_message: None,
            received_at: now,
            processed_at: Some(now),
        }];
        let eval_no_ts = OutcomeEvaluationRecord::new_pending(
            "inv_no_ts".to_string(),
            "merchant_a".to_string(),
            "pay_old".to_string(),
            "razorpay".to_string(),
            "WEBHOOK_DELAYED_OR_MISSING".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec!["webhook:evt_no_ts".to_string()]);
        assert_eq!(
            AuthoritativeOutcomeProof::from_authoritative_records(
                &scope_for("merchant_a"),
                &eval_no_ts,
                &attempt_old,
                None,
                &wh_no_ts,
                &[],
            )
            .unwrap_err()
            .category,
            InferenceErrorCategory::InvalidResponse
        );

        // 5. Genuinely delayed payment webhook (occurred_at 10m before received_at) with explicit citation -> verified
        let wh_genuinely_delayed = vec![WebhookInboxRecord {
            id: 5,
            provider: "razorpay".to_string(),
            provider_event_id: "evt_real_delay".to_string(),
            event_type: "PaymentCaptured".to_string(),
            payment_id: Some("pay_old".to_string()),
            refund_id: None,
            payload: serde_json::json!({
                "created_at": (now - chrono::Duration::minutes(10)).timestamp()
            }),
            status: WebhookStatus::Processed,
            error_message: None,
            received_at: now,
            processed_at: Some(now),
        }];
        let eval_real_delay = OutcomeEvaluationRecord::new_pending(
            "inv_real_delay".to_string(),
            "merchant_a".to_string(),
            "pay_old".to_string(),
            "razorpay".to_string(),
            "WEBHOOK_DELAYED_OR_MISSING".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec!["webhook:evt_real_delay".to_string()]);
        let proof_delay = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval_real_delay,
            &attempt_old,
            None,
            &wh_genuinely_delayed,
            &[],
        )
        .unwrap();
        assert_eq!(proof_delay.accuracy(), OutcomeAccuracy::Correct);

        // 6. Discrepancy provenance: arbitrary provider_observed:failed rejected; actual PaymentEntity(Captured) + Attempt(authorized) accepted
        let attempt_disc = PaymentAttemptRecord {
            transaction_id: "tx_disc2".to_string(),
            merchant_id: "merchant_a".to_string(),
            provider: "razorpay".to_string(),
            provider_order_id: None,
            payment_reference: Some("pay_disc2".to_string()),
            amount_minor: 10000,
            currency: "INR".to_string(),
            state: "authorized".to_string(),
            created_at: now,
            updated_at: now,
        };
        let entity_disc = PaymentEntity {
            payment_id: "pay_disc2".to_string(),
            state: PaymentState::Captured,
            amount_minor: 10000,
            currency: "INR".to_string(),
            refunded_amount_minor: 0,
            events_seen: vec![],
            refunds: vec![],
            created_at: now,
            updated_at: now,
        };
        let eval_arb_obs = OutcomeEvaluationRecord::new_pending(
            "inv_arb_obs".to_string(),
            "merchant_a".to_string(),
            "pay_disc2".to_string(),
            "razorpay".to_string(),
            "PROVIDER_STATE_DISCREPANCY".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec![
            "state:captured".to_string(),
            "provider_observed:failed".to_string(),
        ]);
        assert_eq!(
            AuthoritativeOutcomeProof::from_authoritative_records(
                &scope_for("merchant_a"),
                &eval_arb_obs,
                &attempt_disc,
                Some(&entity_disc),
                &[],
                &[],
            )
            .unwrap_err()
            .category,
            InferenceErrorCategory::InvalidResponse
        );

        let eval_exact_disc = OutcomeEvaluationRecord::new_pending(
            "inv_exact_disc".to_string(),
            "merchant_a".to_string(),
            "pay_disc2".to_string(),
            "razorpay".to_string(),
            "PROVIDER_STATE_DISCREPANCY".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec![
            "state:captured".to_string(),
            "attempt_state:authorized".to_string(),
        ]);
        let proof_disc = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval_exact_disc,
            &attempt_disc,
            Some(&entity_disc),
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(proof_disc.accuracy(), OutcomeAccuracy::Correct);
    }

    #[tokio::test]
    async fn test_entity_created_at_fallback_removed_provider_mismatch_and_processed_timeout() {
        use esa_state::{WebhookInboxRecord, WebhookStatus};

        let now = Utc::now();

        // 1. Payload with ONLY payload.payment.entity.created_at (old payment creation) must NOT verify WEBHOOK_DELAYED_OR_MISSING
        let attempt = PaymentAttemptRecord {
            transaction_id: "tx_ent_ts".to_string(),
            merchant_id: "merchant_a".to_string(),
            provider: "razorpay".to_string(),
            provider_order_id: None,
            payment_reference: Some("pay_ent_ts".to_string()),
            amount_minor: 10000,
            currency: "INR".to_string(),
            state: "captured".to_string(),
            created_at: now - chrono::Duration::hours(3),
            updated_at: now,
        };
        let wh_entity_ts_only = vec![WebhookInboxRecord {
            id: 10,
            provider: "razorpay".to_string(),
            provider_event_id: "evt_ent_ts".to_string(),
            event_type: "PaymentCaptured".to_string(),
            payment_id: Some("pay_ent_ts".to_string()),
            refund_id: None,
            payload: serde_json::json!({
                "payload": {
                    "payment": {
                        "entity": {
                            "created_at": (now - chrono::Duration::hours(3)).timestamp()
                        }
                    }
                }
            }),
            status: WebhookStatus::Processed,
            error_message: None,
            received_at: now,
            processed_at: Some(now),
        }];
        let eval_ent_ts = OutcomeEvaluationRecord::new_pending(
            "inv_ent_ts".to_string(),
            "merchant_a".to_string(),
            "pay_ent_ts".to_string(),
            "razorpay".to_string(),
            "WEBHOOK_DELAYED_OR_MISSING".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec!["webhook:evt_ent_ts".to_string()]);
        assert_eq!(
            AuthoritativeOutcomeProof::from_authoritative_records(
                &scope_for("merchant_a"),
                &eval_ent_ts,
                &attempt,
                None,
                &wh_entity_ts_only,
                &[],
            )
            .unwrap_err()
            .category,
            InferenceErrorCategory::InvalidResponse
        );

        // 2. Evaluation provider mismatch against attempt provider MUST fail proof construction
        let eval_prov_mismatch = OutcomeEvaluationRecord::new_pending(
            "inv_prov_mm".to_string(),
            "merchant_a".to_string(),
            "pay_ent_ts".to_string(),
            "cashfree".to_string(), // differs from attempt.provider ("razorpay")!
            "HEALTHY_COMPLETED".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        );
        assert_eq!(
            AuthoritativeOutcomeProof::from_authoritative_records(
                &scope_for("merchant_a"),
                &eval_prov_mismatch,
                &attempt,
                None,
                &[],
                &[],
            )
            .unwrap_err()
            .category,
            InferenceErrorCategory::InvalidResponse
        );

        // 3. Locally failed webhook-processing record with timeout-like error text MUST be rejected for PROVIDER_DEGRADED_OR_TIMEOUT
        let attempt_failed = PaymentAttemptRecord {
            state: "failed".to_string(),
            ..attempt.clone()
        };
        let wh_local_timeout_err = vec![WebhookInboxRecord {
            id: 11,
            provider: "razorpay".to_string(),
            provider_event_id: "evt_local_to".to_string(),
            event_type: "PaymentFailed".to_string(),
            payment_id: Some("pay_ent_ts".to_string()),
            refund_id: None,
            payload: serde_json::json!({"event": "payment.failed"}),
            status: WebhookStatus::Failed,
            error_message: Some("HTTP 504 provider timeout while parsing locally".to_string()),
            received_at: now,
            processed_at: None,
        }];
        let eval_local_to = OutcomeEvaluationRecord::new_pending(
            "inv_local_to".to_string(),
            "merchant_a".to_string(),
            "pay_ent_ts".to_string(),
            "razorpay".to_string(),
            "PROVIDER_DEGRADED_OR_TIMEOUT".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Failed,
        )
        .with_evidence_refs(vec!["webhook:evt_local_to".to_string()]);
        assert_eq!(
            AuthoritativeOutcomeProof::from_authoritative_records(
                &scope_for("merchant_a"),
                &eval_local_to,
                &attempt_failed,
                None,
                &wh_local_timeout_err,
                &[],
            )
            .unwrap_err()
            .category,
            InferenceErrorCategory::InvalidResponse
        );

        // 4. Processed, matching provider-side timeout event with explicit citation MUST be accepted
        let wh_processed_timeout = vec![WebhookInboxRecord {
            id: 12,
            provider: "razorpay".to_string(),
            provider_event_id: "evt_prov_to".to_string(),
            event_type: "payment.failed".to_string(),
            payment_id: Some("pay_ent_ts".to_string()),
            refund_id: None,
            payload: serde_json::json!({"error": {"code": "GATEWAY_TIMEOUT"}}),
            status: WebhookStatus::Processed,
            error_message: None,
            received_at: now,
            processed_at: Some(now),
        }];
        let eval_prov_to = OutcomeEvaluationRecord::new_pending(
            "inv_prov_to".to_string(),
            "merchant_a".to_string(),
            "pay_ent_ts".to_string(),
            "razorpay".to_string(),
            "PROVIDER_DEGRADED_OR_TIMEOUT".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Failed,
        )
        .with_evidence_refs(vec!["webhook:evt_prov_to".to_string()]);
        let proof_to = AuthoritativeOutcomeProof::from_authoritative_records(
            &scope_for("merchant_a"),
            &eval_prov_to,
            &attempt_failed,
            None,
            &wh_processed_timeout,
            &[],
        )
        .unwrap();
        assert_eq!(proof_to.accuracy(), OutcomeAccuracy::Correct);
    }

    #[tokio::test]
    async fn test_phase4_trusted_resolution_lifecycle_duplicate_audit_and_immutability() {
        let wh_store = esa_state::InMemoryWebhookStore::new();
        let mem_store = InMemoryPaymentMemoryStore::new();
        let now = Utc::now();

        // 1. Seed payment attempt for merchant_a
        wh_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_p4_dup".to_string(),
                merchant_id: "merchant_a".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_p4_dup".to_string()),
                payment_reference: Some("pay_p4_dup".to_string()),
                amount_minor: 50000,
                currency: "INR".to_string(),
                state: "created".to_string(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();

        let payload = serde_json::json!({ "event": "payment.captured" });
        let wh_params = esa_state::WebhookEventParams {
            provider: "razorpay",
            provider_event_id: "evt_p4_dup_1",
            event_type: esa_core::PaymentEventType::PaymentCaptured,
            payment_id: "pay_p4_dup",
            amount_minor: 50000,
            currency: "INR",
            refund_id: None,
            refund_amount_minor: None,
            refund_status: None,
            payload: &payload,
        };
        // First delivery only (no duplicate yet!)
        wh_store.process_webhook_event(&wh_params).await.unwrap();

        let eval = OutcomeEvaluationRecord::new_pending(
            "inv_p4_dup".to_string(),
            "merchant_a".to_string(),
            "pay_p4_dup".to_string(),
            "razorpay".to_string(),
            "DUPLICATE_WEBHOOK_SUPPRESSED".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Captured,
        )
        .with_evidence_refs(vec!["webhook:evt_p4_dup_1".to_string()]);
        let eval = mem_store.record_evaluation(eval).await.unwrap();
        let res = TrustedResolutionRecord::from_evaluation(&eval);
        let init_audit = ResolutionAuditEvent {
            audit_id: "raud_init_1".to_string(),
            resolution_id: res.resolution_id.clone(),
            evaluation_id: eval.evaluation_id.clone(),
            merchant_id: "merchant_a".to_string(),
            from_status: None,
            to_status: ResolutionStatus::Pending,
            actor: "system:payment_intelligence".to_string(),
            verification_method: "pending_investigation".to_string(),
            reason: "Initial pending resolution".to_string(),
            evidence_refs: eval.supporting_evidence_refs.clone(),
            evidence_provenance: Vec::new(),
            promoted_memory_id: None,
            created_at: now,
        };
        let res = mem_store.record_resolution(res, init_audit).await.unwrap();

        // 2. Without duplicate delivery audit evidence, verification MUST fail closed and stay Pending
        let (pending_res, pending_audits, pending_mem) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &res.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(pending_res.status, ResolutionStatus::Pending);
        assert!(pending_mem.is_none());
        assert!(!pending_res.missing_evidence.is_empty());
        assert_eq!(pending_audits.len(), 2);

        // 3. Deliver duplicate webhook -> durable webhook_delivery_audit records duplicate suppression
        wh_store.process_webhook_event(&wh_params).await.unwrap();

        let (verified_res, verified_audits, promoted_mem) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &res.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(verified_res.status, ResolutionStatus::Verified);
        assert_eq!(verified_res.accuracy, OutcomeAccuracy::Correct);
        assert!(verified_res.promoted_memory_id.is_some());
        assert!(verified_audits.len() >= 3);

        let mem = promoted_mem.expect("Correct verified outcome must promote to episodic memory");
        let prov = mem
            .provenance
            .clone()
            .expect("Promoted memory must retain provenance");
        assert_eq!(prov.evaluation_id, eval.evaluation_id);
        assert_eq!(prov.resolution_id, res.resolution_id);
        assert_eq!(prov.merchant_id, "merchant_a");
        assert_eq!(prov.payment_reference, "pay_p4_dup");
        assert_eq!(prov.provider, "razorpay");
        assert_eq!(prov.promotion_decision, "promoted_verified_correct");
        assert!(prov
            .evidence_provenance
            .contains(&"webhook_delivery_audit".to_string()));

        // 4. Promoted verified episodic memory is immutable via store_memory
        let mut tampered = mem.clone();
        tampered.summary = "Tampered summary".to_string();
        let immut_err = mem_store.store_memory(tampered).await.unwrap_err();
        assert_eq!(immut_err.category, InferenceErrorCategory::Forbidden);

        // 5. Cross-merchant resolution access is Forbidden
        let cross_err = mem_store
            .get_resolution(&scope_for("merchant_b"), &res.resolution_id)
            .await
            .unwrap_err();
        assert_eq!(cross_err.category, InferenceErrorCategory::Forbidden);

        // 6. Non-proof transition cannot mark a resolution as Verified or Rejected
        let direct_err = mem_store
            .record_resolution_pending_or_expired(
                &scope_for("merchant_a"),
                &res.resolution_id,
                ResolutionStatus::Verified,
                "bypass attempt",
                vec![],
            )
            .await
            .unwrap_err();
        assert_eq!(direct_err.category, InferenceErrorCategory::Forbidden);
    }

    #[tokio::test]
    async fn test_phase4_rejected_expired_and_timeout_without_citation() {
        let wh_store = esa_state::InMemoryWebhookStore::new();
        let mem_store = InMemoryPaymentMemoryStore::new();
        let now = Utc::now();

        wh_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_p4_rej".to_string(),
                merchant_id: "merchant_a".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: None,
                payment_reference: Some("pay_p4_rej".to_string()),
                amount_minor: 10000,
                currency: "INR".to_string(),
                state: "timeout".to_string(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
        wh_store
            .process_webhook_event(&esa_state::WebhookEventParams {
                provider: "razorpay",
                provider_event_id: "evt_p4_fail",
                event_type: esa_core::PaymentEventType::PaymentFailed,
                payment_id: "pay_p4_rej",
                amount_minor: 10000,
                currency: "INR",
                refund_id: None,
                refund_amount_minor: None,
                refund_status: None,
                payload: &serde_json::json!({ "event": "payment.failed" }),
            })
            .await
            .unwrap();

        // 1. Incorrect diagnosis -> Rejected, never promoted
        let eval_inc = mem_store
            .record_evaluation(OutcomeEvaluationRecord::new_pending(
                "inv_p4_rej".to_string(),
                "merchant_a".to_string(),
                "pay_p4_rej".to_string(),
                "razorpay".to_string(),
                "HEALTHY_COMPLETED".to_string(),
                crate::provider::ProviderKind::Ollama,
                "deepseek-r1:1.5b".to_string(),
                false,
                CanonicalPaymentState::Created,
            ))
            .await
            .unwrap();
        let res_inc = mem_store
            .record_resolution(
                TrustedResolutionRecord::from_evaluation(&eval_inc),
                ResolutionAuditEvent {
                    audit_id: "raud_rej_1".to_string(),
                    resolution_id: format!("res_{}", Uuid::new_v4().simple()),
                    evaluation_id: eval_inc.evaluation_id.clone(),
                    merchant_id: "merchant_a".to_string(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system:payment_intelligence".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "init".to_string(),
                    evidence_refs: vec![],
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();
        let (rej_res, _, rej_mem) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &res_inc.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(rej_res.status, ResolutionStatus::Rejected);
        assert_eq!(rej_res.accuracy, OutcomeAccuracy::Incorrect);
        assert!(rej_mem.is_none());

        // 2. Expired resolution -> Expired, never promoted
        let mut eval_exp = OutcomeEvaluationRecord::new_pending(
            "inv_p4_exp".to_string(),
            "merchant_a".to_string(),
            "pay_p4_rej".to_string(),
            "razorpay".to_string(),
            "PAYMENT_FAILED_AT_PROVIDER".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Failed,
        )
        .with_evidence_refs(vec!["webhook:evt_p4_fail".to_string()]);
        eval_exp.expires_at = Some(now - chrono::Duration::seconds(5));
        let eval_exp = mem_store.record_evaluation(eval_exp).await.unwrap();
        let res_exp = mem_store
            .record_resolution(
                TrustedResolutionRecord::from_evaluation(&eval_exp),
                ResolutionAuditEvent {
                    audit_id: "raud_exp_1".to_string(),
                    resolution_id: format!("res_{}", Uuid::new_v4().simple()),
                    evaluation_id: eval_exp.evaluation_id.clone(),
                    merchant_id: "merchant_a".to_string(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system:payment_intelligence".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "init".to_string(),
                    evidence_refs: vec![],
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();
        let (exp_res, _, exp_mem) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &res_exp.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(exp_res.status, ResolutionStatus::Expired);
        assert!(exp_mem.is_none());

        // 3. PROVIDER_DEGRADED_OR_TIMEOUT without citing attempt:<tx_id> or timeout webhook stays Pending
        let eval_to = mem_store
            .record_evaluation(OutcomeEvaluationRecord::new_pending(
                "inv_p4_to".to_string(),
                "merchant_a".to_string(),
                "pay_p4_rej".to_string(),
                "razorpay".to_string(),
                "PROVIDER_DEGRADED_OR_TIMEOUT".to_string(),
                crate::provider::ProviderKind::Ollama,
                "deepseek-r1:1.5b".to_string(),
                false,
                CanonicalPaymentState::Failed,
            ))
            .await
            .unwrap();
        let res_to = mem_store
            .record_resolution(
                TrustedResolutionRecord::from_evaluation(&eval_to),
                ResolutionAuditEvent {
                    audit_id: "raud_to_1".to_string(),
                    resolution_id: format!("res_{}", Uuid::new_v4().simple()),
                    evaluation_id: eval_to.evaluation_id.clone(),
                    merchant_id: "merchant_a".to_string(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system:payment_intelligence".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "init".to_string(),
                    evidence_refs: vec![],
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();
        let (to_res, _, to_mem) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_for("merchant_a"),
            &res_to.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(to_res.status, ResolutionStatus::Pending);
        assert!(to_mem.is_none());
    }

    #[tokio::test]
    async fn test_phase4_provider_timeout_provenance_regressions() {
        use esa_state::InMemoryWebhookStore;
        let wh_store = InMemoryWebhookStore::new();
        let mem_store = InMemoryPaymentMemoryStore::new();
        let now = Utc::now();
        let scope_a = scope_for("merchant_a");

        async fn record_attempt(
            wh: &InMemoryWebhookStore,
            tx_id: &str,
            pay_id: &str,
            state: &str,
            now: chrono::DateTime<Utc>,
        ) {
            wh.record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: tx_id.to_string(),
                merchant_id: "merchant_a".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some(format!("order_{}", pay_id)),
                payment_reference: Some(pay_id.to_string()),
                amount_minor: 50000,
                currency: "INR".to_string(),
                state: state.to_string(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
        }

        // 1. Gateway transport timeout (attempt.state == "timeout") WITHOUT authoritative provider webhook
        // Must fail closed and stay Pending (never verified as provider failure)
        record_attempt(&wh_store, "tx_trans_to", "pay_trans_to", "timeout", now).await;
        let eval_transport = mem_store
            .record_evaluation(
                OutcomeEvaluationRecord::new_pending(
                    "inv_trans_to".to_string(),
                    "merchant_a".to_string(),
                    "pay_trans_to".to_string(),
                    "razorpay".to_string(),
                    "PROVIDER_DEGRADED_OR_TIMEOUT".to_string(),
                    crate::provider::ProviderKind::Ollama,
                    "deepseek-r1:1.5b".to_string(),
                    false,
                    CanonicalPaymentState::Failed,
                )
                .with_evidence_refs(vec!["attempt:tx_trans_to".to_string()]),
            )
            .await
            .unwrap();
        let res_transport = mem_store
            .record_resolution(
                TrustedResolutionRecord::from_evaluation(&eval_transport),
                ResolutionAuditEvent {
                    audit_id: "raud_trans_to".to_string(),
                    resolution_id: format!("res_{}", Uuid::new_v4().simple()),
                    evaluation_id: eval_transport.evaluation_id.clone(),
                    merchant_id: "merchant_a".to_string(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "init".to_string(),
                    evidence_refs: eval_transport.supporting_evidence_refs.clone(),
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();
        let (res_out, _, mem_out) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_a,
            &res_transport.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(res_out.status, ResolutionStatus::Pending);
        assert!(mem_out.is_none());

        // 2. Generic failed payment with timeout-like customer notes
        // Payload has customer_notes: "Payment failed due to gateway_timeout on checkout page"
        // But structured error_code is generic "BAD_REQUEST". Must fail closed and stay Pending!
        record_attempt(&wh_store, "tx_notes_to", "pay_notes_to", "failed", now).await;
        let notes_payload = serde_json::json!({
            "event": "payment.failed",
            "error_code": "BAD_REQUEST",
            "customer_notes": "Payment failed due to gateway_timeout on checkout page",
            "created_at": now.timestamp()
        });
        wh_store
            .process_webhook_event(&esa_state::WebhookEventParams {
                provider: "razorpay",
                provider_event_id: "evt_notes_to",
                event_type: esa_core::PaymentEventType::PaymentFailed,
                payment_id: "pay_notes_to",
                amount_minor: 50000,
                currency: "INR",
                refund_id: None,
                refund_amount_minor: None,
                refund_status: None,
                payload: &notes_payload,
            })
            .await
            .unwrap();
        let eval_notes = mem_store
            .record_evaluation(
                OutcomeEvaluationRecord::new_pending(
                    "inv_notes_to".to_string(),
                    "merchant_a".to_string(),
                    "pay_notes_to".to_string(),
                    "razorpay".to_string(),
                    "PROVIDER_DEGRADED_OR_TIMEOUT".to_string(),
                    crate::provider::ProviderKind::Ollama,
                    "deepseek-r1:1.5b".to_string(),
                    false,
                    CanonicalPaymentState::Failed,
                )
                .with_evidence_refs(vec!["webhook:evt_notes_to".to_string()]),
            )
            .await
            .unwrap();
        let res_notes = mem_store
            .record_resolution(
                TrustedResolutionRecord::from_evaluation(&eval_notes),
                ResolutionAuditEvent {
                    audit_id: "raud_notes_to".to_string(),
                    resolution_id: format!("res_{}", Uuid::new_v4().simple()),
                    evaluation_id: eval_notes.evaluation_id.clone(),
                    merchant_id: "merchant_a".to_string(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "init".to_string(),
                    evidence_refs: eval_notes.supporting_evidence_refs.clone(),
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();
        let (res_notes_out, _, mem_notes_out) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_a,
            &res_notes.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(res_notes_out.status, ResolutionStatus::Pending);
        assert!(mem_notes_out.is_none());

        // 3. Incorrect or unrelated attempt citations: citing attempt:tx_other when attempt is tx_actual
        let eval_unrelated = mem_store
            .record_evaluation(
                OutcomeEvaluationRecord::new_pending(
                    "inv_unrelated_to".to_string(),
                    "merchant_a".to_string(),
                    "pay_trans_to".to_string(),
                    "razorpay".to_string(),
                    "PROVIDER_DEGRADED_OR_TIMEOUT".to_string(),
                    crate::provider::ProviderKind::Ollama,
                    "deepseek-r1:1.5b".to_string(),
                    false,
                    CanonicalPaymentState::Failed,
                )
                .with_evidence_refs(vec!["attempt:tx_unrelated_random".to_string()]),
            )
            .await
            .unwrap();
        let res_unrelated = mem_store
            .record_resolution(
                TrustedResolutionRecord::from_evaluation(&eval_unrelated),
                ResolutionAuditEvent {
                    audit_id: "raud_unrelated_to".to_string(),
                    resolution_id: format!("res_{}", Uuid::new_v4().simple()),
                    evaluation_id: eval_unrelated.evaluation_id.clone(),
                    merchant_id: "merchant_a".to_string(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "init".to_string(),
                    evidence_refs: eval_unrelated.supporting_evidence_refs.clone(),
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();
        let (res_unrel_out, _, mem_unrel_out) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_a,
            &res_unrelated.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(res_unrel_out.status, ResolutionStatus::Pending);
        assert!(mem_unrel_out.is_none());

        // 4. Provider-confirmed timeout: authoritative processed webhook with structured error_code "GATEWAY_TIMEOUT"
        record_attempt(&wh_store, "tx_auth_to", "pay_auth_to", "failed", now).await;
        let auth_payload = serde_json::json!({
            "event": "payment.failed",
            "error": {
                "code": "GATEWAY_TIMEOUT",
                "description": "Provider upstream gateway timed out"
            },
            "created_at": now.timestamp()
        });
        wh_store
            .process_webhook_event(&esa_state::WebhookEventParams {
                provider: "razorpay",
                provider_event_id: "evt_auth_timeout",
                event_type: esa_core::PaymentEventType::PaymentFailed,
                payment_id: "pay_auth_to",
                amount_minor: 50000,
                currency: "INR",
                refund_id: None,
                refund_amount_minor: None,
                refund_status: None,
                payload: &auth_payload,
            })
            .await
            .unwrap();
        let eval_auth = mem_store
            .record_evaluation(
                OutcomeEvaluationRecord::new_pending(
                    "inv_auth_to".to_string(),
                    "merchant_a".to_string(),
                    "pay_auth_to".to_string(),
                    "razorpay".to_string(),
                    "PROVIDER_DEGRADED_OR_TIMEOUT".to_string(),
                    crate::provider::ProviderKind::Ollama,
                    "deepseek-r1:1.5b".to_string(),
                    false,
                    CanonicalPaymentState::Failed,
                )
                .with_evidence_refs(vec!["webhook:evt_auth_timeout".to_string()]),
            )
            .await
            .unwrap();
        let res_auth = mem_store
            .record_resolution(
                TrustedResolutionRecord::from_evaluation(&eval_auth),
                ResolutionAuditEvent {
                    audit_id: "raud_auth_to".to_string(),
                    resolution_id: format!("res_{}", Uuid::new_v4().simple()),
                    evaluation_id: eval_auth.evaluation_id.clone(),
                    merchant_id: "merchant_a".to_string(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "init".to_string(),
                    evidence_refs: eval_auth.supporting_evidence_refs.clone(),
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();
        let (res_auth_out, _, mem_auth_out) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_a,
            &res_auth.resolution_id,
        )
        .await
        .unwrap();
        assert_eq!(res_auth_out.status, ResolutionStatus::Verified);
        assert_eq!(res_auth_out.accuracy, OutcomeAccuracy::Correct);
        let promoted =
            mem_auth_out.expect("Authoritative provider timeout must promote episodic memory");
        assert_eq!(promoted.verification_status, VerificationStatus::Verified);
    }

    #[test]
    fn test_strict_provider_timeout_error_code_allow_list() {
        use crate::memory::types::is_strict_provider_timeout_event;
        use esa_state::{WebhookInboxRecord, WebhookStatus};

        let make_inbox = |provider: &str,
                          event_type: &str,
                          payload: serde_json::Value,
                          status: WebhookStatus| {
            WebhookInboxRecord {
                id: 1,
                provider: provider.to_string(),
                provider_event_id: "evt_1".to_string(),
                event_type: event_type.to_string(),
                payment_id: Some("pay_1".to_string()),
                refund_id: None,
                status,
                error_message: None,
                received_at: Utc::now(),
                processed_at: Some(Utc::now()),
                payload,
            }
        };

        let failed_ev_type = esa_core::PaymentEventType::PaymentFailed;

        // 1. Razorpay allow-list
        for code in [
            "BAD_GATEWAY",
            "GATEWAY_ERROR",
            "GATEWAY_TIMEOUT",
            "SERVER_ERROR",
        ] {
            let ev = make_inbox(
                "razorpay",
                "payment.failed",
                serde_json::json!({ "error": { "code": code } }),
                WebhookStatus::Processed,
            );
            assert!(
                is_strict_provider_timeout_event(&ev, "razorpay", false, Some(&failed_ev_type)),
                "Razorpay code {} must be recognized as provider timeout",
                code
            );
        }

        // 2. Cashfree allow-list
        for code in [
            "GATEWAY_ERROR",
            "GATEWAY_TIMEOUT",
            "BANK_COMMUNICATION_ERROR",
            "SYSTEM_ERROR",
        ] {
            let ev = make_inbox(
                "cashfree",
                "payment.failed",
                serde_json::json!({ "data": { "payment": { "error_code": code } } }),
                WebhookStatus::Processed,
            );
            assert!(
                is_strict_provider_timeout_event(&ev, "cashfree", false, Some(&failed_ev_type)),
                "Cashfree code {} must be recognized as provider timeout",
                code
            );
        }

        // 3. PayU allow-list
        for code in ["E1601", "E1602", "E205", "GATEWAY_TIMEOUT", "BANK_TIMEOUT"] {
            let ev = make_inbox(
                "payu",
                "payment_failed",
                serde_json::json!({ "error_code": code }),
                WebhookStatus::Processed,
            );
            assert!(
                is_strict_provider_timeout_event(&ev, "payu", false, Some(&failed_ev_type)),
                "PayU code {} must be recognized as provider timeout",
                code
            );
        }

        // 4. Free-text descriptions and /error/reason are strictly rejected
        let ev_reason = make_inbox(
            "razorpay",
            "payment.failed",
            serde_json::json!({ "error": { "reason": "gateway_timeout occurred" } }),
            WebhookStatus::Processed,
        );
        assert!(!is_strict_provider_timeout_event(
            &ev_reason,
            "razorpay",
            false,
            Some(&failed_ev_type)
        ));

        // 5. Customer notes with timeout text are rejected
        let ev_notes = make_inbox(
            "razorpay",
            "payment.failed",
            serde_json::json!({ "notes": { "desc": "gateway_timeout" }, "error": { "code": "BAD_REQUEST" } }),
            WebhookStatus::Processed,
        );
        assert!(!is_strict_provider_timeout_event(
            &ev_notes,
            "razorpay",
            false,
            Some(&failed_ev_type)
        ));

        // 6. Non-timeout error codes are rejected
        for bad_code in [
            "BAD_REQUEST",
            "AUTHENTICATION_FAILED",
            "PAYMENT_CANCELLED",
            "NOT_A_TIMEOUT",
            "timeout_not_supported",
        ] {
            let ev_bad = make_inbox(
                "razorpay",
                "payment.failed",
                serde_json::json!({ "error": { "code": bad_code } }),
                WebhookStatus::Processed,
            );
            assert!(!is_strict_provider_timeout_event(
                &ev_bad,
                "razorpay",
                false,
                Some(&failed_ev_type)
            ));
        }

        // 7. Refund event with timeout-like error code is rejected
        let refund_failed_type = esa_core::PaymentEventType::RefundFailed;
        let ev_refund = make_inbox(
            "razorpay",
            "refund.failed",
            serde_json::json!({ "error": { "code": "GATEWAY_TIMEOUT" } }),
            WebhookStatus::Processed,
        );
        assert!(!is_strict_provider_timeout_event(
            &ev_refund,
            "razorpay",
            true,
            Some(&refund_failed_type)
        ));

        // 8. Unprocessed webhook status is rejected
        let ev_unproc = make_inbox(
            "razorpay",
            "payment.failed",
            serde_json::json!({ "error": { "code": "GATEWAY_TIMEOUT" } }),
            WebhookStatus::Pending,
        );
        assert!(!is_strict_provider_timeout_event(
            &ev_unproc,
            "razorpay",
            false,
            Some(&failed_ev_type)
        ));
    }

    #[tokio::test]
    async fn test_generic_store_memory_rejects_fabricated_provenance_and_episodic_creation() {
        let store = InMemoryPaymentMemoryStore::new();
        let now = Utc::now();

        // 1. Caller supplies episodic memory with Verified status, fabricated provenance claiming verification
        let fabricated = MemoryRecord {
            memory_id: "mem_fabricated_1".to_string(),
            memory_type: MemoryType::Episodic,
            scope: MemoryScope::Merchant,
            merchant_id: Some("merchant_a".to_string()),
            provider: Some("razorpay".to_string()),
            incident_category: "PAYMENT_FAILED_AT_PROVIDER".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Failed),
            title: "Fabricated incident".to_string(),
            summary: "Fabricated summary".to_string(),
            resolution: Some("Fabricated resolution".to_string()),
            source_refs: vec!["payment:pay_fake".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: Some(now + chrono::Duration::days(30)),
            provenance: Some(MemoryProvenance {
                evaluation_id: "eval_fake_999".to_string(),
                resolution_id: "res_fake_999".to_string(),
                merchant_id: "merchant_a".to_string(),
                payment_reference: "pay_fake".to_string(),
                provider: "razorpay".to_string(),
                verified_at: now,
                verification_method: "fabricated_bypass".to_string(),
                supporting_evidence_refs: vec!["arbitrary_ref".to_string()],
                evidence_provenance: vec!["fake_source".to_string()],
                promotion_decision: "promoted_verified_correct".to_string(),
            }),
        };

        let err = store.store_memory(fabricated).await.unwrap_err();
        assert_eq!(err.category, InferenceErrorCategory::Forbidden);
        assert!(err
            .message
            .contains("Episodic memories cannot be created or modified directly"));

        // 2. Working, Semantic, and Procedural memory can legitimately be stored
        let working = MemoryRecord {
            memory_id: "mem_wm_1".to_string(),
            memory_type: MemoryType::Working,
            scope: MemoryScope::Merchant,
            merchant_id: Some("merchant_a".to_string()),
            provider: Some("razorpay".to_string()),
            incident_category: "PAYMENT_ANALYSIS".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Authorized),
            title: "Working memory title".to_string(),
            summary: "Working notes".to_string(),
            resolution: None,
            source_refs: vec!["payment:pay_wm".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: Some(now + chrono::Duration::hours(2)),
            provenance: None,
        };
        let stored_wm = store.store_memory(working).await.unwrap();
        assert_eq!(stored_wm.memory_id, "mem_wm_1");
    }

    #[tokio::test]
    async fn test_phase4_untrusted_webhook_audit_regressions() {
        use esa_state::{InMemoryWebhookStore, WebhookDeliveryAuditRecord};
        let wh_store = InMemoryWebhookStore::new();
        let mem_store = InMemoryPaymentMemoryStore::new();
        let now = Utc::now();
        let scope_a = scope_for("merchant_a");

        // 1. Unverified delivery audit with rejected signature
        // Ingress rejects untrusted payload attribution: merchant_id and payment_id are None
        let unverified = WebhookDeliveryAuditRecord {
            delivery_id: "dlv_forged_1".to_string(),
            merchant_id: None,
            provider: "razorpay".to_string(),
            provider_event_id: "evt_forged_spoof".to_string(),
            payment_id: None,
            inbox_id: None,
            original_delivery_id: None,
            attempt_number: 1,
            first_received_at: now,
            received_at: now,
            signature_verified: false,
            processing_result: "signature_failed".to_string(),
            deduplication_result: "not_applicable".to_string(),
            is_duplicate: false,
            final_disposition: "rejected_signature".to_string(),
            error_message: Some("Invalid signature".to_string()),
        };
        wh_store
            .record_webhook_delivery_attempt(&unverified)
            .await
            .unwrap();

        // Querying delivery audits for merchant_a and pay_secret_123 MUST return 0 records
        let audits = wh_store
            .list_webhook_delivery_audits_for_payment("merchant_a", "pay_secret_123", 50)
            .await
            .unwrap();
        assert!(
            audits.is_empty(),
            "Unverified delivery records must never attribute merchant or payment identities"
        );

        // 2. Legitimate webhook arrival: must NOT be front-run by unverified delivery
        wh_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_legit_1".to_string(),
                merchant_id: "merchant_a".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_legit".to_string()),
                payment_reference: Some("pay_legit".to_string()),
                amount_minor: 10000,
                currency: "INR".to_string(),
                state: "captured".to_string(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();

        let legit_payload = serde_json::json!({
            "event": "payment.captured",
            "created_at": now.timestamp()
        });
        wh_store
            .process_webhook_event(&esa_state::WebhookEventParams {
                provider: "razorpay",
                provider_event_id: "evt_forged_spoof", // same event ID that an attacker previously probed with invalid signature
                event_type: esa_core::PaymentEventType::PaymentCaptured,
                payment_id: "pay_legit",
                amount_minor: 10000,
                currency: "INR",
                refund_id: None,
                refund_amount_minor: None,
                refund_status: None,
                payload: &legit_payload,
            })
            .await
            .unwrap();

        // The first verified delivery must have attempt_number == 1 and original_delivery_id == None
        let legit_audits = wh_store
            .list_webhook_delivery_audits_for_payment("merchant_a", "pay_legit", 50)
            .await
            .unwrap();
        assert_eq!(legit_audits.len(), 1);
        assert_eq!(legit_audits[0].attempt_number, 1);
        assert!(legit_audits[0].original_delivery_id.is_none());
        assert!(!legit_audits[0].is_duplicate);

        // 3. Untrusted duplicate claims cannot verify DUPLICATE_WEBHOOK_SUPPRESSED
        let eval_dup = mem_store
            .record_evaluation(
                OutcomeEvaluationRecord::new_pending(
                    "inv_untrusted_dup".to_string(),
                    "merchant_a".to_string(),
                    "pay_legit".to_string(),
                    "razorpay".to_string(),
                    "DUPLICATE_WEBHOOK_SUPPRESSED".to_string(),
                    crate::provider::ProviderKind::Ollama,
                    "deepseek-r1:1.5b".to_string(),
                    false,
                    CanonicalPaymentState::Captured,
                )
                .with_evidence_refs(vec!["webhook:evt_forged_spoof".to_string()]),
            )
            .await
            .unwrap();
        let res_dup = mem_store
            .record_resolution(
                TrustedResolutionRecord::from_evaluation(&eval_dup),
                ResolutionAuditEvent {
                    audit_id: "raud_dup".to_string(),
                    resolution_id: format!("res_{}", Uuid::new_v4().simple()),
                    evaluation_id: eval_dup.evaluation_id.clone(),
                    merchant_id: "merchant_a".to_string(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "init".to_string(),
                    evidence_refs: eval_dup.supporting_evidence_refs.clone(),
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();
        let (dup_out, _, dup_mem) = verify_resolution_from_webhook_store(
            &wh_store,
            &mem_store,
            &scope_a,
            &res_dup.resolution_id,
        )
        .await
        .unwrap();
        // Since only 1 verified delivery exists (the other was unverified), it must stay Pending!
        assert_eq!(dup_out.status, ResolutionStatus::Pending);
        assert!(dup_mem.is_none());
    }

    #[tokio::test]
    async fn test_phase4_episodic_memory_immutability_and_purge() {
        let mem_store = InMemoryPaymentMemoryStore::new();
        let scope_a = scope_for("merchant_a");
        let now = Utc::now();

        // 1. Create a promoted verified episodic memory
        let verified_memory = MemoryRecord {
            memory_id: "mem_ep_immutable_1".to_string(),
            memory_type: MemoryType::Episodic,
            scope: MemoryScope::Merchant,
            merchant_id: Some("merchant_a".to_string()),
            provider: Some("razorpay".to_string()),
            incident_category: "DUPLICATE_WEBHOOK_SUPPRESSED".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Captured),
            title: "Verified duplicate webhook incident".to_string(),
            summary: "Verified duplicate webhook was suppressed cleanly".to_string(),
            resolution: Some("deduplicated_noop".to_string()),
            source_refs: vec!["payment:pay_immut_1".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: Some(now + chrono::Duration::days(90)),
            provenance: Some(MemoryProvenance {
                evaluation_id: "eval_immut_1".to_string(),
                resolution_id: "res_immut_1".to_string(),
                merchant_id: "merchant_a".to_string(),
                payment_reference: "pay_immut_1".to_string(),
                provider: "razorpay".to_string(),
                verified_at: now,
                verification_method: "deterministic_evidence".to_string(),
                supporting_evidence_refs: vec!["delivery:dlv_1".to_string()],
                evidence_provenance: vec![],
                promotion_decision: "promoted_verified_correct".to_string(),
            }),
        };
        // 1. Direct store_memory with verified episodic memory must return Forbidden (Finding 1)
        let create_err = mem_store
            .store_memory(verified_memory.clone())
            .await
            .unwrap_err();
        assert_eq!(create_err.category, InferenceErrorCategory::Forbidden);

        // Insert legitimate record directly via test helper for immutability testing
        mem_store.insert_test_memory(verified_memory.clone());

        // 2. Modifying via store_memory must return Forbidden
        let mut tampered = verified_memory.clone();
        tampered.summary = "Tampered summary attempt".to_string();
        let update_err = mem_store.store_memory(tampered).await.unwrap_err();
        assert_eq!(update_err.category, InferenceErrorCategory::Forbidden);

        // 3. Deleting verified episodic memory via delete_memory must return Forbidden
        let del_err = mem_store
            .delete_memory(&scope_a, "mem_ep_immutable_1")
            .await
            .unwrap_err();
        assert_eq!(del_err.category, InferenceErrorCategory::Forbidden);

        // 4. Memory must still exist intact
        let retrieved = mem_store
            .query_memories(&MemoryQuery {
                scope: scope_a.clone(),
                memory_types: vec![MemoryType::Episodic],
                provider: None,
                incident_category: None,
                lifecycle_state: None,
                since: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].memory_id, "mem_ep_immutable_1");

        // 5. Expired memory purge: unexpired memory is NOT purged
        let purged = mem_store.purge_expired().await.unwrap();
        assert_eq!(purged, 0);

        // Expired evaluation is purged correctly without touching active memories
        let mut exp_eval = OutcomeEvaluationRecord::new_pending(
            "inv_exp_purge".to_string(),
            "merchant_a".to_string(),
            "pay_exp_purge".to_string(),
            "razorpay".to_string(),
            "PROVIDER_STATE_DISCREPANCY".to_string(),
            crate::provider::ProviderKind::Ollama,
            "deepseek-r1:1.5b".to_string(),
            false,
            CanonicalPaymentState::Failed,
        );
        exp_eval.expires_at = Some(now - chrono::Duration::hours(1));
        mem_store.record_evaluation(exp_eval).await.unwrap();
        let purged_count = mem_store.purge_expired().await.unwrap();
        assert_eq!(purged_count, 1);
    }

    #[tokio::test]
    async fn test_phase4_concurrent_verification_atomicity() {
        use esa_state::InMemoryWebhookStore;
        let wh_store = Arc::new(InMemoryWebhookStore::new());
        let mem_store = Arc::new(InMemoryPaymentMemoryStore::new());
        let now = Utc::now();
        let scope_a = scope_for("merchant_a");

        wh_store
            .record_payment_attempt(&PaymentAttemptRecord {
                transaction_id: "tx_conc_1".to_string(),
                merchant_id: "merchant_a".to_string(),
                provider: "razorpay".to_string(),
                provider_order_id: Some("order_conc".to_string()),
                payment_reference: Some("pay_conc".to_string()),
                amount_minor: 25000,
                currency: "INR".to_string(),
                state: "captured".to_string(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();

        let captured_payload = serde_json::json!({
            "event": "payment.captured",
            "created_at": now.timestamp()
        });
        wh_store
            .process_webhook_event(&esa_state::WebhookEventParams {
                provider: "razorpay",
                provider_event_id: "evt_conc_dup",
                event_type: esa_core::PaymentEventType::PaymentCaptured,
                payment_id: "pay_conc",
                amount_minor: 25000,
                currency: "INR",
                refund_id: None,
                refund_amount_minor: None,
                refund_status: None,
                payload: &captured_payload,
            })
            .await
            .unwrap();
        wh_store
            .process_webhook_event(&esa_state::WebhookEventParams {
                provider: "razorpay",
                provider_event_id: "evt_conc_dup",
                event_type: esa_core::PaymentEventType::PaymentCaptured,
                payment_id: "pay_conc",
                amount_minor: 25000,
                currency: "INR",
                refund_id: None,
                refund_amount_minor: None,
                refund_status: None,
                payload: &captured_payload,
            })
            .await
            .unwrap();

        let eval = mem_store
            .record_evaluation(
                OutcomeEvaluationRecord::new_pending(
                    "inv_conc".to_string(),
                    "merchant_a".to_string(),
                    "pay_conc".to_string(),
                    "razorpay".to_string(),
                    "DUPLICATE_WEBHOOK_SUPPRESSED".to_string(),
                    crate::provider::ProviderKind::Ollama,
                    "deepseek-r1:1.5b".to_string(),
                    false,
                    CanonicalPaymentState::Captured,
                )
                .with_evidence_refs(vec!["webhook:evt_conc_dup".to_string()]),
            )
            .await
            .unwrap();

        let res = mem_store
            .record_resolution(
                TrustedResolutionRecord::from_evaluation(&eval),
                ResolutionAuditEvent {
                    audit_id: "raud_conc".to_string(),
                    resolution_id: format!("res_{}", Uuid::new_v4().simple()),
                    evaluation_id: eval.evaluation_id.clone(),
                    merchant_id: "merchant_a".to_string(),
                    from_status: None,
                    to_status: ResolutionStatus::Pending,
                    actor: "system".to_string(),
                    verification_method: "pending_investigation".to_string(),
                    reason: "init".to_string(),
                    evidence_refs: eval.supporting_evidence_refs.clone(),
                    evidence_provenance: vec![],
                    promoted_memory_id: None,
                    created_at: now,
                },
            )
            .await
            .unwrap();

        let res_id = res.resolution_id.clone();
        let (r1, r2) = tokio::join!(
            verify_resolution_from_webhook_store(&*wh_store, &*mem_store, &scope_a, &res_id),
            verify_resolution_from_webhook_store(&*wh_store, &*mem_store, &scope_a, &res_id),
        );

        let (res1, audits1, mem1) = r1.unwrap();
        let (res2, audits2, mem2) = r2.unwrap();

        assert_eq!(res1.status, ResolutionStatus::Verified);
        assert_eq!(res2.status, ResolutionStatus::Verified);
        assert_eq!(res1.promoted_memory_id, res2.promoted_memory_id);
        assert_eq!(mem1.unwrap().memory_id, mem2.unwrap().memory_id);
        assert_eq!(audits1.len(), audits2.len());

        // Ensure only one episodic memory was created in the store
        let all_mem = mem_store
            .query_memories(&MemoryQuery {
                scope: scope_a,
                memory_types: vec![MemoryType::Episodic],
                provider: None,
                incident_category: None,
                lifecycle_state: None,
                since: None,
                limit: 50,
            })
            .await
            .unwrap();
        assert_eq!(
            all_mem.len(),
            1,
            "Concurrent verification must not produce duplicate episodic memories"
        );
    }
}
