use crate::memory::store::PaymentMemoryStore;
use crate::memory::types::{
    MemoryQuery, MemoryRecord, MemoryScope, MemoryType, VerificationStatus,
};
use crate::payment_context::{CanonicalPaymentState, PaymentIntelligenceContext};
use crate::provider::InferenceError;
use chrono::Utc;
use std::sync::Arc;

/// Deterministic retriever over `PaymentMemoryStore` enforcing bounded, tenant-isolated reads.
#[derive(Clone)]
pub struct MemoryRetriever {
    store: Arc<dyn PaymentMemoryStore>,
}

impl MemoryRetriever {
    pub fn new(store: Arc<dyn PaymentMemoryStore>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> Arc<dyn PaymentMemoryStore> {
        Arc::clone(&self.store)
    }

    pub async fn retrieve_for_context(
        &self,
        ctx: &PaymentIntelligenceContext,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, InferenceError> {
        let query = MemoryQuery::for_context(ctx, limit);
        self.store.query_memories(&query).await
    }
}

/// Seeds curated, versioned Semantic and Procedural knowledge into `MemoryScope::System`.
/// All seeded procedures describe strictly read-only investigation workflows.
pub async fn seed_system_knowledge(
    store: &dyn PaymentMemoryStore,
) -> Result<usize, InferenceError> {
    let now = Utc::now();
    let records = vec![
        MemoryRecord {
            memory_id: "sem_esapay_lifecycle_v1".to_string(),
            memory_type: MemoryType::Semantic,
            scope: MemoryScope::System,
            merchant_id: None,
            provider: None,
            incident_category: "lifecycle_semantics".to_string(),
            lifecycle_state: None,
            title: "ESAPay Authoritative Payment Lifecycle Rules".to_string(),
            summary: "Authorized means funds are reserved only; only authoritative Capture or OrderPaid evidence transitions durable state to Captured. Delayed failure events cannot downgrade Captured payments."
                .to_string(),
            resolution: Some("Verify durable payment_records state and webhook_inbox status.".to_string()),
            source_refs: vec!["doc:esa-core/payment_state.rs@v1".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: None,
        },
        MemoryRecord {
            memory_id: "sem_razorpay_auth_capture_v1".to_string(),
            memory_type: MemoryType::Semantic,
            scope: MemoryScope::System,
            merchant_id: None,
            provider: Some("razorpay".to_string()),
            incident_category: "authorization_not_captured".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Authorized),
            title: "Razorpay Authorized vs Captured Semantics".to_string(),
            summary: "Razorpay payments may enter 'authorized' before capture. Confirmation endpoint records Authorized without captured workload side effects until Razorpay reports 'captured' or payment.captured webhook arrives."
                .to_string(),
            resolution: Some("Inspect Razorpay dashboard/API payment status and webhook_inbox for payment.captured.".to_string()),
            source_refs: vec!["doc:razorpay/payments-capture@v1".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: None,
        },
        MemoryRecord {
            memory_id: "sem_cashfree_webhook_delay_v1".to_string(),
            memory_type: MemoryType::Semantic,
            scope: MemoryScope::System,
            merchant_id: None,
            provider: Some("cashfree".to_string()),
            incident_category: "webhook_delayed_or_missing".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Pending),
            title: "Cashfree Order Confirmation & Webhook Delivery".to_string(),
            summary: "Cashfree orders remain Pending/Active while customer authorization or asynchronous bank webhook delivery is in flight; confirm endpoint authoritatively queries Cashfree order payments."
                .to_string(),
            resolution: Some("Check webhook_inbox delivery timestamps and Cashfree order payment status.".to_string()),
            source_refs: vec!["doc:cashfree/pg-orders@2023-08-01".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: None,
        },
        MemoryRecord {
            memory_id: "sem_payu_hash_verify_v1".to_string(),
            memory_type: MemoryType::Semantic,
            scope: MemoryScope::System,
            merchant_id: None,
            provider: Some("payu".to_string()),
            incident_category: "provider_state_discrepancy".to_string(),
            lifecycle_state: None,
            title: "PayU Hosted Checkout & Verify Payment Semantics".to_string(),
            summary: "PayU callbacks and webhooks require reverse-hash verification; pending or unknown statuses fail closed until verify_payment confirms settlement."
                .to_string(),
            resolution: Some("Verify PayU txnid status via read-only verification lookup and inspect webhook_inbox.".to_string()),
            source_refs: vec!["doc:payu/hosted-verify@v1".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: None,
        },
        MemoryRecord {
            memory_id: "sem_refund_reservation_v1".to_string(),
            memory_type: MemoryType::Semantic,
            scope: MemoryScope::System,
            merchant_id: None,
            provider: None,
            incident_category: "refund_reservation_pending".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::PendingReconciliation),
            title: "ESAPay Atomic Refund Reservation & Reconciliation".to_string(),
            summary: "Refund reservations deduct available refundable balance atomically before provider refund calls. Refunds arriving before capture enter unapplied/pending_reconciliation until capture settles."
                .to_string(),
            resolution: Some("Inspect payment_refunds applied_to_balance flag and remaining_refundable_minor.".to_string()),
            source_refs: vec!["doc:esa-state/webhook_store.rs@v1".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: None,
        },
        MemoryRecord {
            memory_id: "proc_investigate_pending_v1".to_string(),
            memory_type: MemoryType::Procedural,
            scope: MemoryScope::System,
            merchant_id: None,
            provider: None,
            incident_category: "provider_capture_pending".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Pending),
            title: "Procedure: Investigate Pending Payment (Read-Only)".to_string(),
            summary: "1) Check payment_attempts state and payment_reference. 2) Inspect webhook_inbox for missing or unapplied events. 3) Compare provider health latency/success rate. 4) Await authoritative confirmation; never assume failure or capture."
                .to_string(),
            resolution: Some("Perform read-only provider status inquiry or await webhook.".to_string()),
            source_refs: vec!["runbook:esapay/investigate-pending@v1".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: None,
        },
        MemoryRecord {
            memory_id: "proc_check_webhook_v1".to_string(),
            memory_type: MemoryType::Procedural,
            scope: MemoryScope::System,
            merchant_id: None,
            provider: None,
            incident_category: "webhook_delayed_or_missing".to_string(),
            lifecycle_state: None,
            title: "Procedure: Check Webhook Delivery & Deduplication (Read-Only)".to_string(),
            summary: "1) Count webhook_inbox entries for payment_id. 2) Check received_at delay relative to attempt created_at. 3) Inspect failed/unapplied error_message and duplicate provider_event_id."
                .to_string(),
            resolution: Some("Verify webhook signature configuration and provider webhook delivery logs.".to_string()),
            source_refs: vec!["runbook:esapay/check-webhook@v1".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: None,
        },
        MemoryRecord {
            memory_id: "proc_investigate_refund_v1".to_string(),
            memory_type: MemoryType::Procedural,
            scope: MemoryScope::System,
            merchant_id: None,
            provider: None,
            incident_category: "refund_reservation_pending".to_string(),
            lifecycle_state: Some(CanonicalPaymentState::Captured),
            title: "Procedure: Investigate Refund Reservation (Read-Only)".to_string(),
            summary: "1) Confirm captured_amount_minor. 2) Sum processed_refunds_minor and pending_reservations_minor. 3) Verify remaining_refundable_minor >= 0 and check payment_refunds status."
                .to_string(),
            resolution: Some("Inspect pending refund reservation status with provider.".to_string()),
            source_refs: vec!["runbook:esapay/investigate-refund@v1".to_string()],
            verification_status: VerificationStatus::Verified,
            version: 1,
            created_at: now,
            updated_at: now,
            expires_at: None,
        },
    ];

    let count = records.len();
    for rec in records {
        store.store_memory(rec).await?;
    }
    Ok(count)
}
