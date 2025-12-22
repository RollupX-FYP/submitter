use crate::domain::{
    batch::{Batch, BatchId},
    errors::DomainError,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait Storage: Send + Sync {
    async fn save_batch(&self, batch: &Batch) -> Result<(), DomainError>;
    async fn get_batch(&self, id: BatchId) -> Result<Option<Batch>, DomainError>;
    async fn get_pending_batches(&self) -> Result<Vec<Batch>, DomainError>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProofResponse {
    pub proof: String, // Serialized proof
}

#[async_trait]
pub trait ProofProvider: Send + Sync {
    async fn get_proof(
        &self,
        batch_id: &BatchId,
        public_inputs: &[u8],
    ) -> Result<ProofResponse, DomainError>;
}

#[async_trait]
pub trait DaStrategy: Send + Sync {
    /// Broadcasts the transaction and returns the hash immediately.
    async fn submit(&self, batch: &Batch, proof: &str) -> Result<String, DomainError>;

    /// Checks if a transaction has been confirmed.
    async fn check_confirmation(&self, tx_hash: &str) -> Result<bool, DomainError>;
}
