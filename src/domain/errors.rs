use thiserror::Error;

#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Prover error: {0}")]
    Prover(String),
    #[error("DA error: {0}")]
    Da(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Internal error: {0}")]
    Internal(String),
}
