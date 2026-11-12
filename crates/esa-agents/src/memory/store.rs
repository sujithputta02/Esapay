use crate::memory::types::{
    AuthoritativeOutcomeProof, MemoryQuery, MemoryRecord, MemoryScope, MemoryType, OutcomeAccuracy,
    OutcomeEvaluationRecord, VerificationStatus,
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
}

/// Trusted crate-internal verification boundary: fetches authoritative payment, webhook, and refund
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

    let proof = AuthoritativeOutcomeProof::from_authoritative_records(
        scope,
        &eval,
        &attempt,
        payment_entity.as_ref(),
        &inbox_events,
        &refunds,
    )?;

    memory_store.verify_and_promote_outcome(scope, proof).await
}

/// Thread-safe in-memory `PaymentMemoryStore` enforcing identical tenant isolation and retention rules.
pub struct InMemoryPaymentMemoryStore {
    memories: Mutex<HashMap<String, MemoryRecord>>,
    evaluations: Mutex<HashMap<String, OutcomeEvaluationRecord>>,
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
        }
    }
}

#[async_trait]
impl PaymentMemoryStore for InMemoryPaymentMemoryStore {
    async fn store_memory(&self, record: MemoryRecord) -> Result<MemoryRecord, InferenceError> {
        let validated = record.sanitize_and_validate()?;
        let mut guard = self.memories.lock().unwrap();
        if let Some(existing) = guard.get(&validated.memory_id) {
            if existing.scope != validated.scope || existing.merchant_id != validated.merchant_id {
                return Err(InferenceError::new(
                    InferenceErrorCategory::Forbidden,
                    "Cannot overwrite memory record owned by another scope or merchant",
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
                ],
                verification_status: VerificationStatus::Verified,
                version: 1,
                created_at: now,
                updated_at: now,
                expires_at: Some(now + chrono::Duration::days(180)),
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
        let valid = make_episodic("mem_ep_1", "merchant_a", VerificationStatus::Verified);
        store.store_memory(valid).await.unwrap();

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
        store
            .store_memory(make_episodic(
                "mem_a",
                "merchant_a",
                VerificationStatus::Verified,
            ))
            .await
            .unwrap();
        store
            .store_memory(make_episodic(
                "mem_b",
                "merchant_b",
                VerificationStatus::Verified,
            ))
            .await
            .unwrap();

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
        store.store_memory(expired).await.unwrap();

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
        store
            .store_memory(make_episodic(
                "mem_del_1",
                "merchant_a",
                VerificationStatus::Verified,
            ))
            .await
            .unwrap();

        // Merchant B cannot delete Merchant A's memory
        let err_b = store
            .delete_memory(&scope_for("merchant_b"), "mem_del_1")
            .await
            .unwrap_err();
        assert_eq!(err_b.category, InferenceErrorCategory::Forbidden);

        // Merchant A can delete its own memory
        let deleted_a = store
            .delete_memory(&scope_for("merchant_a"), "mem_del_1")
            .await
            .unwrap();
        assert!(deleted_a);
    }

    #[tokio::test]
    async fn test_retriever_uses_merchant_episodic_and_system_knowledge() {
        let store = Arc::new(InMemoryPaymentMemoryStore::new());
        seed_system_knowledge(store.as_ref()).await.unwrap();
        store
            .store_memory(make_episodic(
                "mem_ret_a",
                "merchant_a",
                VerificationStatus::Verified,
            ))
            .await
            .unwrap();
        store
            .store_memory(make_episodic(
                "mem_ret_b",
                "merchant_b",
                VerificationStatus::Verified,
            ))
            .await
            .unwrap();

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
        store.store_memory(rec_a).await.unwrap();

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
}
