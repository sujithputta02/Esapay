use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::events::PaymentEventType;

/// Errors arising from payment state machine transitions and monetary amounts.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PaymentError {
    #[error("Invalid currency code: '{0}'. Must be a 3-letter alphabetic ISO-4217 code.")]
    InvalidCurrency(String),

    #[error("Amount {amount_minor} is below minimum required {minimum_required} minor units for currency {currency}.")]
    AmountBelowMinimum {
        amount_minor: u64,
        minimum_required: u64,
        currency: String,
    },

    #[error(
        "Amount {amount_minor} exceeds maximum allowable bounds of {maximum_allowed} minor units."
    )]
    AmountAboveMaximum {
        amount_minor: u64,
        maximum_allowed: u64,
    },

    #[error("Missing amount: either amount_minor or amount must be specified.")]
    MissingAmount,

    #[error("Conflicting amount inputs: 'amount_minor' is {amount_minor} but legacy amount field is {legacy_amount}.")]
    ConflictingAmountInputs {
        amount_minor: u64,
        legacy_amount: u64,
    },

    #[error("Prohibited state transition: cannot transition payment '{payment_id}' from {from:?} to {to:?} on event '{event}'. Reason: {reason}")]
    ProhibitedTransition {
        payment_id: String,
        from: PaymentState,
        to: PaymentState,
        event: String,
        reason: String,
    },
}

/// Validated ISO-4217 Currency representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Currency(String);

impl Currency {
    pub const INR: &'static str = "INR";
    pub const USD: &'static str = "USD";
    pub const EUR: &'static str = "EUR";
    pub const GBP: &'static str = "GBP";
    pub const JPY: &'static str = "JPY";
    pub const SGD: &'static str = "SGD";
    pub const AED: &'static str = "AED";

    pub fn new(code: &str) -> Result<Self, PaymentError> {
        let clean = code.trim().to_uppercase();
        if clean.len() != 3 || !clean.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(PaymentError::InvalidCurrency(code.to_string()));
        }
        Ok(Currency(clean))
    }

    pub fn code(&self) -> &str {
        &self.0
    }

    /// Decimal scale (exponent) for minor units:
    /// JPY = 0 (1 JPY = 1 minor unit)
    /// Standard currencies (INR, USD, EUR, GBP, SGD, AED) = 2 (100 minor units per major unit)
    pub fn decimal_places(&self) -> u32 {
        match self.0.as_str() {
            "JPY" | "KRW" | "VND" | "CLP" => 0,
            "BHD" | "JOD" | "KWD" | "OMR" | "TND" => 3,
            _ => 2,
        }
    }

    /// Minimum transaction amount in minor units to protect against fractional-cent abuse and gateway rejections.
    pub fn minimum_amount_minor(&self) -> u64 {
        match self.0.as_str() {
            "INR" => 100, // 100 paise (₹1.00)
            "USD" => 50,  // 50 cents ($0.50)
            "EUR" => 50,  // 50 euro cents (€0.50)
            "GBP" => 30,  // 30 pence (£0.30)
            "JPY" => 50,  // 50 yen (¥50)
            _ => 50,      // standard default minimum
        }
    }
}

/// Canonical monetary amount representation with currency and minor unit precision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentAmount {
    /// Canonical minor units (cents, paise, etc.)
    pub amount_minor: u64,
    /// Validated 3-letter ISO-4217 currency code
    pub currency: String,
}

impl PaymentAmount {
    pub fn new(amount_minor: u64, currency_code: &str) -> Result<Self, PaymentError> {
        let currency = Currency::new(currency_code)?;
        let min_amount = currency.minimum_amount_minor();
        if amount_minor < min_amount {
            return Err(PaymentError::AmountBelowMinimum {
                amount_minor,
                minimum_required: min_amount,
                currency: currency.code().to_string(),
            });
        }
        if amount_minor > 100_000_000_000 {
            return Err(PaymentError::AmountAboveMaximum {
                amount_minor,
                maximum_allowed: 100_000_000_000,
            });
        }
        Ok(Self {
            amount_minor,
            currency: currency.code().to_string(),
        })
    }

    /// Resolves canonical amount from potentially ambiguous or legacy input fields.
    /// Rejects conflicting inputs where both amount_minor and legacy amount/amount_cents are provided with differing values.
    pub fn resolve_from_inputs(
        amount_minor_input: Option<u64>,
        legacy_amount_input: Option<u64>,
        currency_code: Option<&str>,
    ) -> Result<Self, PaymentError> {
        let currency = currency_code.unwrap_or("INR");
        match (amount_minor_input, legacy_amount_input) {
            (Some(minor), Some(legacy)) if minor != legacy => {
                Err(PaymentError::ConflictingAmountInputs {
                    amount_minor: minor,
                    legacy_amount: legacy,
                })
            }
            (Some(minor), _) => Self::new(minor, currency),
            (None, Some(legacy)) => Self::new(legacy, currency),
            (None, None) => Err(PaymentError::MissingAmount),
        }
    }
}

/// Payment lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaymentState {
    /// Order or transaction intent initialized
    Created,
    /// Authorized by issuing bank / gateway; funds reserved
    Authorized,
    /// Captured; funds settled authoritatively
    Captured,
    /// Payment attempt failed or declined
    Failed,
    /// Partially refunded (0 < refunded_minor < total_captured_minor)
    PartiallyRefunded,
    /// Fully refunded (refunded_minor >= total_captured_minor)
    Refunded,
    /// Out-of-order, anomalous, or conflicting transition requiring ledger reconciliation
    PendingReconciliation,
}

impl PaymentState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, PaymentState::Refunded)
    }

    pub fn is_captured(&self) -> bool {
        matches!(
            self,
            PaymentState::Captured | PaymentState::PartiallyRefunded | PaymentState::Refunded
        )
    }
}

/// Outcome of attempting a payment state transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionOutcome {
    /// State transitioned successfully
    Applied {
        previous_state: PaymentState,
        new_state: PaymentState,
        reason: String,
    },
    /// Event was duplicate or already reflected in state (idempotent no-op)
    DuplicateIgnored {
        current_state: PaymentState,
        reason: String,
    },
    /// Event arrived out of order (e.g. late failure after capture), safe no-op
    LateEventIgnored {
        current_state: PaymentState,
        reason: String,
    },
    /// Discrepancy detected (e.g. authoritative capture after failure); flagged for reconciliation
    ReconciliationRequired {
        previous_state: PaymentState,
        new_state: PaymentState,
        reason: String,
    },
}

/// Representation of a managed payment transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentEntity {
    pub payment_id: String,
    pub state: PaymentState,
    pub amount_minor: u64,
    pub currency: String,
    pub refunded_amount_minor: u64,
    pub events_seen: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PaymentEntity {
    pub fn new(payment_id: String, amount_minor: u64, currency: String) -> Self {
        let now = Utc::now();
        Self {
            payment_id,
            state: PaymentState::Created,
            amount_minor,
            currency,
            refunded_amount_minor: 0,
            events_seen: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Process a payment event and transition state safely.
    pub fn apply_event(
        &mut self,
        event_id: &str,
        event_type: &PaymentEventType,
        refund_amount_minor: Option<u64>,
    ) -> Result<TransitionOutcome, PaymentError> {
        // Record event seen for deduplication
        let is_duplicate_event = self.events_seen.iter().any(|id| id == event_id);
        if !is_duplicate_event {
            self.events_seen.push(event_id.to_string());
        }

        let previous_state = self.state;
        let now = Utc::now();

        match (self.state, event_type) {
            // ==================== CREATED ====================
            (PaymentState::Created, PaymentEventType::PaymentAuthorized) => {
                self.state = PaymentState::Authorized;
                self.updated_at = now;
                Ok(TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment successfully authorized".to_string(),
                })
            }
            (PaymentState::Created, PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid) => {
                // Authoritative captured event accepted even if earlier Authorized event was missed!
                self.state = PaymentState::Captured;
                self.updated_at = now;
                Ok(TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment captured directly from Created (prior authorization event omitted or out-of-order)".to_string(),
                })
            }
            (PaymentState::Created, PaymentEventType::PaymentFailed) => {
                self.state = PaymentState::Failed;
                self.updated_at = now;
                Ok(TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment attempt failed".to_string(),
                })
            }
            (PaymentState::Created, PaymentEventType::RefundCreated) => {
                self.state = PaymentState::PendingReconciliation;
                self.updated_at = now;
                Ok(TransitionOutcome::ReconciliationRequired {
                    previous_state,
                    new_state: self.state,
                    reason: "Refund received before payment capture; flagged for reconciliation".to_string(),
                })
            }

            // ==================== AUTHORIZED ====================
            (PaymentState::Authorized, PaymentEventType::PaymentAuthorized) => {
                Ok(TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already in Authorized state".to_string(),
                })
            }
            (PaymentState::Authorized, PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid) => {
                self.state = PaymentState::Captured;
                self.updated_at = now;
                Ok(TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment captured successfully".to_string(),
                })
            }
            (PaymentState::Authorized, PaymentEventType::PaymentFailed) => {
                self.state = PaymentState::Failed;
                self.updated_at = now;
                Ok(TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment authorization failed or voided".to_string(),
                })
            }
            (PaymentState::Authorized, PaymentEventType::RefundCreated) => {
                self.state = PaymentState::PendingReconciliation;
                self.updated_at = now;
                Ok(TransitionOutcome::ReconciliationRequired {
                    previous_state,
                    new_state: self.state,
                    reason: "Refund received before payment capture; flagged for reconciliation".to_string(),
                })
            }

            // ==================== CAPTURED ====================
            (PaymentState::Captured, PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid) => {
                Ok(TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already captured".to_string(),
                })
            }
            (PaymentState::Captured, PaymentEventType::PaymentAuthorized) => {
                Ok(TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Authoritative capture already confirmed; trailing authorization event ignored".to_string(),
                })
            }
            (PaymentState::Captured, PaymentEventType::PaymentFailed) => {
                // CRITICAL SAFETY INVARIANT: A delayed failure event must NEVER downgrade a confirmed capture!
                Ok(TransitionOutcome::LateEventIgnored {
                    current_state: self.state,
                    reason: "Delayed failure event cannot downgrade a confirmed capture".to_string(),
                })
            }
            (PaymentState::Captured, PaymentEventType::RefundCreated) => {
                let refund = refund_amount_minor.unwrap_or(self.amount_minor);
                self.refunded_amount_minor = self.refunded_amount_minor.saturating_add(refund);
                self.updated_at = now;

                if self.refunded_amount_minor >= self.amount_minor {
                    self.state = PaymentState::Refunded;
                    Ok(TransitionOutcome::Applied {
                        previous_state,
                        new_state: self.state,
                        reason: "Payment fully refunded".to_string(),
                    })
                } else {
                    self.state = PaymentState::PartiallyRefunded;
                    Ok(TransitionOutcome::Applied {
                        previous_state,
                        new_state: self.state,
                        reason: format!(
                            "Payment partially refunded ({} of {} minor units)",
                            self.refunded_amount_minor, self.amount_minor
                        ),
                    })
                }
            }

            // ==================== PARTIALLY REFUNDED ====================
            (
                PaymentState::PartiallyRefunded,
                PaymentEventType::PaymentCaptured
                | PaymentEventType::OrderPaid
                | PaymentEventType::PaymentAuthorized,
            ) => {
                Ok(TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already captured and partially refunded".to_string(),
                })
            }
            (PaymentState::PartiallyRefunded, PaymentEventType::PaymentFailed) => {
                Ok(TransitionOutcome::LateEventIgnored {
                    current_state: self.state,
                    reason: "Delayed failure event cannot downgrade a partially refunded capture".to_string(),
                })
            }
            (PaymentState::PartiallyRefunded, PaymentEventType::RefundCreated) => {
                let refund = refund_amount_minor.unwrap_or(self.amount_minor.saturating_sub(self.refunded_amount_minor));
                self.refunded_amount_minor = self.refunded_amount_minor.saturating_add(refund);
                self.updated_at = now;

                if self.refunded_amount_minor >= self.amount_minor {
                    self.state = PaymentState::Refunded;
                    Ok(TransitionOutcome::Applied {
                        previous_state,
                        new_state: self.state,
                        reason: "Payment fully refunded after additional refund".to_string(),
                    })
                } else {
                    Ok(TransitionOutcome::Applied {
                        previous_state,
                        new_state: self.state,
                        reason: format!(
                            "Additional partial refund applied (total: {} of {} minor units)",
                            self.refunded_amount_minor, self.amount_minor
                        ),
                    })
                }
            }

            // ==================== REFUNDED (TERMINAL) ====================
            (PaymentState::Refunded, PaymentEventType::RefundCreated) => {
                Ok(TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already fully refunded".to_string(),
                })
            }
            (PaymentState::Refunded, event) => {
                Err(PaymentError::ProhibitedTransition {
                    payment_id: self.payment_id.clone(),
                    from: PaymentState::Refunded,
                    to: match event {
                        PaymentEventType::PaymentAuthorized => PaymentState::Authorized,
                        PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid => PaymentState::Captured,
                        PaymentEventType::PaymentFailed => PaymentState::Failed,
                        _ => PaymentState::Refunded,
                    },
                    event: format!("{:?}", event),
                    reason: "Refunded is a terminal state; cannot transition to active states".to_string(),
                })
            }

            // ==================== FAILED ====================
            (PaymentState::Failed, PaymentEventType::PaymentFailed) => {
                Ok(TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already failed".to_string(),
                })
            }
            (PaymentState::Failed, PaymentEventType::PaymentAuthorized) => {
                Ok(TransitionOutcome::LateEventIgnored {
                    current_state: self.state,
                    reason: "Authorization received after payment was marked failed; ignored".to_string(),
                })
            }
            (PaymentState::Failed, PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid) => {
                // Authoritative captured event arriving after initial failure (e.g. delayed webhook / async bank settlement)
                self.state = PaymentState::PendingReconciliation;
                self.updated_at = now;
                Ok(TransitionOutcome::ReconciliationRequired {
                    previous_state,
                    new_state: self.state,
                    reason: "Authoritative provider capture received after previous failure; marked for ledger reconciliation".to_string(),
                })
            }
            (PaymentState::Failed, PaymentEventType::RefundCreated) => {
                Err(PaymentError::ProhibitedTransition {
                    payment_id: self.payment_id.clone(),
                    from: PaymentState::Failed,
                    to: PaymentState::Refunded,
                    event: "refund.created".to_string(),
                    reason: "Cannot refund a payment that failed without being captured".to_string(),
                })
            }

            // ==================== PENDING RECONCILIATION ====================
            (PaymentState::PendingReconciliation, _) => {
                Ok(TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment is pending reconciliation; further automated state mutations suspended".to_string(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_standard_lifecycle() {
        let mut payment = PaymentEntity::new("pay_001".into(), 5000, "INR".into());
        assert_eq!(payment.state, PaymentState::Created);

        // Created -> Authorized
        let res = payment
            .apply_event("evt_auth_1", &PaymentEventType::PaymentAuthorized, None)
            .unwrap();
        assert!(matches!(res, TransitionOutcome::Applied { .. }));
        assert_eq!(payment.state, PaymentState::Authorized);

        // Authorized -> Captured
        let res = payment
            .apply_event("evt_cap_1", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        assert!(matches!(res, TransitionOutcome::Applied { .. }));
        assert_eq!(payment.state, PaymentState::Captured);

        // Captured -> PartiallyRefunded
        let res = payment
            .apply_event("evt_ref_1", &PaymentEventType::RefundCreated, Some(2000))
            .unwrap();
        assert!(matches!(res, TransitionOutcome::Applied { .. }));
        assert_eq!(payment.state, PaymentState::PartiallyRefunded);
        assert_eq!(payment.refunded_amount_minor, 2000);

        // PartiallyRefunded -> Refunded
        let res = payment
            .apply_event("evt_ref_2", &PaymentEventType::RefundCreated, Some(3000))
            .unwrap();
        assert!(matches!(res, TransitionOutcome::Applied { .. }));
        assert_eq!(payment.state, PaymentState::Refunded);
        assert_eq!(payment.refunded_amount_minor, 5000);
        assert!(payment.state.is_terminal());
    }

    #[test]
    fn test_missing_earlier_event_captured_directly_from_created() {
        let mut payment = PaymentEntity::new("pay_direct_capture".into(), 10000, "INR".into());
        assert_eq!(payment.state, PaymentState::Created);

        // Missing earlier Authorized event: Valid authoritative Captured event must NOT be rejected
        let res = payment
            .apply_event("evt_cap_direct", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        assert!(matches!(res, TransitionOutcome::Applied { .. }));
        assert_eq!(payment.state, PaymentState::Captured);
    }

    #[test]
    fn test_late_failure_event_never_downgrades_confirmed_capture() {
        let mut payment = PaymentEntity::new("pay_captured".into(), 7500, "USD".into());
        payment
            .apply_event("evt_cap", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        assert_eq!(payment.state, PaymentState::Captured);

        // Delayed failure event arrives after capture
        let res = payment
            .apply_event("evt_fail_late", &PaymentEventType::PaymentFailed, None)
            .unwrap();
        assert!(matches!(res, TransitionOutcome::LateEventIgnored { .. }));
        // State remains Captured!
        assert_eq!(payment.state, PaymentState::Captured);
    }

    #[test]
    fn test_late_failure_event_never_downgrades_partial_refund() {
        let mut payment = PaymentEntity::new("pay_part_ref".into(), 10000, "EUR".into());
        payment
            .apply_event("evt_cap", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        payment
            .apply_event("evt_ref", &PaymentEventType::RefundCreated, Some(4000))
            .unwrap();
        assert_eq!(payment.state, PaymentState::PartiallyRefunded);

        // Delayed failure arrives
        let res = payment
            .apply_event("evt_fail_late", &PaymentEventType::PaymentFailed, None)
            .unwrap();
        assert!(matches!(res, TransitionOutcome::LateEventIgnored { .. }));
        assert_eq!(payment.state, PaymentState::PartiallyRefunded);
    }

    #[test]
    fn test_authoritative_capture_after_failure_transitions_to_pending_reconciliation() {
        let mut payment = PaymentEntity::new("pay_recon".into(), 5000, "INR".into());
        payment
            .apply_event("evt_fail", &PaymentEventType::PaymentFailed, None)
            .unwrap();
        assert_eq!(payment.state, PaymentState::Failed);

        // Authoritative captured event arrives after failure was recorded
        let res = payment
            .apply_event("evt_cap_late", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        assert!(matches!(
            res,
            TransitionOutcome::ReconciliationRequired { .. }
        ));
        assert_eq!(payment.state, PaymentState::PendingReconciliation);
    }

    #[test]
    fn test_duplicate_events_are_idempotent() {
        let mut payment = PaymentEntity::new("pay_dup".into(), 5000, "INR".into());
        payment
            .apply_event("evt_auth", &PaymentEventType::PaymentAuthorized, None)
            .unwrap();

        let res1 = payment
            .apply_event("evt_auth", &PaymentEventType::PaymentAuthorized, None)
            .unwrap();
        assert!(matches!(res1, TransitionOutcome::DuplicateIgnored { .. }));
        assert_eq!(payment.state, PaymentState::Authorized);

        payment
            .apply_event("evt_cap", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        let res2 = payment
            .apply_event("evt_cap", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        assert!(matches!(res2, TransitionOutcome::DuplicateIgnored { .. }));
        assert_eq!(payment.state, PaymentState::Captured);
    }

    #[test]
    fn test_prohibited_transitions_from_refunded() {
        let mut payment = PaymentEntity::new("pay_terminal".into(), 5000, "INR".into());
        payment
            .apply_event("evt_cap", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        payment
            .apply_event("evt_ref", &PaymentEventType::RefundCreated, Some(5000))
            .unwrap();
        assert_eq!(payment.state, PaymentState::Refunded);

        // Attempting to authorize or capture a fully refunded payment is prohibited
        let err_auth = payment.apply_event(
            "evt_illegal_auth",
            &PaymentEventType::PaymentAuthorized,
            None,
        );
        assert!(matches!(
            err_auth,
            Err(PaymentError::ProhibitedTransition { .. })
        ));

        let err_cap =
            payment.apply_event("evt_illegal_cap", &PaymentEventType::PaymentCaptured, None);
        assert!(matches!(
            err_cap,
            Err(PaymentError::ProhibitedTransition { .. })
        ));
    }

    #[test]
    fn test_currency_validation_and_decimals() {
        assert!(Currency::new("inr").is_ok());
        assert_eq!(Currency::new("INR").unwrap().decimal_places(), 2);
        assert_eq!(Currency::new("JPY").unwrap().decimal_places(), 0);
        assert_eq!(Currency::new("BHD").unwrap().decimal_places(), 3);

        assert!(matches!(
            Currency::new("US"),
            Err(PaymentError::InvalidCurrency(_))
        ));
        assert!(matches!(
            Currency::new("US1"),
            Err(PaymentError::InvalidCurrency(_))
        ));
        assert!(matches!(
            Currency::new("TOOLONG"),
            Err(PaymentError::InvalidCurrency(_))
        ));
    }

    #[test]
    fn test_currency_minimum_amounts() {
        // INR minimum is 100 paise (₹1)
        assert!(PaymentAmount::new(99, "INR").is_err());
        assert!(PaymentAmount::new(100, "INR").is_ok());

        // USD minimum is 50 cents ($0.50)
        assert!(PaymentAmount::new(49, "USD").is_err());
        assert!(PaymentAmount::new(50, "USD").is_ok());

        // JPY minimum is 50 yen
        assert!(PaymentAmount::new(49, "JPY").is_err());
        assert!(PaymentAmount::new(50, "JPY").is_ok());
    }

    #[test]
    fn test_conflicting_amount_inputs_rejected() {
        // Conflicting inputs: minor = 5000, legacy = 6000 -> REJECT
        let res = PaymentAmount::resolve_from_inputs(Some(5000), Some(6000), Some("INR"));
        assert!(matches!(
            res,
            Err(PaymentError::ConflictingAmountInputs {
                amount_minor: 5000,
                legacy_amount: 6000
            })
        ));

        // Matching inputs: minor = 5000, legacy = 5000 -> ACCEPT
        let res = PaymentAmount::resolve_from_inputs(Some(5000), Some(5000), Some("INR")).unwrap();
        assert_eq!(res.amount_minor, 5000);

        // Only minor input provided -> ACCEPT
        let res = PaymentAmount::resolve_from_inputs(Some(5000), None, Some("INR")).unwrap();
        assert_eq!(res.amount_minor, 5000);

        // Only legacy input provided -> ACCEPT (wire compatibility)
        let res = PaymentAmount::resolve_from_inputs(None, Some(5000), Some("INR")).unwrap();
        assert_eq!(res.amount_minor, 5000);

        // Neither provided -> REJECT
        let res = PaymentAmount::resolve_from_inputs(None, None, Some("INR"));
        assert!(matches!(res, Err(PaymentError::MissingAmount)));
    }

    #[test]
    fn test_serde_wire_compatibility() {
        // Verify JSON serialization and deserialization
        let payment = PaymentEntity::new("pay_json".into(), 2500, "USD".into());
        let json = serde_json::to_string(&payment).unwrap();
        assert!(json.contains("\"amount_minor\":2500"));
        assert!(json.contains("\"currency\":\"USD\""));
        assert!(json.contains("\"state\":\"CREATED\""));

        let deserialized: PaymentEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.amount_minor, 2500);
        assert_eq!(deserialized.currency, "USD");
        assert_eq!(deserialized.state, PaymentState::Created);
    }
}
