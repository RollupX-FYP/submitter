use crate::domain::{
    batch::{Batch, BatchId},
    errors::DomainError,
};
use async_trait::async_trait;
use ethers::types::H256;
use serde::{Deserialize, Serialize};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait BridgeReader: Send + Sync {
    /// Fetches the current state root from the L1 ZKRollupBridge contract.
    async fn state_root(&self) -> Result<H256, DomainError>;
}

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
    /// Returns the DA ID required by the contract (0 = Calldata, 1 = Blob).
    fn da_id(&self) -> u8;

    /// Computes the commitment to be used as a Public Input.
    /// Calldata: keccak256(batch.data)
    /// Blob: batch.blob_versioned_hash
    fn compute_commitment(&self, batch: &Batch) -> Result<H256, DomainError>;

    /// Encodes the 'daMeta' bytes for the transaction.
    /// Calldata: empty bytes
    /// Blob: abi.encode(versioned_hash, blob_index)
    fn encode_da_meta(&self, batch: &Batch) -> Result<Vec<u8>, DomainError>;

    /// Broadcasts the transaction and returns the hash immediately.
    async fn submit(&self, batch: &Batch, proof: &str) -> Result<String, DomainError>;

    /// Checks if a transaction has been confirmed.
    async fn check_confirmation(&self, tx_hash: &str) -> Result<bool, DomainError>;
}
