use crate::payment_context::{CanonicalPaymentState, PaymentIntelligenceContext};
use crate::privacy;
use crate::provider::{ExecutionScope, InferenceError, InferenceErrorCategory, ProviderKind};
use chrono::{DateTime, Duration, Utc};
use esa_core::payment_state::{PaymentEntity, PaymentState};
use esa_core::provider::PaymentAttemptRecord;
use esa_state::webhook_store::{RefundRecord, WebhookInboxRecord};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    Working,
    Episodic,
    Semantic,
    Procedural,
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::Working => "working",
            MemoryType::Episodic => "episodic",
            MemoryType::Semantic => "semantic",
            MemoryType::Procedural => "procedural",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "working" => Some(MemoryType::Working),
            "episodic" => Some(MemoryType::Episodic),
            "semantic" => Some(MemoryType::Semantic),
            "procedural" => Some(MemoryType::Procedural),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    System,
    Merchant,
}

impl MemoryScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryScope::System => "system",
            MemoryScope::Merchant => "merchant",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Unverified,
    Verified,
    Rejected,
}

impl VerificationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            VerificationStatus::Unverified => "unverified",
            VerificationStatus::Verified => "verified",
            VerificationStatus::Rejected => "rejected",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "unverified" => Some(VerificationStatus::Unverified),
            "verified" => Some(VerificationStatus::Verified),
            "rejected" => Some(VerificationStatus::Rejected),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub memory_id: String,
    pub memory_type: MemoryType,
    pub scope: MemoryScope,
    pub merchant_id: Option<String>,
    pub provider: Option<String>,
    pub incident_category: String,
    pub lifecycle_state: Option<CanonicalPaymentState>,
    pub title: String,
    pub summary: String,
    pub resolution: Option<String>,
    pub source_refs: Vec<String>,
    pub verification_status: VerificationStatus,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl MemoryRecord {
    /// Enforces memory safety & scope invariants before persistence:
    /// - Scrubs secrets/PANs/CVVs/tokens/PII and strips `<think>` blocks.
    /// - `System` (`Semantic`/`Procedural`) memory CANNOT carry a `merchant_id`.
    /// - `Merchant` (`Episodic`) memory MUST carry a valid non-empty `merchant_id`.
    /// - `Episodic` memory MUST be `VerificationStatus::Verified` to enter persistent episodic store.
    pub fn sanitize_and_validate(mut self) -> Result<Self, InferenceError> {
        self.title = privacy::truncate_utf8(
            &privacy::redact_sensitive_text(&privacy::strip_chain_of_thought(&self.title)).redacted,
            160,
        );
        self.summary = privacy::truncate_utf8(
            &privacy::redact_sensitive_text(&privacy::strip_chain_of_thought(&self.summary))
                .redacted,
            1024,
        );
        self.resolution = self.resolution.map(|r| {
            privacy::truncate_utf8(
                &privacy::redact_sensitive_text(&privacy::strip_chain_of_thought(&r)).redacted,
                512,
            )
        });
        self.source_refs = self
            .source_refs
            .into_iter()
            .map(|s| {
                privacy::truncate_utf8(&privacy::redact_sensitive_text(s.trim()).redacted, 128)
            })
            .filter(|s| !s.is_empty())
            .take(16)
            .collect();

        if self.title.trim().is_empty() || self.summary.trim().is_empty() {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Memory title and summary cannot be empty",
            ));
        }

        match self.scope {
            MemoryScope::System => {
                if self.merchant_id.is_some() {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidScope,
                        "System memory cannot contain a merchant_id",
                    ));
                }
                if self.memory_type == MemoryType::Episodic {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidScope,
                        "Episodic merchant incidents must be Merchant-scoped, not System-scoped",
                    ));
                }
            }
            MemoryScope::Merchant => {
                let raw_mid = self.merchant_id.as_deref().unwrap_or("");
                let scope_check = ExecutionScope::MerchantScoped {
                    merchant_id: raw_mid.to_string(),
                };
                let clean_mid = scope_check.normalized_merchant_id()?.unwrap().to_string();
                self.merchant_id = Some(clean_mid);

                if matches!(
                    self.memory_type,
                    MemoryType::Semantic | MemoryType::Procedural
                ) {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidScope,
                        "Semantic and Procedural knowledge must be System-scoped",
                    ));
                }
                if self.memory_type == MemoryType::Episodic
                    && self.verification_status != VerificationStatus::Verified
                {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "Unverified diagnoses cannot be persisted into verified episodic memory",
                    ));
                }
            }
        }

        Ok(self)
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|exp| exp <= now)
    }
}

#[derive(Debug, Clone)]
pub struct MemoryQuery {
    pub scope: ExecutionScope,
    pub memory_types: Vec<MemoryType>,
    pub provider: Option<String>,
    pub incident_category: Option<String>,
    pub lifecycle_state: Option<CanonicalPaymentState>,
    pub since: Option<DateTime<Utc>>,
    pub limit: usize,
}

impl MemoryQuery {
    pub fn for_context(ctx: &PaymentIntelligenceContext, limit: usize) -> Self {
        Self {
            scope: ctx.scope.clone(),
            memory_types: vec![
                MemoryType::Episodic,
                MemoryType::Semantic,
                MemoryType::Procedural,
            ],
            provider: Some(ctx.identity.provider.clone()),
            incident_category: None,
            lifecycle_state: Some(ctx.authoritative_state),
            since: Some(Utc::now() - Duration::days(90)),
            limit: limit.clamp(1, 20),
        }
    }
}

/// Request-scoped, merchant-isolated working memory cleared after an investigation finishes.
#[derive(Debug, Clone)]
pub struct WorkingMemory {
    pub investigation_id: String,
    pub scope: ExecutionScope,
    pub payment_reference: String,
    pub observed_evidence: Vec<String>,
    pub step_notes: BTreeMap<String, String>,
    pub active: bool,
}

impl WorkingMemory {
    pub fn new(ctx: &PaymentIntelligenceContext) -> Result<Self, InferenceError> {
        let _ = ctx.scope.normalized_merchant_id()?;
        Ok(Self {
            investigation_id: ctx.investigation_id.clone(),
            scope: ctx.scope.clone(),
            payment_reference: ctx
                .identity
                .payment_reference
                .clone()
                .unwrap_or_else(|| ctx.identity.transaction_id.clone()),
            observed_evidence: ctx.allowed_evidence_refs.clone(),
            step_notes: BTreeMap::new(),
            active: true,
        })
    }

    pub fn record_step(&mut self, step: &str, note: &str) {
        if self.active {
            let clean = privacy::truncate_utf8(
                &privacy::redact_sensitive_text(&privacy::strip_chain_of_thought(note)).redacted,
                256,
            );
            self.step_notes.insert(step.to_string(), clean);
        }
    }

    pub fn clear(&mut self) {
        self.observed_evidence.clear();
        self.step_notes.clear();
        self.active = false;
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeAccuracy {
    Pending,
    Correct,
    PartiallyCorrect,
    Incorrect,
}

impl OutcomeAccuracy {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutcomeAccuracy::Pending => "pending",
            OutcomeAccuracy::Correct => "correct",
            OutcomeAccuracy::PartiallyCorrect => "partially_correct",
            OutcomeAccuracy::Incorrect => "incorrect",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(OutcomeAccuracy::Pending),
            "correct" => Some(OutcomeAccuracy::Correct),
            "partially_correct" => Some(OutcomeAccuracy::PartiallyCorrect),
            "incorrect" => Some(OutcomeAccuracy::Incorrect),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeEvaluationRecord {
    pub evaluation_id: String,
    pub investigation_id: String,
    pub merchant_id: String,
    pub payment_reference: String,
    pub provider: String,
    pub recommended_root_cause: String,
    #[serde(default)]
    pub supporting_evidence_refs: Vec<String>,
    pub inference_provider: ProviderKind,
    pub model_id: String,
    pub deterministic_fallback_used: bool,
    pub observed_authoritative_state: CanonicalPaymentState,
    pub verified_outcome_state: Option<CanonicalPaymentState>,
    pub accuracy: OutcomeAccuracy,
    pub promoted_memory_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl OutcomeEvaluationRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new_pending(
        investigation_id: String,
        merchant_id: String,
        payment_reference: String,
        provider: String,
        recommended_root_cause: String,
        inference_provider: ProviderKind,
        model_id: String,
        deterministic_fallback_used: bool,
        observed_authoritative_state: CanonicalPaymentState,
    ) -> Self {
        let now = Utc::now();
        Self {
            evaluation_id: format!("eval_{}", Uuid::new_v4().simple()),
            investigation_id,
            merchant_id,
            payment_reference,
            provider,
            recommended_root_cause,
            supporting_evidence_refs: Vec::new(),
            inference_provider,
            model_id,
            deterministic_fallback_used,
            observed_authoritative_state,
            verified_outcome_state: None,
            accuracy: OutcomeAccuracy::Pending,
            promoted_memory_id: None,
            created_at: now,
            verified_at: None,
            expires_at: Some(now + Duration::days(90)),
        }
    }

    pub fn with_evidence_refs(mut self, refs: Vec<String>) -> Self {
        self.supporting_evidence_refs = refs;
        self
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|exp| exp <= now)
    }
}

/// Sealed authoritative outcome proof derived strictly inside `memory` from ESAPay's
/// durable `WebhookStore` payment, webhook, and refund records.
/// External callers cannot construct this proof using arbitrary payment records.
#[derive(Debug, Clone)]
pub struct AuthoritativeOutcomeProof {
    evaluation_id: String,
    merchant_id: String,
    payment_reference: String,
    authoritative_state: CanonicalPaymentState,
    accuracy: OutcomeAccuracy,
    verified_resolution: String,
}

impl AuthoritativeOutcomeProof {
    #[allow(dead_code)]
    pub(super) fn from_authoritative_records(
        scope: &ExecutionScope,
        eval: &OutcomeEvaluationRecord,
        attempt: &PaymentAttemptRecord,
        payment_entity: Option<&PaymentEntity>,
        inbox_events: &[WebhookInboxRecord],
        refunds: &[RefundRecord],
    ) -> Result<Self, InferenceError> {
        let caller_mid = scope.normalized_merchant_id()?.ok_or_else(|| {
            InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "System scope cannot verify merchant-scoped evaluations",
            )
        })?;
        if eval.merchant_id != caller_mid || attempt.merchant_id != caller_mid {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Authoritative payment record or evaluation does not belong to authenticated merchant",
            ));
        }
        if eval.is_expired(Utc::now()) {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Cannot verify expired evaluation record",
            ));
        }
        let matches_ref = attempt.payment_reference.as_deref()
            == Some(eval.payment_reference.as_str())
            || attempt.transaction_id == eval.payment_reference;
        if !matches_ref {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                "Authoritative payment attempt does not match evaluation payment_reference",
            ));
        }

        let authoritative_state = match payment_entity {
            Some(pe) => CanonicalPaymentState::from_payment_state(pe.state),
            None => CanonicalPaymentState::from_attempt_state(&attempt.state),
        };

        // Non-terminal outcomes cannot be promoted; keep the evaluation pending
        if !matches!(
            authoritative_state,
            CanonicalPaymentState::Captured
                | CanonicalPaymentState::PartiallyRefunded
                | CanonicalPaymentState::Refunded
                | CanonicalPaymentState::Failed
        ) {
            return Err(InferenceError::new(
                InferenceErrorCategory::InvalidResponse,
                format!(
                    "Authoritative payment state '{}' is non-terminal; keeping evaluation pending",
                    authoritative_state.as_str()
                ),
            ));
        }

        // Compute deterministic evidence facts from authoritative records
        let mut delayed_event_ids = HashSet::new();
        let mut provider_timeout_event_ids = HashSet::new();
        let mut provider_failed_event_ids = HashSet::new();

        for ev in inbox_events {
            if ev
                .received_at
                .signed_duration_since(attempt.created_at)
                .num_seconds()
                > 300
            {
                delayed_event_ids.insert(ev.provider_event_id.clone());
            }
            let et = ev.event_type.to_ascii_lowercase();
            let err_msg = ev
                .error_message
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase();
            let payload_str = ev.payload.to_string().to_ascii_lowercase();
            let is_provider_timeout_or_outage = et.contains("timeout")
                || et.contains("gateway_error")
                || err_msg.contains("provider timeout")
                || err_msg.contains("gateway timeout")
                || err_msg.contains("502")
                || err_msg.contains("503")
                || err_msg.contains("504")
                || payload_str.contains("gateway_timeout");
            if is_provider_timeout_or_outage {
                provider_timeout_event_ids.insert(ev.provider_event_id.clone());
            }
            let is_provider_failure_event = (et.contains("failed") || et.contains("failure"))
                && ev.status != esa_state::WebhookStatus::Failed;
            if is_provider_failure_event {
                provider_failed_event_ids.insert(ev.provider_event_id.clone());
            }
        }

        let captured_amount_minor = match payment_entity {
            Some(pe)
                if matches!(
                    pe.state,
                    PaymentState::Captured
                        | PaymentState::PartiallyRefunded
                        | PaymentState::Refunded
                ) =>
            {
                pe.amount_minor
            }
            _ => 0,
        };
        let processed_refunds_minor = payment_entity
            .map(|pe| pe.refunded_amount_minor)
            .unwrap_or(0);
        let pending_reservations_minor: u64 = refunds
            .iter()
            .filter(|r| !r.applied_to_balance && r.status.eq_ignore_ascii_case("pending"))
            .map(|r| r.amount_minor)
            .fold(0u64, |acc, x| acc.saturating_add(x));
        let remaining_refundable_minor = captured_amount_minor
            .saturating_sub(processed_refunds_minor)
            .saturating_sub(pending_reservations_minor);

        let has_overage_refund = captured_amount_minor > 0
            && refunds
                .iter()
                .any(|r| !r.applied_to_balance && r.amount_minor > remaining_refundable_minor);

        let cited = &eval.supporting_evidence_refs;
        let has_cited_match = |prefix: &str, valid_ids: &HashSet<String>| {
            if cited.is_empty() {
                !valid_ids.is_empty()
            } else {
                cited.iter().any(|r| {
                    r.strip_prefix(prefix)
                        .is_some_and(|id| valid_ids.contains(id))
                })
            }
        };

        // Diagnosis-level verification against specific authoritative evidence (never terminal state alone)
        let rc = eval.recommended_root_cause.as_str();
        let accuracy = match rc {
            "DUPLICATE_WEBHOOK_SUPPRESSED" => {
                // webhook_inbox enforces UNIQUE(provider, provider_event_id) for idempotency and
                // does not persist a durable duplicate-delivery log; keep unsupported until one exists.
                return Err(InferenceError::new(
                    InferenceErrorCategory::InvalidResponse,
                    "DUPLICATE_WEBHOOK_SUPPRESSED cannot be independently promoted without durable duplicate-delivery evidence; keeping evaluation pending",
                ));
            }
            "WEBHOOK_DELAYED_OR_MISSING" => {
                let delayed_ok = has_cited_match("webhook:", &delayed_event_ids);
                if !delayed_ok {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "WEBHOOK_DELAYED_OR_MISSING requires independently verified delayed-event evidence; empty webhook_inbox alone is insufficient, keeping evaluation pending",
                    ));
                }
                OutcomeAccuracy::Correct
            }
            "REFUND_RESERVATION_PENDING" => {
                if pending_reservations_minor == 0 {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "REFUND_RESERVATION_PENDING has no pending reservation in authoritative refund records; keeping evaluation pending",
                    ));
                }
                OutcomeAccuracy::Correct
            }
            "REFUND_EXCEEDS_REMAINING_BALANCE" => {
                if !has_overage_refund {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "REFUND_EXCEEDS_REMAINING_BALANCE has no over-balance refund in authoritative records; keeping evaluation pending",
                    ));
                }
                OutcomeAccuracy::Correct
            }
            "PROVIDER_DEGRADED_OR_TIMEOUT" => {
                let has_provider_timeout = !provider_timeout_event_ids.is_empty()
                    || (authoritative_state == CanonicalPaymentState::Failed
                        && attempt.state.eq_ignore_ascii_case("timeout"));
                if !has_provider_timeout {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "PROVIDER_DEGRADED_OR_TIMEOUT lacks authoritative provider timeout/degradation evidence; keeping evaluation pending",
                    ));
                }
                OutcomeAccuracy::Correct
            }
            "PROVIDER_STATE_DISCREPANCY" => {
                let attempt_canon = CanonicalPaymentState::from_attempt_state(&attempt.state);
                if payment_entity.is_none()
                    || attempt_canon == CanonicalPaymentState::Unknown
                    || attempt_canon == authoritative_state
                {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "PROVIDER_STATE_DISCREPANCY has no conflicting authoritative vs attempt state; keeping evaluation pending",
                    ));
                }
                let expected_state_ref = format!("state:{}", authoritative_state.as_str());
                let expected_payment_ref = format!("payment:{}", eval.payment_reference);
                let expected_attempt_ref = format!("attempt_state:{}", attempt.state.trim());
                let expected_attempt_canon_ref =
                    format!("attempt_state:{}", attempt_canon.as_str());

                let cites_auth = cited
                    .iter()
                    .any(|r| r == &expected_state_ref || r == &expected_payment_ref);
                let cites_conflict = cited.iter().any(|r| {
                    r == &expected_attempt_ref
                        || r == &expected_attempt_canon_ref
                        || r.strip_prefix("provider_observed:").is_some_and(|obs| {
                            let c = CanonicalPaymentState::from_attempt_state(obs);
                            c != CanonicalPaymentState::Unknown && c != authoritative_state
                        })
                });
                if !cites_auth || !cites_conflict {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "PROVIDER_STATE_DISCREPANCY evidence refs do not match the actual conflicting authoritative and attempt/observed states; keeping evaluation pending",
                    ));
                }
                OutcomeAccuracy::Correct
            }
            "HEALTHY_COMPLETED" => match authoritative_state {
                CanonicalPaymentState::Captured
                | CanonicalPaymentState::PartiallyRefunded
                | CanonicalPaymentState::Refunded
                    if pending_reservations_minor == 0 =>
                {
                    OutcomeAccuracy::Correct
                }
                CanonicalPaymentState::Failed => OutcomeAccuracy::Incorrect,
                _ => {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "HEALTHY_COMPLETED cannot be verified while reservations or non-terminal states remain; keeping evaluation pending",
                    ));
                }
            },
            "PAYMENT_FAILED_AT_PROVIDER" => match authoritative_state {
                CanonicalPaymentState::Failed => {
                    if !has_cited_match("webhook:", &provider_failed_event_ids) {
                        return Err(InferenceError::new(
                            InferenceErrorCategory::InvalidResponse,
                            "PAYMENT_FAILED_AT_PROVIDER lacks matching durable provider-originated failure event evidence; keeping evaluation pending",
                        ));
                    }
                    OutcomeAccuracy::Correct
                }
                CanonicalPaymentState::Captured
                | CanonicalPaymentState::PartiallyRefunded
                | CanonicalPaymentState::Refunded => OutcomeAccuracy::Incorrect,
                _ => {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "PAYMENT_FAILED_AT_PROVIDER is not supported by authoritative state; keeping evaluation pending",
                    ));
                }
            },
            "AUTHORIZATION_NOT_CAPTURED"
            | "PROVIDER_CAPTURE_PENDING"
            | "RECONCILIATION_REQUIRED" => match authoritative_state {
                CanonicalPaymentState::Captured
                | CanonicalPaymentState::PartiallyRefunded
                | CanonicalPaymentState::Refunded
                | CanonicalPaymentState::Failed => OutcomeAccuracy::PartiallyCorrect,
                _ => {
                    return Err(InferenceError::new(
                        InferenceErrorCategory::InvalidResponse,
                        "Evaluation remains pending until authoritative payment resolves",
                    ));
                }
            },
            _ => {
                return Err(InferenceError::new(
                    InferenceErrorCategory::InvalidResponse,
                    format!(
                        "Diagnosis '{}' cannot be independently verified from authoritative records; keeping evaluation pending",
                        rc
                    ),
                ));
            }
        };

        let verified_resolution = format!(
            "Authoritative {} payment {} verified '{}' for diagnosis '{}' (source: {})",
            attempt.provider,
            eval.payment_reference,
            authoritative_state.as_str(),
            rc,
            if payment_entity.is_some() {
                "payment_records+webhook_inbox+refunds"
            } else {
                "payment_attempts+webhook_inbox+refunds"
            }
        );

        Ok(Self {
            evaluation_id: eval.evaluation_id.clone(),
            merchant_id: caller_mid.to_string(),
            payment_reference: eval.payment_reference.clone(),
            authoritative_state,
            accuracy,
            verified_resolution,
        })
    }

    pub fn evaluation_id(&self) -> &str {
        &self.evaluation_id
    }

    pub fn merchant_id(&self) -> &str {
        &self.merchant_id
    }

    pub fn payment_reference(&self) -> &str {
        &self.payment_reference
    }

    pub fn authoritative_state(&self) -> CanonicalPaymentState {
        self.authoritative_state
    }

    pub fn accuracy(&self) -> OutcomeAccuracy {
        self.accuracy
    }

    pub fn verified_resolution(&self) -> &str {
        &self.verified_resolution
    }
}
