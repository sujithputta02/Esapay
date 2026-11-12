use crate::privacy;
use crate::provider::{ExecutionScope, InferenceError, InferenceErrorCategory};
use chrono::{DateTime, Utc};
use esa_core::payment_state::{PaymentEntity, PaymentState};
use esa_core::provider::{PaymentAttemptRecord, ProviderHealth};
use esa_state::webhook_store::{RefundRecord, WebhookInboxRecord, WebhookStatus};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use uuid::Uuid;

/// Normalized payment lifecycle state used by Payment Intelligence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalPaymentState {
    Created,
    Authorized,
    Captured,
    Failed,
    Pending,
    PendingReconciliation,
    PartiallyRefunded,
    Refunded,
    Unknown,
}

impl CanonicalPaymentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            CanonicalPaymentState::Created => "created",
            CanonicalPaymentState::Authorized => "authorized",
            CanonicalPaymentState::Captured => "captured",
            CanonicalPaymentState::Failed => "failed",
            CanonicalPaymentState::Pending => "pending",
            CanonicalPaymentState::PendingReconciliation => "pending_reconciliation",
            CanonicalPaymentState::PartiallyRefunded => "partially_refunded",
            CanonicalPaymentState::Refunded => "refunded",
            CanonicalPaymentState::Unknown => "unknown",
        }
    }

    pub fn from_payment_state(state: PaymentState) -> Self {
        match state {
            PaymentState::Created => CanonicalPaymentState::Created,
            PaymentState::Authorized => CanonicalPaymentState::Authorized,
            PaymentState::Captured => CanonicalPaymentState::Captured,
            PaymentState::Failed => CanonicalPaymentState::Failed,
            PaymentState::PendingReconciliation => CanonicalPaymentState::PendingReconciliation,
            PaymentState::PartiallyRefunded => CanonicalPaymentState::PartiallyRefunded,
            PaymentState::Refunded => CanonicalPaymentState::Refunded,
        }
    }

    pub fn from_attempt_state(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "created" => CanonicalPaymentState::Created,
            "authorized" => CanonicalPaymentState::Authorized,
            "captured" => CanonicalPaymentState::Captured,
            "failed" => CanonicalPaymentState::Failed,
            "pending" | "active" => CanonicalPaymentState::Pending,
            "pending_reconciliation" => CanonicalPaymentState::PendingReconciliation,
            "partially_refunded" => CanonicalPaymentState::PartiallyRefunded,
            "refunded" => CanonicalPaymentState::Refunded,
            _ => CanonicalPaymentState::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentIdentityContext {
    pub transaction_id: String,
    pub payment_reference: Option<String>,
    pub provider: String,
    pub provider_order_id: Option<String>,
    pub merchant_id: String,
    pub currency: String,
    pub amount_minor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentEventTimelineItem {
    pub timestamp: DateTime<Utc>,
    pub source: String,
    pub event_type: String,
    pub status: String,
    pub event_id: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderObservationContext {
    pub provider: String,
    pub observed_status: Option<String>,
    pub provider_healthy: Option<bool>,
    pub p95_latency_ms: Option<f64>,
    pub success_rate: Option<f64>,
    pub webhook_delivery_count: usize,
    pub duplicate_webhook_count: usize,
    pub delayed_webhook_detected: bool,
    pub discrepancy_detected: bool,
    pub missing_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundContextItem {
    pub refund_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub status: String,
    pub applied_to_balance: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundSummaryContext {
    pub captured_amount_minor: u64,
    pub processed_refunds_minor: u64,
    pub pending_reservations_minor: u64,
    pub remaining_refundable_minor: u64,
    pub refund_records: Vec<RefundContextItem>,
}

/// Read-only, deterministically constructed payment context for investigation and memory retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentIntelligenceContext {
    pub investigation_id: String,
    pub scope: ExecutionScope,
    pub identity: PaymentIdentityContext,
    pub authoritative_state: CanonicalPaymentState,
    pub attempt_state: String,
    pub events: Vec<PaymentEventTimelineItem>,
    pub provider_observation: ProviderObservationContext,
    pub refund_summary: RefundSummaryContext,
    pub allowed_evidence_refs: Vec<String>,
    pub missing_data: Vec<String>,
}

/// Builds a canonical `PaymentIntelligenceContext` strictly from authoritative ESAPay records.
/// Enforces merchant ownership when `scope` is `MerchantScoped`.
pub fn build_payment_intelligence_context(
    scope: &ExecutionScope,
    attempt: &PaymentAttemptRecord,
    payment_entity: Option<&PaymentEntity>,
    inbox_events: &[WebhookInboxRecord],
    refunds: &[RefundRecord],
    provider_health: Option<&ProviderHealth>,
    observed_provider_status: Option<&str>,
) -> Result<PaymentIntelligenceContext, InferenceError> {
    if let Some(caller_merchant) = scope.normalized_merchant_id()? {
        if attempt.merchant_id != caller_merchant {
            return Err(InferenceError::new(
                InferenceErrorCategory::Forbidden,
                "Payment attempt does not belong to the authenticated merchant",
            ));
        }
    }

    let mut missing_data = Vec::new();
    let mut allowed_refs = BTreeSet::new();

    allowed_refs.insert(format!("attempt:{}", attempt.transaction_id));
    allowed_refs.insert(format!("attempt_state:{}", attempt.state));
    allowed_refs.insert(format!("provider:{}", attempt.provider));
    if let Some(ref oid) = attempt.provider_order_id {
        allowed_refs.insert(format!("order:{}", oid));
    }
    if let Some(ref pid) = attempt.payment_reference {
        allowed_refs.insert(format!("payment:{}", pid));
    } else {
        missing_data.push("payment_reference_missing".to_string());
    }

    // Never infer Captured from Authorized, Pending, or Created attempt state.
    let authoritative_state = match payment_entity {
        Some(pe) => CanonicalPaymentState::from_payment_state(pe.state),
        None => {
            missing_data.push("durable_payment_record_missing".to_string());
            CanonicalPaymentState::from_attempt_state(&attempt.state)
        }
    };
    allowed_refs.insert(format!("state:{}", authoritative_state.as_str()));

    if let Some(pe) = payment_entity {
        allowed_refs.insert(format!("payment:{}", pe.payment_id));
    }

    // Build chronological event timeline
    let mut events = Vec::new();
    events.push(PaymentEventTimelineItem {
        timestamp: attempt.created_at,
        source: "payment_attempt".to_string(),
        event_type: "attempt_created".to_string(),
        status: attempt.state.clone(),
        event_id: attempt.transaction_id.clone(),
        summary: format!(
            "Initiated {} payment for {} {}",
            attempt.provider, attempt.amount_minor, attempt.currency
        ),
    });

    let mut seen_event_ids = HashSet::new();
    let mut duplicate_webhook_count = 0usize;
    let mut delayed_webhook_detected = false;

    for ev in inbox_events {
        if !seen_event_ids.insert(ev.provider_event_id.clone()) {
            duplicate_webhook_count += 1;
        }
        if ev
            .received_at
            .signed_duration_since(attempt.created_at)
            .num_seconds()
            > 300
        {
            delayed_webhook_detected = true;
        }
        let status_str = match ev.status {
            WebhookStatus::Processed => "processed",
            WebhookStatus::Pending => "pending",
            WebhookStatus::Failed => "failed",
            WebhookStatus::Unapplied => "unapplied",
        };
        allowed_refs.insert(format!("webhook:{}", ev.provider_event_id));
        let err_note = ev
            .error_message
            .as_deref()
            .map(|e| format!(" ({})", privacy::redact_sensitive_text(e).redacted))
            .unwrap_or_default();
        events.push(PaymentEventTimelineItem {
            timestamp: ev.received_at,
            source: "webhook_inbox".to_string(),
            event_type: ev.event_type.clone(),
            status: status_str.to_string(),
            event_id: ev.provider_event_id.clone(),
            summary: privacy::truncate_utf8(
                &format!(
                    "{} {} -> {}{}",
                    ev.provider, ev.event_type, status_str, err_note
                ),
                200,
            ),
        });
    }

    if inbox_events.is_empty() {
        missing_data.push("webhook_events_missing".to_string());
    }

    // Deterministic refund summary calculation (never delegated to LLM)
    let captured_amount_minor = match payment_entity {
        Some(pe)
            if matches!(
                pe.state,
                PaymentState::Captured | PaymentState::PartiallyRefunded | PaymentState::Refunded
            ) =>
        {
            pe.amount_minor
        }
        _ => 0,
    };

    let processed_refunds_minor: u64 = payment_entity
        .map(|pe| pe.refunded_amount_minor)
        .unwrap_or(0);

    let mut pending_reservations_minor = 0u64;
    let mut refund_items = Vec::with_capacity(refunds.len());
    for rf in refunds {
        allowed_refs.insert(format!("refund:{}", rf.refund_id));
        if !rf.applied_to_balance && rf.status.eq_ignore_ascii_case("pending") {
            pending_reservations_minor = pending_reservations_minor.saturating_add(rf.amount_minor);
        }
        refund_items.push(RefundContextItem {
            refund_id: rf.refund_id.clone(),
            amount_minor: rf.amount_minor,
            currency: rf.currency.clone(),
            status: rf.status.clone(),
            applied_to_balance: rf.applied_to_balance,
        });
    }
    if !refunds.is_empty() || processed_refunds_minor > 0 {
        allowed_refs.insert("refund_summary:remaining_refundable_minor".to_string());
    }

    let remaining_refundable_minor = captured_amount_minor
        .saturating_sub(processed_refunds_minor)
        .saturating_sub(pending_reservations_minor);

    // Check discrepancy between durable PaymentEntity state, attempt state, or observed provider status
    let mut discrepancy_detected = false;
    let attempt_canon = CanonicalPaymentState::from_attempt_state(&attempt.state);
    if payment_entity.is_some() && attempt_canon != authoritative_state {
        discrepancy_detected = true;
    }
    if let Some(obs) = observed_provider_status {
        let obs_canon = CanonicalPaymentState::from_attempt_state(obs);
        if obs_canon != CanonicalPaymentState::Unknown && obs_canon != authoritative_state {
            discrepancy_detected = true;
        }
        allowed_refs.insert(format!(
            "provider_observed:{}",
            obs.trim().to_ascii_lowercase()
        ));
    } else {
        missing_data.push("provider_observed_status_missing".to_string());
    }

    let mut obs_missing = Vec::new();
    if provider_health.is_none() {
        obs_missing.push("provider_health_snapshot".to_string());
        missing_data.push("provider_health_snapshot_missing".to_string());
    } else {
        allowed_refs.insert(format!("provider_health:{}", attempt.provider));
    }
    if observed_provider_status.is_none() {
        obs_missing.push("observed_provider_status".to_string());
    }

    events.sort_by_key(|a| a.timestamp);

    Ok(PaymentIntelligenceContext {
        investigation_id: format!("inv_{}", Uuid::new_v4().simple()),
        scope: scope.clone(),
        identity: PaymentIdentityContext {
            transaction_id: attempt.transaction_id.clone(),
            payment_reference: attempt.payment_reference.clone(),
            provider: attempt.provider.clone(),
            provider_order_id: attempt.provider_order_id.clone(),
            merchant_id: attempt.merchant_id.clone(),
            currency: attempt.currency.clone(),
            amount_minor: attempt.amount_minor,
        },
        authoritative_state,
        attempt_state: attempt.state.clone(),
        events,
        provider_observation: ProviderObservationContext {
            provider: attempt.provider.clone(),
            observed_status: observed_provider_status.map(|s| s.to_string()),
            provider_healthy: provider_health.map(|h| h.is_healthy),
            p95_latency_ms: provider_health.map(|h| h.p95_latency_ms),
            success_rate: provider_health.map(|h| h.success_rate),
            webhook_delivery_count: inbox_events.len(),
            duplicate_webhook_count,
            delayed_webhook_detected,
            discrepancy_detected,
            missing_fields: obs_missing,
        },
        refund_summary: RefundSummaryContext {
            captured_amount_minor,
            processed_refunds_minor,
            pending_reservations_minor,
            remaining_refundable_minor,
            refund_records: refund_items,
        },
        allowed_evidence_refs: allowed_refs.into_iter().collect(),
        missing_data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use esa_core::types::{PaymentGateway, PaymentMethodClass};
    use esa_core::WorkloadState;

    fn scope_for(merchant_id: &str) -> ExecutionScope {
        ExecutionScope::MerchantScoped {
            merchant_id: merchant_id.to_string(),
        }
    }

    fn make_attempt(
        merchant_id: &str,
        state: &str,
        payment_ref: Option<&str>,
    ) -> PaymentAttemptRecord {
        let now = Utc::now();
        PaymentAttemptRecord {
            transaction_id: "tx_001".to_string(),
            merchant_id: merchant_id.to_string(),
            provider: "razorpay".to_string(),
            provider_order_id: Some("order_001".to_string()),
            payment_reference: payment_ref.map(|s| s.to_string()),
            amount_minor: 50000,
            currency: "INR".to_string(),
            state: state.to_string(),
            created_at: now - chrono::Duration::minutes(10),
            updated_at: now,
        }
    }

    fn make_entity(payment_id: &str, state: PaymentState, refunded: u64) -> PaymentEntity {
        let now = Utc::now();
        PaymentEntity {
            payment_id: payment_id.to_string(),
            state,
            amount_minor: 50000,
            currency: "INR".to_string(),
            refunded_amount_minor: refunded,
            events_seen: vec!["evt_1".to_string()],
            refunds: Vec::new(),
            created_at: now - chrono::Duration::minutes(10),
            updated_at: now,
        }
    }

    fn make_webhook(
        id: i64,
        event_id: &str,
        event_type: &str,
        status: WebhookStatus,
        received_at: DateTime<Utc>,
        processed_at: Option<DateTime<Utc>>,
    ) -> WebhookInboxRecord {
        WebhookInboxRecord {
            id,
            provider: "razorpay".to_string(),
            provider_event_id: event_id.to_string(),
            event_type: event_type.to_string(),
            payment_id: Some("pay_001".to_string()),
            refund_id: None,
            payload: serde_json::json!({"event": event_type}),
            status,
            error_message: None,
            received_at,
            processed_at,
        }
    }

    fn make_health(healthy: bool) -> ProviderHealth {
        ProviderHealth {
            gateway: PaymentGateway::Razorpay,
            name: "Razorpay".to_string(),
            is_configured: true,
            is_healthy: healthy,
            is_sandbox: true,
            status: if healthy {
                WorkloadState::Healthy
            } else {
                WorkloadState::Degraded
            },
            p95_latency_ms: 210.0,
            success_rate: if healthy { 0.99 } else { 0.72 },
            active_traffic_pct: 50.0,
            supported_methods: vec![PaymentMethodClass::Upi, PaymentMethodClass::Card],
            message: "ok".to_string(),
        }
    }

    #[test]
    fn test_context_captured_payment() {
        let scope = scope_for("merchant_a");
        let attempt = make_attempt("merchant_a", "captured", Some("pay_001"));
        let entity = make_entity("pay_001", PaymentState::Captured, 0);
        let health = make_health(true);
        let ctx = build_payment_intelligence_context(
            &scope,
            &attempt,
            Some(&entity),
            &[],
            &[],
            Some(&health),
            Some("captured"),
        )
        .unwrap();

        assert_eq!(ctx.authoritative_state, CanonicalPaymentState::Captured);
        assert_eq!(ctx.refund_summary.captured_amount_minor, 50000);
        assert_eq!(ctx.refund_summary.remaining_refundable_minor, 50000);
        assert!(!ctx.provider_observation.discrepancy_detected);
        assert!(ctx
            .allowed_evidence_refs
            .contains(&"attempt:tx_001".to_string()));
        assert!(ctx
            .allowed_evidence_refs
            .contains(&"payment:pay_001".to_string()));
    }

    #[test]
    fn test_context_authorized_not_captured() {
        let scope = scope_for("merchant_a");
        let attempt = make_attempt("merchant_a", "authorized", Some("pay_001"));
        let entity = make_entity("pay_001", PaymentState::Authorized, 0);
        let ctx = build_payment_intelligence_context(
            &scope,
            &attempt,
            Some(&entity),
            &[],
            &[],
            None,
            None,
        )
        .unwrap();

        assert_eq!(ctx.authoritative_state, CanonicalPaymentState::Authorized);
        assert_eq!(ctx.refund_summary.captured_amount_minor, 0);
        assert_eq!(ctx.refund_summary.remaining_refundable_minor, 0);
    }

    #[test]
    fn test_context_failed_payment() {
        let scope = scope_for("merchant_a");
        let attempt = make_attempt("merchant_a", "failed", Some("pay_001"));
        let entity = make_entity("pay_001", PaymentState::Failed, 0);
        let ctx = build_payment_intelligence_context(
            &scope,
            &attempt,
            Some(&entity),
            &[],
            &[],
            None,
            Some("failed"),
        )
        .unwrap();

        assert_eq!(ctx.authoritative_state, CanonicalPaymentState::Failed);
        assert!(!ctx.provider_observation.discrepancy_detected);
    }

    #[test]
    fn test_context_pending_payment() {
        let scope = scope_for("merchant_a");
        let attempt = make_attempt("merchant_a", "pending", None);
        let ctx = build_payment_intelligence_context(&scope, &attempt, None, &[], &[], None, None)
            .unwrap();

        assert_eq!(ctx.authoritative_state, CanonicalPaymentState::Pending);
        assert!(ctx
            .missing_data
            .iter()
            .any(|m| m.contains("durable_payment_record_missing")));
    }

    #[test]
    fn test_context_unknown_state_and_cross_merchant_rejected() {
        let scope_a = scope_for("merchant_a");
        let attempt_a = make_attempt("merchant_a", "weird_gateway_state", None);
        let ctx =
            build_payment_intelligence_context(&scope_a, &attempt_a, None, &[], &[], None, None)
                .unwrap();
        assert_eq!(ctx.authoritative_state, CanonicalPaymentState::Unknown);

        let scope_b = scope_for("merchant_b");
        let err =
            build_payment_intelligence_context(&scope_b, &attempt_a, None, &[], &[], None, None)
                .unwrap_err();
        assert_eq!(err.category, InferenceErrorCategory::Forbidden);
    }

    #[test]
    fn test_context_duplicate_webhook() {
        let scope = scope_for("merchant_a");
        let attempt = make_attempt("merchant_a", "captured", Some("pay_001"));
        let now = Utc::now();
        let events = vec![
            make_webhook(
                1,
                "evt_1",
                "payment.captured",
                WebhookStatus::Processed,
                now,
                Some(now),
            ),
            make_webhook(
                2,
                "evt_1",
                "payment.captured",
                WebhookStatus::Processed,
                now,
                Some(now),
            ),
        ];
        let ctx =
            build_payment_intelligence_context(&scope, &attempt, None, &events, &[], None, None)
                .unwrap();
        assert_eq!(ctx.provider_observation.duplicate_webhook_count, 1);
        assert_eq!(ctx.provider_observation.webhook_delivery_count, 2);
    }

    #[test]
    fn test_context_delayed_webhook() {
        let scope = scope_for("merchant_a");
        let attempt = make_attempt("merchant_a", "captured", Some("pay_001"));
        let t0 = Utc::now() - chrono::Duration::minutes(2);
        let t1 = t0 + chrono::Duration::seconds(10);
        let events = vec![make_webhook(
            1,
            "evt_delayed",
            "payment.captured",
            WebhookStatus::Processed,
            t0,
            Some(t1),
        )];
        let ctx =
            build_payment_intelligence_context(&scope, &attempt, None, &events, &[], None, None)
                .unwrap();
        assert!(ctx.provider_observation.delayed_webhook_detected);
    }

    #[test]
    fn test_context_payment_refund_discrepancy() {
        let scope = scope_for("merchant_a");
        let attempt = make_attempt("merchant_a", "authorized", Some("pay_001"));
        let entity = make_entity("pay_001", PaymentState::Authorized, 0);
        let ctx = build_payment_intelligence_context(
            &scope,
            &attempt,
            Some(&entity),
            &[],
            &[],
            None,
            Some("captured"),
        )
        .unwrap();
        assert!(ctx.provider_observation.discrepancy_detected);
    }

    #[test]
    fn test_context_concurrent_refund_reservations() {
        let scope = scope_for("merchant_a");
        let attempt = make_attempt("merchant_a", "captured", Some("pay_001"));
        let entity = make_entity("pay_001", PaymentState::PartiallyRefunded, 20000);
        let refunds = vec![
            RefundRecord {
                provider: "razorpay".to_string(),
                refund_id: "rfnd_1".to_string(),
                payment_id: "pay_001".to_string(),
                amount_minor: 20000,
                currency: "INR".to_string(),
                status: "processed".to_string(),
                applied_to_balance: true,
            },
            RefundRecord {
                provider: "razorpay".to_string(),
                refund_id: "rfnd_2".to_string(),
                payment_id: "pay_001".to_string(),
                amount_minor: 15000,
                currency: "INR".to_string(),
                status: "pending".to_string(),
                applied_to_balance: false,
            },
        ];
        let ctx = build_payment_intelligence_context(
            &scope,
            &attempt,
            Some(&entity),
            &[],
            &refunds,
            None,
            None,
        )
        .unwrap();

        assert_eq!(ctx.refund_summary.captured_amount_minor, 50000);
        assert_eq!(ctx.refund_summary.processed_refunds_minor, 20000);
        assert_eq!(ctx.refund_summary.pending_reservations_minor, 15000);
        assert_eq!(ctx.refund_summary.remaining_refundable_minor, 15000);
        assert!(ctx
            .allowed_evidence_refs
            .contains(&"refund:rfnd_2".to_string()));
    }

    #[test]
    fn test_context_missing_provider_info() {
        let scope = scope_for("merchant_a");
        let attempt = make_attempt("merchant_a", "created", None);
        let ctx = build_payment_intelligence_context(&scope, &attempt, None, &[], &[], None, None)
            .unwrap();
        assert!(ctx
            .provider_observation
            .missing_fields
            .contains(&"provider_health_snapshot".to_string()));
        assert!(ctx
            .provider_observation
            .missing_fields
            .contains(&"observed_provider_status".to_string()));
    }
}
