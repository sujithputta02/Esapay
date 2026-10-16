use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::events::PaymentEventType;

/// Errors arising from payment state machine transitions and monetary amounts.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PaymentError {
    #[error("Unsupported or invalid currency code: '{0}'. Supported currencies are: INR, USD, EUR, GBP, JPY, SGD, AED, CAD, AUD, CHF, HKD.")]
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

    #[error("Missing amount: either amount_minor, amount, or amount_cents must be specified.")]
    MissingAmount,

    #[error("Conflicting amount inputs: 'amount_minor' is {amount_minor} but legacy input field is {legacy_amount}.")]
    ConflictingAmountInputs {
        amount_minor: u64,
        legacy_amount: u64,
    },

    #[error("Refund amount cannot be zero for payment '{payment_id}'.")]
    ZeroRefundAmount { payment_id: String },

    #[error("Refund amount {refund_amount_minor} exceeds remaining captured balance {remaining_balance} for payment '{payment_id}'.")]
    RefundExceedsBalance {
        payment_id: String,
        refund_amount_minor: u64,
        remaining_balance: u64,
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

/// Metadata configuration for a supported currency in the currency registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrencyMetadata {
    pub code: &'static str,
    pub decimal_places: u32,
    pub minimum_amount_minor: u64,
    pub name: &'static str,
}

/// Authoritative supported currency registry.
/// Defines supported ISO-4217 currencies, decimal scale, and provider-specific transaction minimums.
pub const SUPPORTED_CURRENCY_REGISTRY: &[CurrencyMetadata] = &[
    CurrencyMetadata {
        code: "INR",
        decimal_places: 2,
        minimum_amount_minor: 100, // 100 paise (₹1.00) - Razorpay minimum
        name: "Indian Rupee",
    },
    CurrencyMetadata {
        code: "USD",
        decimal_places: 2,
        minimum_amount_minor: 50, // 50 cents ($0.50) - Card network minimum
        name: "United States Dollar",
    },
    CurrencyMetadata {
        code: "EUR",
        decimal_places: 2,
        minimum_amount_minor: 50, // 50 euro cents (€0.50)
        name: "Euro",
    },
    CurrencyMetadata {
        code: "GBP",
        decimal_places: 2,
        minimum_amount_minor: 30, // 30 pence (£0.30)
        name: "British Pound Sterling",
    },
    CurrencyMetadata {
        code: "JPY",
        decimal_places: 0,
        minimum_amount_minor: 50, // 50 yen (¥50) - Zero decimal currency
        name: "Japanese Yen",
    },
    CurrencyMetadata {
        code: "SGD",
        decimal_places: 2,
        minimum_amount_minor: 50, // 50 cents (S$0.50)
        name: "Singapore Dollar",
    },
    CurrencyMetadata {
        code: "AED",
        decimal_places: 2,
        minimum_amount_minor: 100, // 100 fils (1.00 AED)
        name: "United Arab Emirates Dirham",
    },
    CurrencyMetadata {
        code: "CAD",
        decimal_places: 2,
        minimum_amount_minor: 50, // 50 cents (C$0.50)
        name: "Canadian Dollar",
    },
    CurrencyMetadata {
        code: "AUD",
        decimal_places: 2,
        minimum_amount_minor: 50, // 50 cents (A$0.50)
        name: "Australian Dollar",
    },
    CurrencyMetadata {
        code: "CHF",
        decimal_places: 2,
        minimum_amount_minor: 50, // 50 centimes (0.50 CHF)
        name: "Swiss Franc",
    },
    CurrencyMetadata {
        code: "HKD",
        decimal_places: 2,
        minimum_amount_minor: 400, // 400 cents (HK$4.00)
        name: "Hong Kong Dollar",
    },
];

/// Validated ISO-4217 Currency representation backed by the supported currency registry.
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
    pub const CAD: &'static str = "CAD";
    pub const AUD: &'static str = "AUD";
    pub const CHF: &'static str = "CHF";
    pub const HKD: &'static str = "HKD";

    pub fn new(code: &str) -> Result<Self, PaymentError> {
        let clean = code.trim().to_uppercase();
        if clean.is_empty() {
            return Err(PaymentError::InvalidCurrency(
                "Currency cannot be empty".to_string(),
            ));
        }

        let matched = SUPPORTED_CURRENCY_REGISTRY.iter().find(|c| c.code == clean);

        match matched {
            Some(meta) => Ok(Currency(meta.code.to_string())),
            None => Err(PaymentError::InvalidCurrency(format!(
                "Unsupported currency code: '{}'. Supported currencies are: {}",
                code,
                SUPPORTED_CURRENCY_REGISTRY
                    .iter()
                    .map(|c| c.code)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    pub fn code(&self) -> &str {
        &self.0
    }

    pub fn metadata(&self) -> CurrencyMetadata {
        *SUPPORTED_CURRENCY_REGISTRY
            .iter()
            .find(|c| c.code == self.0)
            .expect("Currency invariant: code must exist in registry")
    }

    /// Decimal scale (exponent) for minor units:
    /// JPY = 0 (1 JPY = 1 minor unit)
    /// Standard currencies = 2 (100 minor units per major unit)
    pub fn decimal_places(&self) -> u32 {
        self.metadata().decimal_places
    }

    /// Minimum transaction amount in minor units to protect against fractional abuse and gateway rejections.
    pub fn minimum_amount_minor(&self) -> u64 {
        self.metadata().minimum_amount_minor
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
        Ok(Self {
            amount_minor,
            currency: currency.code().to_string(),
        })
    }

    /// Resolves canonical amount from potentially ambiguous or legacy input fields:
    /// - `amount_minor_input`: modern canonical minor units (e.g. cents/paise)
    /// - `legacy_amount_input`: legacy integer amount field
    /// - `amount_cents_input`: legacy explicit cents field
    ///
    /// Preserves legacy field semantics, resolving minor units while rejecting conflicting inputs.
    pub fn resolve_from_all_inputs(
        amount_minor_input: Option<u64>,
        legacy_amount_input: Option<u64>,
        amount_cents_input: Option<u64>,
        currency_code: Option<&str>,
    ) -> Result<Self, PaymentError> {
        let currency = currency_code.unwrap_or(Currency::INR);

        // Check conflicts between legacy amount and amount_cents
        if let (Some(a), Some(c)) = (legacy_amount_input, amount_cents_input) {
            if a != c {
                return Err(PaymentError::ConflictingAmountInputs {
                    amount_minor: c,
                    legacy_amount: a,
                });
            }
        }

        let resolved_legacy = legacy_amount_input.or(amount_cents_input);

        match (amount_minor_input, resolved_legacy) {
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

    /// Backwards-compatible convenience wrapper for 2 inputs
    pub fn resolve_from_inputs(
        amount_minor_input: Option<u64>,
        legacy_amount_input: Option<u64>,
        currency_code: Option<&str>,
    ) -> Result<Self, PaymentError> {
        Self::resolve_from_all_inputs(amount_minor_input, legacy_amount_input, None, currency_code)
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
    ///
    /// Guarantees:
    /// 1. Idempotency: Repeated event IDs return `TransitionOutcome::DuplicateIgnored` immediately
    ///    without any secondary transitions or side-effects (e.g. repeated refunds).
    /// 2. Fault Isolation: Rejected transitions NEVER record the event ID into `events_seen`.
    /// 3. Refund Bounding: Refunds of zero or refunds exceeding remaining captured balance are strictly rejected.
    pub fn apply_event(
        &mut self,
        event_id: &str,
        event_type: &PaymentEventType,
        refund_amount_minor: Option<u64>,
    ) -> Result<TransitionOutcome, PaymentError> {
        let clean_event_id = event_id.trim();
        if clean_event_id.is_empty() {
            return Err(PaymentError::ProhibitedTransition {
                payment_id: self.payment_id.clone(),
                from: self.state,
                to: self.state,
                event: format!("{:?}", event_type),
                reason: "Event ID cannot be empty".to_string(),
            });
        }

        // 1. CRITICAL IDEMPOTENCY CHECK:
        // A repeated event ID MUST NOT cause a second state transition or any side effect!
        if self.events_seen.iter().any(|id| id == clean_event_id) {
            return Ok(TransitionOutcome::DuplicateIgnored {
                current_state: self.state,
                reason: format!("Event '{}' has already been processed", clean_event_id),
            });
        }

        let previous_state = self.state;
        let now = Utc::now();

        // 2. Perform state transition and side-effect calculation
        let outcome = match (self.state, event_type) {
            // ==================== CREATED ====================
            (PaymentState::Created, PaymentEventType::PaymentAuthorized) => {
                self.state = PaymentState::Authorized;
                self.updated_at = now;
                TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment successfully authorized".to_string(),
                }
            }
            (
                PaymentState::Created,
                PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid,
            ) => {
                // Authoritative captured event accepted even if earlier Authorized event was missed!
                self.state = PaymentState::Captured;
                self.updated_at = now;
                TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment captured directly from Created (prior authorization event omitted or out-of-order)".to_string(),
                }
            }
            (PaymentState::Created, PaymentEventType::PaymentFailed) => {
                self.state = PaymentState::Failed;
                self.updated_at = now;
                TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment attempt failed".to_string(),
                }
            }
            (PaymentState::Created, PaymentEventType::RefundCreated) => {
                self.state = PaymentState::PendingReconciliation;
                self.updated_at = now;
                TransitionOutcome::ReconciliationRequired {
                    previous_state,
                    new_state: self.state,
                    reason: "Refund received before payment capture; flagged for reconciliation".to_string(),
                }
            }

            // ==================== AUTHORIZED ====================
            (PaymentState::Authorized, PaymentEventType::PaymentAuthorized) => {
                TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already in Authorized state".to_string(),
                }
            }
            (
                PaymentState::Authorized,
                PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid,
            ) => {
                self.state = PaymentState::Captured;
                self.updated_at = now;
                TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment captured successfully".to_string(),
                }
            }
            (PaymentState::Authorized, PaymentEventType::PaymentFailed) => {
                self.state = PaymentState::Failed;
                self.updated_at = now;
                TransitionOutcome::Applied {
                    previous_state,
                    new_state: self.state,
                    reason: "Payment authorization failed or voided".to_string(),
                }
            }
            (PaymentState::Authorized, PaymentEventType::RefundCreated) => {
                self.state = PaymentState::PendingReconciliation;
                self.updated_at = now;
                TransitionOutcome::ReconciliationRequired {
                    previous_state,
                    new_state: self.state,
                    reason: "Refund received before payment capture; flagged for reconciliation".to_string(),
                }
            }

            // ==================== CAPTURED ====================
            (
                PaymentState::Captured,
                PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid,
            ) => {
                TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already captured".to_string(),
                }
            }
            (PaymentState::Captured, PaymentEventType::PaymentAuthorized) => {
                TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Authoritative capture already confirmed; trailing authorization event ignored".to_string(),
                }
            }
            (PaymentState::Captured, PaymentEventType::PaymentFailed) => {
                // CRITICAL SAFETY INVARIANT: A delayed failure event must NEVER downgrade a confirmed capture!
                TransitionOutcome::LateEventIgnored {
                    current_state: self.state,
                    reason: "Delayed failure event cannot downgrade a confirmed capture".to_string(),
                }
            }
            (PaymentState::Captured, PaymentEventType::RefundCreated) => {
                let remaining_captured =
                    self.amount_minor.saturating_sub(self.refunded_amount_minor);
                let refund = match refund_amount_minor {
                    Some(0) => {
                        return Err(PaymentError::ZeroRefundAmount {
                            payment_id: self.payment_id.clone(),
                        });
                    }
                    Some(amt) if amt > remaining_captured => {
                        return Err(PaymentError::RefundExceedsBalance {
                            payment_id: self.payment_id.clone(),
                            refund_amount_minor: amt,
                            remaining_balance: remaining_captured,
                        });
                    }
                    Some(amt) => amt,
                    None => {
                        if remaining_captured == 0 {
                            return Err(PaymentError::ZeroRefundAmount {
                                payment_id: self.payment_id.clone(),
                            });
                        }
                        remaining_captured
                    }
                };

                self.refunded_amount_minor = self.refunded_amount_minor.saturating_add(refund);
                self.updated_at = now;

                if self.refunded_amount_minor >= self.amount_minor {
                    self.state = PaymentState::Refunded;
                    TransitionOutcome::Applied {
                        previous_state,
                        new_state: self.state,
                        reason: "Payment fully refunded".to_string(),
                    }
                } else {
                    self.state = PaymentState::PartiallyRefunded;
                    TransitionOutcome::Applied {
                        previous_state,
                        new_state: self.state,
                        reason: format!(
                            "Payment partially refunded ({} of {} minor units)",
                            self.refunded_amount_minor, self.amount_minor
                        ),
                    }
                }
            }

            // ==================== PARTIALLY REFUNDED ====================
            (
                PaymentState::PartiallyRefunded,
                PaymentEventType::PaymentCaptured
                | PaymentEventType::OrderPaid
                | PaymentEventType::PaymentAuthorized,
            ) => {
                TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already captured and partially refunded".to_string(),
                }
            }
            (PaymentState::PartiallyRefunded, PaymentEventType::PaymentFailed) => {
                TransitionOutcome::LateEventIgnored {
                    current_state: self.state,
                    reason: "Delayed failure event cannot downgrade a partially refunded capture"
                        .to_string(),
                }
            }
            (PaymentState::PartiallyRefunded, PaymentEventType::RefundCreated) => {
                let remaining_captured =
                    self.amount_minor.saturating_sub(self.refunded_amount_minor);
                let refund = match refund_amount_minor {
                    Some(0) => {
                        return Err(PaymentError::ZeroRefundAmount {
                            payment_id: self.payment_id.clone(),
                        });
                    }
                    Some(amt) if amt > remaining_captured => {
                        return Err(PaymentError::RefundExceedsBalance {
                            payment_id: self.payment_id.clone(),
                            refund_amount_minor: amt,
                            remaining_balance: remaining_captured,
                        });
                    }
                    Some(amt) => amt,
                    None => {
                        if remaining_captured == 0 {
                            return Err(PaymentError::ZeroRefundAmount {
                                payment_id: self.payment_id.clone(),
                            });
                        }
                        remaining_captured
                    }
                };

                self.refunded_amount_minor = self.refunded_amount_minor.saturating_add(refund);
                self.updated_at = now;

                if self.refunded_amount_minor >= self.amount_minor {
                    self.state = PaymentState::Refunded;
                    TransitionOutcome::Applied {
                        previous_state,
                        new_state: self.state,
                        reason: "Payment fully refunded after additional refund".to_string(),
                    }
                } else {
                    TransitionOutcome::Applied {
                        previous_state,
                        new_state: self.state,
                        reason: format!(
                            "Additional partial refund applied (total: {} of {} minor units)",
                            self.refunded_amount_minor, self.amount_minor
                        ),
                    }
                }
            }

            // ==================== REFUNDED (TERMINAL) ====================
            (PaymentState::Refunded, PaymentEventType::RefundCreated) => {
                TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already fully refunded".to_string(),
                }
            }
            (PaymentState::Refunded, event) => {
                return Err(PaymentError::ProhibitedTransition {
                    payment_id: self.payment_id.clone(),
                    from: PaymentState::Refunded,
                    to: match event {
                        PaymentEventType::PaymentAuthorized => PaymentState::Authorized,
                        PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid => {
                            PaymentState::Captured
                        }
                        PaymentEventType::PaymentFailed => PaymentState::Failed,
                        _ => PaymentState::Refunded,
                    },
                    event: format!("{:?}", event),
                    reason: "Refunded is a terminal state; cannot transition to active states"
                        .to_string(),
                });
            }

            // ==================== FAILED ====================
            (PaymentState::Failed, PaymentEventType::PaymentFailed) => {
                TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment already failed".to_string(),
                }
            }
            (PaymentState::Failed, PaymentEventType::PaymentAuthorized) => {
                TransitionOutcome::LateEventIgnored {
                    current_state: self.state,
                    reason: "Authorization received after payment was marked failed; ignored"
                        .to_string(),
                }
            }
            (
                PaymentState::Failed,
                PaymentEventType::PaymentCaptured | PaymentEventType::OrderPaid,
            ) => {
                // Authoritative captured event arriving after initial failure (e.g. delayed webhook / async bank settlement)
                self.state = PaymentState::PendingReconciliation;
                self.updated_at = now;
                TransitionOutcome::ReconciliationRequired {
                    previous_state,
                    new_state: self.state,
                    reason: "Authoritative provider capture received after previous failure; marked for ledger reconciliation".to_string(),
                }
            }
            (PaymentState::Failed, PaymentEventType::RefundCreated) => {
                return Err(PaymentError::ProhibitedTransition {
                    payment_id: self.payment_id.clone(),
                    from: PaymentState::Failed,
                    to: PaymentState::Refunded,
                    event: "refund.created".to_string(),
                    reason: "Cannot refund a payment that failed without being captured"
                        .to_string(),
                });
            }

            // ==================== PENDING RECONCILIATION ====================
            (PaymentState::PendingReconciliation, _) => {
                TransitionOutcome::DuplicateIgnored {
                    current_state: self.state,
                    reason: "Payment is pending reconciliation; further automated state mutations suspended".to_string(),
                }
            }
        };

        // 3. ONLY record event_id upon successful transition execution!
        // Rejected transitions do not pollute the processed event log.
        self.events_seen.push(clean_event_id.to_string());

        Ok(outcome)
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
    fn test_duplicate_refund_event_does_not_mutate_refunded_amount() {
        let mut payment = PaymentEntity::new("pay_dup_ref".into(), 10000, "INR".into());
        payment
            .apply_event("evt_cap", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        assert_eq!(payment.state, PaymentState::Captured);
        assert_eq!(payment.refunded_amount_minor, 0);

        // First refund event
        let res1 = payment
            .apply_event("evt_ref_same", &PaymentEventType::RefundCreated, Some(3000))
            .unwrap();
        assert!(matches!(res1, TransitionOutcome::Applied { .. }));
        assert_eq!(payment.state, PaymentState::PartiallyRefunded);
        assert_eq!(payment.refunded_amount_minor, 3000);

        // Duplicate of the exact same refund event: MUST be ignored immediately with zero side-effects!
        let res2 = payment
            .apply_event("evt_ref_same", &PaymentEventType::RefundCreated, Some(3000))
            .unwrap();
        assert!(matches!(res2, TransitionOutcome::DuplicateIgnored { .. }));
        assert_eq!(
            payment.refunded_amount_minor, 3000,
            "Duplicate refund event must NOT increase refunded amount"
        );
        assert_eq!(payment.state, PaymentState::PartiallyRefunded);
    }

    #[test]
    fn test_zero_refund_amount_rejected() {
        let mut payment = PaymentEntity::new("pay_zero_ref".into(), 5000, "INR".into());
        payment
            .apply_event("evt_cap", &PaymentEventType::PaymentCaptured, None)
            .unwrap();

        // Refund amount of zero MUST be rejected
        let res = payment.apply_event("evt_ref_zero", &PaymentEventType::RefundCreated, Some(0));
        assert_eq!(
            res,
            Err(PaymentError::ZeroRefundAmount {
                payment_id: "pay_zero_ref".into()
            })
        );
        assert_eq!(payment.refunded_amount_minor, 0);
        assert_eq!(payment.state, PaymentState::Captured);
        assert!(
            !payment.events_seen.contains(&"evt_ref_zero".to_string()),
            "Rejected event must NOT be stored in events_seen"
        );
    }

    #[test]
    fn test_refund_exceeding_remaining_captured_balance_rejected() {
        let mut payment = PaymentEntity::new("pay_over_ref".into(), 5000, "INR".into());
        payment
            .apply_event("evt_cap", &PaymentEventType::PaymentCaptured, None)
            .unwrap();

        // Attempting to refund more than total captured balance (6000 > 5000)
        let res = payment.apply_event(
            "evt_ref_excess",
            &PaymentEventType::RefundCreated,
            Some(6000),
        );
        assert_eq!(
            res,
            Err(PaymentError::RefundExceedsBalance {
                payment_id: "pay_over_ref".into(),
                refund_amount_minor: 6000,
                remaining_balance: 5000
            })
        );
        assert_eq!(payment.refunded_amount_minor, 0);
        assert_eq!(payment.state, PaymentState::Captured);
        assert!(!payment.events_seen.contains(&"evt_ref_excess".to_string()));

        // Valid partial refund of 3000
        payment
            .apply_event(
                "evt_ref_valid",
                &PaymentEventType::RefundCreated,
                Some(3000),
            )
            .unwrap();
        assert_eq!(payment.refunded_amount_minor, 3000);
        assert_eq!(payment.state, PaymentState::PartiallyRefunded);

        // Attempting second refund exceeding remaining balance (remaining = 2000, requested = 2500)
        let res2 = payment.apply_event(
            "evt_ref_excess2",
            &PaymentEventType::RefundCreated,
            Some(2500),
        );
        assert_eq!(
            res2,
            Err(PaymentError::RefundExceedsBalance {
                payment_id: "pay_over_ref".into(),
                refund_amount_minor: 2500,
                remaining_balance: 2000
            })
        );
        assert_eq!(payment.refunded_amount_minor, 3000);
        assert_eq!(payment.state, PaymentState::PartiallyRefunded);
    }

    #[test]
    fn test_repeated_captures_are_idempotent() {
        let mut payment = PaymentEntity::new("pay_dup_cap".into(), 5000, "INR".into());
        payment
            .apply_event("evt_auth", &PaymentEventType::PaymentAuthorized, None)
            .unwrap();

        let res1 = payment
            .apply_event("evt_cap_1", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        assert!(matches!(res1, TransitionOutcome::Applied { .. }));
        assert_eq!(payment.state, PaymentState::Captured);

        // Same event ID capture
        let res2 = payment
            .apply_event("evt_cap_1", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        assert!(matches!(res2, TransitionOutcome::DuplicateIgnored { .. }));

        // Different event ID repeated capture
        let res3 = payment
            .apply_event("evt_cap_2", &PaymentEventType::PaymentCaptured, None)
            .unwrap();
        assert!(matches!(res3, TransitionOutcome::DuplicateIgnored { .. }));
        assert_eq!(payment.state, PaymentState::Captured);
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
    fn test_supported_currency_registry_validation() {
        // Valid registered currencies
        assert!(Currency::new("INR").is_ok());
        assert!(Currency::new("inr").is_ok());
        assert!(Currency::new("USD").is_ok());
        assert!(Currency::new("EUR").is_ok());
        assert!(Currency::new("GBP").is_ok());
        assert!(Currency::new("JPY").is_ok());
        assert!(Currency::new("SGD").is_ok());
        assert!(Currency::new("AED").is_ok());
        assert!(Currency::new("CAD").is_ok());
        assert!(Currency::new("AUD").is_ok());
        assert!(Currency::new("CHF").is_ok());
        assert!(Currency::new("HKD").is_ok());

        // Arbitrary shape-only 3-letter codes are REJECTED
        assert!(matches!(
            Currency::new("ZZZ"),
            Err(PaymentError::InvalidCurrency(_))
        ));
        assert!(matches!(
            Currency::new("FOO"),
            Err(PaymentError::InvalidCurrency(_))
        ));
        assert!(matches!(
            Currency::new("BAR"),
            Err(PaymentError::InvalidCurrency(_))
        ));
        assert!(matches!(
            Currency::new("ABC"),
            Err(PaymentError::InvalidCurrency(_))
        ));

        // Precision checks
        assert_eq!(Currency::new("JPY").unwrap().decimal_places(), 0);
        assert_eq!(Currency::new("INR").unwrap().decimal_places(), 2);
        assert_eq!(Currency::new("USD").unwrap().decimal_places(), 2);
        assert_eq!(Currency::new("EUR").unwrap().decimal_places(), 2);
    }

    #[test]
    fn test_currency_provider_minimum_amounts() {
        // INR minimum is 100 paise (₹1)
        assert!(PaymentAmount::new(99, "INR").is_err());
        assert!(PaymentAmount::new(100, "INR").is_ok());

        // USD minimum is 50 cents ($0.50)
        assert!(PaymentAmount::new(49, "USD").is_err());
        assert!(PaymentAmount::new(50, "USD").is_ok());

        // JPY minimum is 50 yen
        assert!(PaymentAmount::new(49, "JPY").is_err());
        assert!(PaymentAmount::new(50, "JPY").is_ok());

        // GBP minimum is 30 pence
        assert!(PaymentAmount::new(29, "GBP").is_err());
        assert!(PaymentAmount::new(30, "GBP").is_ok());

        // Large justified amounts are permitted without arbitrary artificial bounds
        assert!(PaymentAmount::new(100_000_000_000, "USD").is_ok());
        assert!(PaymentAmount::new(500_000_000_000, "INR").is_ok());
    }

    #[test]
    fn test_legacy_api_field_compatibility() {
        // 1. Only amount_cents passed (legacy cents field) -> preserves value as minor units
        let res1 =
            PaymentAmount::resolve_from_all_inputs(None, None, Some(4500), Some("USD")).unwrap();
        assert_eq!(res1.amount_minor, 4500);

        // 2. Only amount passed (legacy integer field) -> preserves value as minor units
        let res2 =
            PaymentAmount::resolve_from_all_inputs(None, Some(4500), None, Some("USD")).unwrap();
        assert_eq!(res2.amount_minor, 4500);

        // 3. Only amount_minor passed -> modern field resolved
        let res3 =
            PaymentAmount::resolve_from_all_inputs(Some(4500), None, None, Some("USD")).unwrap();
        assert_eq!(res3.amount_minor, 4500);

        // 4. Both amount and amount_cents match -> succeeds
        let res4 =
            PaymentAmount::resolve_from_all_inputs(None, Some(4500), Some(4500), Some("USD"))
                .unwrap();
        assert_eq!(res4.amount_minor, 4500);

        // 5. Conflicting inputs between amount and amount_cents -> rejected
        let err1 =
            PaymentAmount::resolve_from_all_inputs(None, Some(4500), Some(5000), Some("USD"));
        assert!(matches!(
            err1,
            Err(PaymentError::ConflictingAmountInputs { .. })
        ));

        // 6. Conflicting inputs between amount_minor and legacy amount -> rejected
        let err2 =
            PaymentAmount::resolve_from_all_inputs(Some(4500), Some(5000), None, Some("USD"));
        assert!(matches!(
            err2,
            Err(PaymentError::ConflictingAmountInputs { .. })
        ));

        // 7. All three match -> succeeds
        let res5 =
            PaymentAmount::resolve_from_all_inputs(Some(4500), Some(4500), Some(4500), Some("USD"))
                .unwrap();
        assert_eq!(res5.amount_minor, 4500);
    }

    #[test]
    fn test_serde_wire_compatibility() {
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
