use crate::memory::types::{
    MemoryQuery, MemoryRecord, MemoryScope, MemoryType, OutcomeAccuracy, OutcomeEvaluationRecord,
    VerificationStatus,
};
use crate::payment_context::CanonicalPaymentState;
use crate::provider::{ExecutionScope, InferenceError, InferenceErrorCategory};
use async_trait::async_trait;
use chrono::Utc;
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
        evaluation_id: &str,
        verified_outcome_state: CanonicalPaymentState,
        accuracy: OutcomeAccuracy,
        verified_resolution: &str,
    ) -> Result<(OutcomeEvaluationRecord, Option<MemoryRecord>), InferenceError>;
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
        let mut guard = self.memories.lock().unwrap();
        let before = guard.len();
        guard.retain(|_, m| !m.is_expired(now));
        Ok(before.saturating_sub(guard.len()))
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
        let caller_mid = scope.normalized_merchant_id()?;
        let guard = self.evaluations.lock().unwrap();
        match guard.get(evaluation_id) {
            Some(ev) => {
                if let Some(mid) = caller_mid {
                    if ev.merchant_id != mid {
                        return Err(InferenceError::new(
                            InferenceErrorCategory::Forbidden,
                            "Evaluation does not belong to authenticated merchant",
                        ));
                    }
                }
                Ok(Some(ev.clone()))
            }
            None => Ok(None),
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
        let caller_mid = scope.normalized_merchant_id()?;
        let mut eval_guard = self.evaluations.lock().unwrap();
        let eval = eval_guard.get_mut(evaluation_id).ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::NotFound,
                "Evaluation record not found",
            )
        })?;
        if let Some(mid) = caller_mid {
            if eval.merchant_id != mid {
                return Err(InferenceError::new(
                    InferenceErrorCategory::Forbidden,
                    "Evaluation does not belong to authenticated merchant",
                ));
            }
        }

        let now = Utc::now();
        eval.verified_outcome_state = Some(verified_outcome_state);
        eval.accuracy = accuracy;
        eval.verified_at = Some(now);

        let promoted = if matches!(
            accuracy,
            OutcomeAccuracy::Correct | OutcomeAccuracy::PartiallyCorrect
        ) {
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
            eval.promoted_memory_id = Some(mem_id);
            Some(record)
        } else {
            None
        };

        let updated_eval = eval.clone();
        drop(eval_guard);

        if let Some(ref rec) = promoted {
            let mut mem_guard = self.memories.lock().unwrap();
            mem_guard.insert(rec.memory_id.clone(), rec.clone());
        }

        Ok((updated_eval, promoted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{seed_system_knowledge, MemoryRetriever, WorkingMemory};
    use crate::payment_context::build_payment_intelligence_context;
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
}
