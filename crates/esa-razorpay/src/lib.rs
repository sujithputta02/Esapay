//! Razorpay Test Mode payment adapter — webhook validation, rate limits, normalization.

pub mod adapter;
pub mod client;
pub mod config;
pub mod dedup;
pub mod rate_limit;
pub mod webhook;

pub use adapter::RazorpayAdapter;
pub use config::{RazorpayConfig, RazorpayConfigError};
pub use webhook::WebhookError;
pub use webhook::{payment_entity_to_event, verify_signature};
