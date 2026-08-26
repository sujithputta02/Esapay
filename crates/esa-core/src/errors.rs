use thiserror::Error;

#[derive(Debug, Error)]
pub enum EsaError {
    #[error("State version mismatch: expected {expected}, got {actual}")]
    StateVersionMismatch { expected: u64, actual: u64 },

    #[error("Policy violation: {reason}")]
    PolicyViolation { reason: String },

    #[error("Action execution failed: {reason}")]
    ActionExecutionFailed { reason: String },

    #[error("Invalid action: {reason}")]
    InvalidAction { reason: String },

    #[error("Agent unavailable: {agent}")]
    AgentUnavailable { agent: String },

    #[error("Resource not found: {resource}")]
    ResourceNotFound { resource: String },

    #[error("Rate limit exceeded: {message}")]
    RateLimitExceeded { message: String },

    #[error("Token budget exceeded: {current}/{limit}")]
    TokenBudgetExceeded { current: usize, limit: usize },

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Redis error: {0}")]
    Redis(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type EsaResult<T> = Result<T, EsaError>;
