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

    #[error("Unsupported combination: gateway '{provider}' does not support currency '{currency}' with payment method '{method}'.")]
    UnsupportedProviderCurrencyMethod {
        provider: String,
        currency: String,
        method: String,
    },
}

/// Universally valid ISO-4217 Currency Metadata.
/// Completely independent of provider- or payment-method-specific policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrencyMetadata {
    pub code: &'static str,
    pub numeric_code: u16,
    pub decimal_places: u32,
    pub name: &'static str,
    pub minor_unit_name: &'static str,
}

/// Authoritative ISO-4217 currency registry.
/// Defines supported ISO-4217 currencies, numeric codes, decimal scale (exponent), and unit names.
pub const SUPPORTED_CURRENCY_REGISTRY: &[CurrencyMetadata] = &[
    CurrencyMetadata {
        code: "INR",
        numeric_code: 356,
        decimal_places: 2,
        name: "Indian Rupee",
        minor_unit_name: "paise",
    },
    CurrencyMetadata {
        code: "USD",
        numeric_code: 840,
        decimal_places: 2,
        name: "United States Dollar",
        minor_unit_name: "cents",
    },
    CurrencyMetadata {
        code: "EUR",
        numeric_code: 978,
        decimal_places: 2,
        name: "Euro",
        minor_unit_name: "euro cents",
    },
    CurrencyMetadata {
        code: "GBP",
        numeric_code: 826,
        decimal_places: 2,
        name: "British Pound Sterling",
        minor_unit_name: "pence",
    },
    CurrencyMetadata {
        code: "JPY",
        numeric_code: 392,
        decimal_places: 0,
        name: "Japanese Yen",
        minor_unit_name: "yen",
    },
    CurrencyMetadata {
        code: "SGD",
        numeric_code: 702,
        decimal_places: 2,
        name: "Singapore Dollar",
        minor_unit_name: "cents",
    },
    CurrencyMetadata {
        code: "AED",
        numeric_code: 784,
        decimal_places: 2,
        name: "United Arab Emirates Dirham",
        minor_unit_name: "fils",
    },
    CurrencyMetadata {
        code: "CAD",
        numeric_code: 124,
        decimal_places: 2,
        name: "Canadian Dollar",
        minor_unit_name: "cents",
    },
    CurrencyMetadata {
        code: "AUD",
        numeric_code: 36,
        decimal_places: 2,
        name: "Australian Dollar",
        minor_unit_name: "cents",
    },
    CurrencyMetadata {
        code: "CHF",
        numeric_code: 756,
        decimal_places: 2,
        name: "Swiss Franc",
        minor_unit_name: "centimes",
    },
    CurrencyMetadata {
        code: "HKD",
        numeric_code: 344,
        decimal_places: 2,
        name: "Hong Kong Dollar",
        minor_unit_name: "cents",
    },
];

/// Policy rule for provider- and payment-method-specific minimum transaction thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMinimumPolicy {
    pub provider: &'static str,
    pub currency: &'static str,
    pub payment_method: &'static str,
    pub minimum_amount_minor: u64,
    pub source: &'static str,
    pub policy_justification: &'static str,
    pub regional_settlement_assumption: &'static str,
}

/// Documented minimum transaction policies by provider and payment method.
/// Derived strictly from official payment gateway API specifications and scheme rules.
/// Unverified gateways or generic guesses are strictly excluded to fail closed.
pub const PROVIDER_MINIMUM_POLICIES: &[ProviderMinimumPolicy] = &[
    ProviderMinimumPolicy {
        provider: "razorpay",
        currency: "INR",
        payment_method: "all",
        minimum_amount_minor: 100, // 100 paise (₹1.00)
        source: "Razorpay Orders API Docs (https://razorpay.com/docs/api/orders/#create-an-order)",
        policy_justification: "Razorpay Orders API enforces a mandatory minimum order value of 100 paise (₹1.00) across all domestic payment methods.",
        regional_settlement_assumption: "Indian domestic entity with standard INR merchant account settlement.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "USD",
        payment_method: "card",
        minimum_amount_minor: 50, // 50 cents ($0.50)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires a minimum charge of $0.50 USD to cover interchange fixed costs and per-transaction fees.",
        regional_settlement_assumption: "US or international Stripe account settling in USD.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "EUR",
        payment_method: "card",
        minimum_amount_minor: 50, // 50 euro cents (€0.50)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires a minimum charge of €0.50 EUR to cover card network processing minimums.",
        regional_settlement_assumption: "Eurozone or international Stripe account settling in EUR via SEPA rails.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "GBP",
        payment_method: "card",
        minimum_amount_minor: 30, // 30 pence (£0.30)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires a minimum charge of £0.30 GBP to offset domestic UK debit/credit clearing fees.",
        regional_settlement_assumption: "UK Stripe account settling in GBP via domestic UK clearing.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "JPY",
        payment_method: "card",
        minimum_amount_minor: 50, // 50 yen (¥50)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires a minimum charge of ¥50 JPY for zero-decimal Japanese yen transactions.",
        regional_settlement_assumption: "Japan Stripe account settling in JPY.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "SGD",
        payment_method: "card",
        minimum_amount_minor: 50, // 50 cents (S$0.50)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires a minimum charge of S$0.50 SGD to cover regional gateway clearing costs.",
        regional_settlement_assumption: "Singapore Stripe account settling in SGD.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "AED",
        payment_method: "card",
        minimum_amount_minor: 200, // 200 fils (2.00 AED)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires a minimum charge of 2.00 AED to ensure processing costs do not exceed total ticket value.",
        regional_settlement_assumption: "UAE Stripe account settling in AED.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "CAD",
        payment_method: "card",
        minimum_amount_minor: 50, // 50 cents (C$0.50)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires C$0.50 CAD minimum charge to cover interchange and Interac processing thresholds.",
        regional_settlement_assumption: "Canada Stripe account settling in CAD.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "AUD",
        payment_method: "card",
        minimum_amount_minor: 50, // 50 cents (A$0.50)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires A$0.50 AUD minimum charge to satisfy Australian debit scheme rules.",
        regional_settlement_assumption: "Australia Stripe account settling in AUD.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "CHF",
        payment_method: "card",
        minimum_amount_minor: 50, // 50 centimes (0.50 CHF)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires 0.50 CHF minimum charge to cover Swiss clearing fees.",
        regional_settlement_assumption: "Switzerland Stripe account settling in CHF.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "HKD",
        payment_method: "card",
        minimum_amount_minor: 400, // 400 cents (HK$4.00)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires HK$4.00 HKD minimum charge due to higher fixed processing fee floor in Hong Kong.",
        regional_settlement_assumption: "Hong Kong Stripe account settling in HKD.",
    },
    ProviderMinimumPolicy {
        provider: "stripe",
        currency: "INR",
        payment_method: "card",
        minimum_amount_minor: 50, // 50 paise (₹0.50)
        source: "Stripe Currency Guide (https://docs.stripe.com/currencies#minimum-and-maximum-charge-amounts)",
        policy_justification: "Stripe requires a minimum charge of ₹0.50 INR (50 paise) for card and export transaction processing.",
        regional_settlement_assumption: "India Stripe account settling in INR or cross-border export transaction with INR charge currency.",
    },
    ProviderMinimumPolicy {
        provider: "phonepe",
        currency: "INR",
        payment_method: "all",
        minimum_amount_minor: 100, // 100 paise (₹1.00)
        source: "PhonePe PG Developer Documentation (https://developer.phonepe.com/v1/reference/standard-pay-api)",
        policy_justification: "PhonePe Standard Pay API mandates integer `amount` parameter with minimum value 100 paise (₹1.00).",
        regional_settlement_assumption: "Indian domestic merchant account with standard INR settlement via PhonePe PG.",
    },
    ProviderMinimumPolicy {
        provider: "cashfree",
        currency: "INR",
        payment_method: "all",
        minimum_amount_minor: 100, // 100 paise (₹1.00)
        source: "Cashfree Payment Gateway API Documentation (https://docs.cashfree.com/reference/createorder)",
        policy_justification: "Cashfree Orders API mandates `order_amount` >= 1.00 INR (100 paise) for order creation.",
        regional_settlement_assumption: "Indian domestic merchant account with standard INR settlement via Cashfree PG.",
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

    pub fn numeric_code(&self) -> u16 {
        self.metadata().numeric_code
    }

    /// Decimal scale (exponent) for minor units:
    /// JPY = 0 (1 JPY = 1 minor unit)
    /// Standard currencies = 2 (100 minor units per major unit)
    pub fn decimal_places(&self) -> u32 {
        self.metadata().decimal_places
    }

    /// Name of the minor currency sub-unit (paise, cents, pence, yen, etc.)
    pub fn minor_unit_name(&self) -> &'static str {
        self.metadata().minor_unit_name
    }

    /// Default baseline minimum transaction amount in minor units for currency.
    /// Only currency-wide universal policies (payment_method: "all") apply by default.
    /// Payment-method-specific requirements (e.g. Stripe card floors) are NOT inferred
    /// as generic currency minimums.
    pub fn default_minimum_amount_minor(&self) -> u64 {
        PROVIDER_MINIMUM_POLICIES
            .iter()
            .find(|p| p.currency == self.0 && p.payment_method == "all")
            .map(|p| p.minimum_amount_minor)
            .unwrap_or(1)
    }

    /// Backwards-compatible alias for default minimum minor units.
    pub fn minimum_amount_minor(&self) -> u64 {
        self.default_minimum_amount_minor()
    }

    /// Look up minimum transaction amount for a specific payment provider and method.
    /// Evaluates provider- and payment-method-specific requirements without conflating them
    /// with generic currency minimums. Returns `PaymentError::UnsupportedProviderCurrencyMethod`
    /// if the combination is not supported by any configured provider policy.
    pub fn minimum_amount_for_provider(
        &self,
        provider: &str,
        method: Option<&str>,
    ) -> Result<u64, PaymentError> {
        let p_clean = provider.trim().to_lowercase();
        let m_clean = method.unwrap_or("").trim().to_lowercase();

        // 1. Caller-supplied wildcard "all" is strictly rejected to prevent policy bypasses
        if m_clean == "all" {
            return Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: provider.to_string(),
                currency: self.0.clone(),
                method: "all".to_string(),
            });
        }

        // 2. Explicit concrete method requested
        if !m_clean.is_empty() {
            // Normalize recognized payment method alias
            let canonical_method = match m_clean.as_str() {
                "card" | "credit" | "debit" => "card",
                "upi" => "upi",
                "netbanking" | "nb" => "netbanking",
                "wallet" => "wallet",
                _ => {
                    // Unsupported method string (e.g. "crypto", "direct_debit", etc.)
                    return Err(PaymentError::UnsupportedProviderCurrencyMethod {
                        provider: provider.to_string(),
                        currency: self.0.clone(),
                        method: m_clean,
                    });
                }
            };

            // Provider method capability check: verify gateway genuinely supports the requested method
            let provider_supports_method = match p_clean.as_str() {
                "stripe" => canonical_method == "card",
                "razorpay" | "phonepe" | "cashfree" => {
                    canonical_method == "card"
                        || canonical_method == "upi"
                        || canonical_method == "netbanking"
                        || canonical_method == "wallet"
                }
                _ => false,
            };

            if !provider_supports_method {
                return Err(PaymentError::UnsupportedProviderCurrencyMethod {
                    provider: provider.to_string(),
                    currency: self.0.clone(),
                    method: m_clean,
                });
            }

            // Match against documented provider policies
            if let Some(p) = PROVIDER_MINIMUM_POLICIES.iter().find(|p| {
                p.currency == self.0
                    && p.provider.eq_ignore_ascii_case(&p_clean)
                    && (p.payment_method.eq_ignore_ascii_case(canonical_method)
                        || p.payment_method == "all")
            }) {
                return Ok(p.minimum_amount_minor);
            }

            return Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: provider.to_string(),
                currency: self.0.clone(),
                method: m_clean,
            });
        }

        // 3. Unspecified method: ONLY matches providers with universal currency policies ("all").
        // Method-specific policies (such as Stripe's card-only policy) MUST NOT match an unspecified method!
        if let Some(p) = PROVIDER_MINIMUM_POLICIES.iter().find(|p| {
            p.currency == self.0
                && p.provider.eq_ignore_ascii_case(&p_clean)
                && p.payment_method == "all"
        }) {
            return Ok(p.minimum_amount_minor);
        }

        // 4. Unsupported combination or missing method-specific policy -> fail closed
        Err(PaymentError::UnsupportedProviderCurrencyMethod {
            provider: provider.to_string(),
            currency: self.0.clone(),
            method: "unspecified".to_string(),
        })
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
        let min_amount = currency.default_minimum_amount_minor();
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

    /// Validates amount against a specific payment provider and payment method policy.
    pub fn new_with_provider_policy(
        amount_minor: u64,
        currency_code: &str,
        provider: &str,
        payment_method: Option<&str>,
    ) -> Result<Self, PaymentError> {
        let currency = Currency::new(currency_code)?;
        let min_amount = currency.minimum_amount_for_provider(provider, payment_method)?;
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

    /// Resolves canonical amount from potentially ambiguous or legacy input fields,
    /// and enforces provider- and method-specific monetary validation on the selected gateway.
    pub fn resolve_from_all_inputs_with_provider(
        amount_minor_input: Option<u64>,
        legacy_amount_input: Option<u64>,
        amount_cents_input: Option<u64>,
        currency_code: Option<&str>,
        provider: &str,
        payment_method: Option<&str>,
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
            (Some(minor), _) => {
                Self::new_with_provider_policy(minor, currency, provider, payment_method)
            }
            (None, Some(legacy)) => {
                Self::new_with_provider_policy(legacy, currency, provider, payment_method)
            }
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

    /// Converts canonical integer minor units to major units f64 according to the currency decimal scale.
    /// E.g. 50000 INR (scale 2) -> 500.00
    /// E.g. 50000 USD (scale 2) -> 500.00
    /// E.g. 5000 JPY (scale 0)  -> 5000.00
    pub fn to_major_units_f64(&self) -> f64 {
        let decimals = self.currency_obj().decimal_places();
        let factor = 10_u64.pow(decimals) as f64;
        self.amount_minor as f64 / factor
    }

    /// Formats the amount in major units with appropriate decimal precision and currency code.
    /// E.g. "500.00 INR", "50.00 USD", "5000 JPY"
    pub fn format_major(&self) -> String {
        let decimals = self.currency_obj().decimal_places();
        if decimals == 0 {
            format!("{} {}", self.amount_minor, self.currency)
        } else {
            format!(
                "{:.precision$} {}",
                self.to_major_units_f64(),
                self.currency,
                precision = decimals as usize
            )
        }
    }

    /// Helper to get validated Currency object
    pub fn currency_obj(&self) -> Currency {
        Currency::new(&self.currency).expect("PaymentAmount invariant: currency must be valid")
    }

    /// Minor unit name (e.g. "paise" for INR, "cents" for USD, "yen" for JPY)
    pub fn minor_unit_name(&self) -> &'static str {
        self.currency_obj().metadata().minor_unit_name
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
        // INR universal baseline minimum is 100 paise (₹1) across all methods
        assert!(PaymentAmount::new(99, "INR").is_err());
        assert!(PaymentAmount::new(100, "INR").is_ok());

        // Generic currency baseline floor is 1 minor unit (rejects 0-amount transactions)
        // Payment-method-specific requirements are NOT inferred as generic currency minimums!
        assert!(PaymentAmount::new(0, "USD").is_err());
        assert!(PaymentAmount::new(1, "USD").is_ok());
        assert!(PaymentAmount::new(49, "USD").is_ok());

        // Provider policy enforcement (new_with_provider_policy) enforces method-specific floors:
        // Stripe card USD minimum is 50 cents ($0.50)
        assert!(
            PaymentAmount::new_with_provider_policy(49, "USD", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(50, "USD", "stripe", Some("card")).is_ok());

        // Stripe card JPY minimum is 50 yen (¥50)
        assert!(
            PaymentAmount::new_with_provider_policy(49, "JPY", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(50, "JPY", "stripe", Some("card")).is_ok());

        // Stripe card GBP minimum is 30 pence (£0.30)
        assert!(
            PaymentAmount::new_with_provider_policy(29, "GBP", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(30, "GBP", "stripe", Some("card")).is_ok());

        // Stripe card AED minimum is 200 fils (2.00 AED)
        assert!(
            PaymentAmount::new_with_provider_policy(199, "AED", "stripe", Some("card")).is_err()
        );
        assert!(
            PaymentAmount::new_with_provider_policy(200, "AED", "stripe", Some("card")).is_ok()
        );

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

    #[test]
    fn test_currency_metadata_separation_and_provider_policies() {
        // Universally valid ISO-4217 metadata
        let inr_meta = Currency::new("INR").unwrap().metadata();
        assert_eq!(inr_meta.numeric_code, 356);
        assert_eq!(inr_meta.decimal_places, 2);
        assert_eq!(inr_meta.minor_unit_name, "paise");

        let jpy_meta = Currency::new("JPY").unwrap().metadata();
        assert_eq!(jpy_meta.numeric_code, 392);
        assert_eq!(jpy_meta.decimal_places, 0);
        assert_eq!(jpy_meta.minor_unit_name, "yen");

        let usd_meta = Currency::new("USD").unwrap().metadata();
        assert_eq!(usd_meta.numeric_code, 840);
        assert_eq!(usd_meta.decimal_places, 2);
        assert_eq!(usd_meta.minor_unit_name, "cents");

        // Provider policies separate with verified sources
        let rzp_min = Currency::new("INR")
            .unwrap()
            .minimum_amount_for_provider("razorpay", Some("upi"))
            .unwrap();
        assert_eq!(rzp_min, 100);

        let stripe_usd_min = Currency::new("USD")
            .unwrap()
            .minimum_amount_for_provider("stripe", Some("card"))
            .unwrap();
        assert_eq!(stripe_usd_min, 50);

        let stripe_hkd_min = Currency::new("HKD")
            .unwrap()
            .minimum_amount_for_provider("stripe", Some("card"))
            .unwrap();
        assert_eq!(stripe_hkd_min, 400);

        let stripe_inr_min = Currency::new("INR")
            .unwrap()
            .minimum_amount_for_provider("stripe", Some("card"))
            .unwrap();
        assert_eq!(stripe_inr_min, 50); // 50 paise = ₹0.50 per official Stripe doc

        let stripe_aed_min = Currency::new("AED")
            .unwrap()
            .minimum_amount_for_provider("stripe", Some("card"))
            .unwrap();
        assert_eq!(stripe_aed_min, 200); // 200 fils = 2.00 AED preserved

        let phonepe_min = Currency::new("INR")
            .unwrap()
            .minimum_amount_for_provider("phonepe", Some("upi"))
            .unwrap();
        assert_eq!(phonepe_min, 100);

        // Unsupported combinations must be explicitly rejected with UnsupportedProviderCurrencyMethod
        let unsupp_rzp_usd = Currency::new("USD")
            .unwrap()
            .minimum_amount_for_provider("razorpay", Some("card"));
        assert_eq!(
            unsupp_rzp_usd,
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "razorpay".to_string(),
                currency: "USD".to_string(),
                method: "card".to_string(),
            })
        );

        let unsupp_stripe_upi = Currency::new("INR")
            .unwrap()
            .minimum_amount_for_provider("stripe", Some("upi"));
        assert_eq!(
            unsupp_stripe_upi,
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "stripe".to_string(),
                currency: "INR".to_string(),
                method: "upi".to_string(),
            })
        );

        let unsupp_gw = Currency::new("INR")
            .unwrap()
            .minimum_amount_for_provider("unregistered_gateway", None);
        assert_eq!(
            unsupp_gw,
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "unregistered_gateway".to_string(),
                currency: "INR".to_string(),
                method: "unspecified".to_string(),
            })
        );

        // Method-specific policy bypass regression tests:
        // 1. Stripe INR requires explicit "card"
        let inr = Currency::new("INR").unwrap();
        assert_eq!(
            inr.minimum_amount_for_provider("stripe", Some("card")),
            Ok(50)
        );
        // Omitted method on Stripe INR must NOT match card policy
        assert_eq!(
            inr.minimum_amount_for_provider("stripe", None),
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "stripe".to_string(),
                currency: "INR".to_string(),
                method: "unspecified".to_string(),
            })
        );
        // Caller-supplied "all" wildcard must NOT bypass validation
        assert_eq!(
            inr.minimum_amount_for_provider("stripe", Some("all")),
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "stripe".to_string(),
                currency: "INR".to_string(),
                method: "all".to_string(),
            })
        );
        // Explicitly unsupported method on Stripe (crypto, upi)
        assert_eq!(
            inr.minimum_amount_for_provider("stripe", Some("crypto")),
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "stripe".to_string(),
                currency: "INR".to_string(),
                method: "crypto".to_string(),
            })
        );

        // 2. Razorpay INR universal policy
        assert_eq!(inr.minimum_amount_for_provider("razorpay", None), Ok(100));
        assert_eq!(
            inr.minimum_amount_for_provider("razorpay", Some("upi")),
            Ok(100)
        );
        assert_eq!(
            inr.minimum_amount_for_provider("razorpay", Some("card")),
            Ok(100)
        );
        // Caller wildcard "all" rejected on Razorpay too
        assert_eq!(
            inr.minimum_amount_for_provider("razorpay", Some("all")),
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "razorpay".to_string(),
                currency: "INR".to_string(),
                method: "all".to_string(),
            })
        );
        // Explicit unsupported method on Razorpay
        assert_eq!(
            inr.minimum_amount_for_provider("razorpay", Some("crypto")),
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "razorpay".to_string(),
                currency: "INR".to_string(),
                method: "crypto".to_string(),
            })
        );

        // 3. PhonePe and Cashfree INR universal policies
        assert_eq!(inr.minimum_amount_for_provider("phonepe", None), Ok(100));
        assert_eq!(inr.minimum_amount_for_provider("cashfree", None), Ok(100));

        // Every provider policy must have a non-empty source citation and policy justification
        for policy in PROVIDER_MINIMUM_POLICIES {
            assert!(
                !policy.source.is_empty(),
                "Policy for {} {} must have a source citation",
                policy.provider,
                policy.currency
            );
            assert!(
                !policy.policy_justification.is_empty(),
                "Policy for {} {} must have a policy justification",
                policy.provider,
                policy.currency
            );
            assert!(
                !policy.regional_settlement_assumption.is_empty(),
                "Policy for {} {} must have a regional settlement assumption",
                policy.provider,
                policy.currency
            );
        }
    }

    #[test]
    fn test_monetary_semantics_major_vs_minor_across_currencies() {
        // An identical integer minor value (5000) does NOT mean identical monetary value or major units!
        let inr = PaymentAmount::new(5000, "INR").unwrap();
        let usd = PaymentAmount::new(5000, "USD").unwrap();
        let jpy = PaymentAmount::new(5000, "JPY").unwrap();

        // 1. Major unit values: 50.00 for INR/USD (scale 2), but 5000.00 for JPY (scale 0)
        assert_eq!(inr.to_major_units_f64(), 50.0);
        assert_eq!(usd.to_major_units_f64(), 50.0);
        assert_eq!(jpy.to_major_units_f64(), 5000.0);

        // 2. Formatted major representations
        assert_eq!(inr.format_major(), "50.00 INR");
        assert_eq!(usd.format_major(), "50.00 USD");
        assert_eq!(jpy.format_major(), "5000 JPY");

        // 3. Sub-unit terminology
        assert_eq!(inr.minor_unit_name(), "paise");
        assert_eq!(usd.minor_unit_name(), "cents");
        assert_eq!(jpy.minor_unit_name(), "yen");

        // 4. Minimum values
        let inr_min = PaymentAmount::new(100, "INR").unwrap();
        assert_eq!(inr_min.to_major_units_f64(), 1.0);
        assert_eq!(inr_min.format_major(), "1.00 INR");

        let jpy_min = PaymentAmount::new(50, "JPY").unwrap();
        assert_eq!(jpy_min.to_major_units_f64(), 50.0);
        assert_eq!(jpy_min.format_major(), "50 JPY");
    }

    #[test]
    fn test_provider_specific_policy_enforcement() {
        // 1. Razorpay INR (100 paise domestic minimum)
        assert!(PaymentAmount::new_with_provider_policy(99, "INR", "razorpay", None).is_err());
        assert!(PaymentAmount::new_with_provider_policy(100, "INR", "razorpay", None).is_ok());
        assert!(PaymentAmount::new_with_provider_policy(101, "INR", "razorpay", None).is_ok());

        // 2. Stripe USD card (50 cents)
        assert!(
            PaymentAmount::new_with_provider_policy(49, "USD", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(50, "USD", "stripe", Some("card")).is_ok());
        assert!(PaymentAmount::new_with_provider_policy(51, "USD", "stripe", Some("card")).is_ok());

        // 3. Stripe EUR card (€0.50 = 50 euro cents)
        assert!(
            PaymentAmount::new_with_provider_policy(49, "EUR", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(50, "EUR", "stripe", Some("card")).is_ok());
        assert!(PaymentAmount::new_with_provider_policy(51, "EUR", "stripe", Some("card")).is_ok());

        // 4. Stripe GBP card (£0.30 = 30 pence)
        assert!(
            PaymentAmount::new_with_provider_policy(29, "GBP", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(30, "GBP", "stripe", Some("card")).is_ok());
        assert!(PaymentAmount::new_with_provider_policy(31, "GBP", "stripe", Some("card")).is_ok());

        // 5. Stripe JPY card (¥50 = 50 yen, zero decimal scale)
        assert!(
            PaymentAmount::new_with_provider_policy(49, "JPY", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(50, "JPY", "stripe", Some("card")).is_ok());
        assert!(PaymentAmount::new_with_provider_policy(51, "JPY", "stripe", Some("card")).is_ok());

        // 6. Stripe SGD card (S$0.50 = 50 cents)
        assert!(
            PaymentAmount::new_with_provider_policy(49, "SGD", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(50, "SGD", "stripe", Some("card")).is_ok());
        assert!(PaymentAmount::new_with_provider_policy(51, "SGD", "stripe", Some("card")).is_ok());

        // 7. Stripe AED card (2.00 AED = 200 fils, official Stripe Currency Guide)
        assert!(
            PaymentAmount::new_with_provider_policy(199, "AED", "stripe", Some("card")).is_err()
        );
        assert!(
            PaymentAmount::new_with_provider_policy(200, "AED", "stripe", Some("card")).is_ok()
        );
        assert!(
            PaymentAmount::new_with_provider_policy(201, "AED", "stripe", Some("card")).is_ok()
        );

        // 8. Stripe CAD card (C$0.50 = 50 cents)
        assert!(
            PaymentAmount::new_with_provider_policy(49, "CAD", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(50, "CAD", "stripe", Some("card")).is_ok());
        assert!(PaymentAmount::new_with_provider_policy(51, "CAD", "stripe", Some("card")).is_ok());

        // 9. Stripe AUD card (A$0.50 = 50 cents)
        assert!(
            PaymentAmount::new_with_provider_policy(49, "AUD", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(50, "AUD", "stripe", Some("card")).is_ok());
        assert!(PaymentAmount::new_with_provider_policy(51, "AUD", "stripe", Some("card")).is_ok());

        // 10. Stripe CHF card (0.50 CHF = 50 centimes)
        assert!(
            PaymentAmount::new_with_provider_policy(49, "CHF", "stripe", Some("card")).is_err()
        );
        assert!(PaymentAmount::new_with_provider_policy(50, "CHF", "stripe", Some("card")).is_ok());
        assert!(PaymentAmount::new_with_provider_policy(51, "CHF", "stripe", Some("card")).is_ok());

        // 11. Stripe HKD card (HK$4.00 = 400 cents)
        assert!(
            PaymentAmount::new_with_provider_policy(399, "HKD", "stripe", Some("card")).is_err()
        );
        assert!(
            PaymentAmount::new_with_provider_policy(400, "HKD", "stripe", Some("card")).is_ok()
        );
        assert!(
            PaymentAmount::new_with_provider_policy(401, "HKD", "stripe", Some("card")).is_ok()
        );

        // Invariant: Unsupported provider/currency/method combinations are explicitly rejected
        // Non-card payment methods on Stripe or unregistered providers do NOT silently fall back
        assert_eq!(
            PaymentAmount::new_with_provider_policy(10, "USD", "stripe", Some("bank_transfer")),
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "stripe".to_string(),
                currency: "USD".to_string(),
                method: "bank_transfer".to_string(),
            })
        );
        assert_eq!(
            PaymentAmount::new_with_provider_policy(10, "USD", "custom_gateway", None),
            Err(PaymentError::UnsupportedProviderCurrencyMethod {
                provider: "custom_gateway".to_string(),
                currency: "USD".to_string(),
                method: "unspecified".to_string(),
            })
        );

        // resolve_from_all_inputs_with_provider wire compatibility
        let resolved = PaymentAmount::resolve_from_all_inputs_with_provider(
            None,
            Some(200),
            None,
            Some("AED"),
            "stripe",
            Some("card"),
        )
        .expect("Legacy amount input must resolve against provider policy");
        assert_eq!(resolved.amount_minor, 200);
        assert_eq!(resolved.currency, "AED");

        let conflict = PaymentAmount::resolve_from_all_inputs_with_provider(
            Some(200),
            Some(199),
            None,
            Some("AED"),
            "stripe",
            Some("card"),
        );
        assert!(matches!(
            conflict,
            Err(PaymentError::ConflictingAmountInputs { .. })
        ));
    }
}
