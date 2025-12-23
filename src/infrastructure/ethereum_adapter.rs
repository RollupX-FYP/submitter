use crate::contracts::{Groth16Proof, ZKRollupBridge};
use crate::domain::errors::DomainError;
use async_trait::async_trait;
use ethers::prelude::*;
use std::sync::Arc;

#[async_trait]
pub trait BridgeClient: Send + Sync {
    async fn commit_batch_calldata(
        &self,
        batch_data: Bytes,
        new_root: [u8; 32],
        proof: Groth16Proof,
    ) -> Result<H256, DomainError>;

    async fn commit_batch_blob(
        &self,
        versioned_hash: [u8; 32],
        blob_index: u8,
        use_opcode: bool,
        new_root: [u8; 32],
        proof: Groth16Proof,
    ) -> Result<H256, DomainError>;

    async fn get_transaction_receipt(&self, hash: H256) -> Result<Option<TransactionReceipt>, DomainError>;
    async fn get_block_number(&self) -> Result<U64, DomainError>;
}

pub struct RealBridgeClient<M: Middleware> {
    bridge: ZKRollupBridge<M>,
    client: Arc<M>,
}

#[cfg(not(tarpaulin_include))]
impl<M: Middleware> RealBridgeClient<M> {
    pub fn new(bridge: ZKRollupBridge<M>) -> Self {
        let client = bridge.client();
        Self { bridge, client }
    }
}

#[cfg(not(tarpaulin_include))]
#[async_trait]
impl<M: Middleware + 'static> BridgeClient for RealBridgeClient<M> {
    async fn commit_batch_calldata(
        &self,
        batch_data: Bytes,
        new_root: [u8; 32],
        proof: Groth16Proof,
    ) -> Result<H256, DomainError> {
        let call = self.bridge.commit_batch_calldata(batch_data, new_root, proof);
        let pending = call.send().await.map_err(|e| DomainError::Da(format!("Tx send failed: {}", e)))?;
        Ok(pending.tx_hash())
    }

    async fn commit_batch_blob(
        &self,
        versioned_hash: [u8; 32],
        blob_index: u8,
        use_opcode: bool,
        new_root: [u8; 32],
        proof: Groth16Proof,
    ) -> Result<H256, DomainError> {
        let call = self.bridge.commit_batch_blob(versioned_hash, blob_index, use_opcode, new_root, proof);
        let pending = call.send().await.map_err(|e| DomainError::Da(format!("Tx send failed: {}", e)))?;
        Ok(pending.tx_hash())
    }

    async fn get_transaction_receipt(&self, hash: H256) -> Result<Option<TransactionReceipt>, DomainError> {
        self.client.get_transaction_receipt(hash).await.map_err(|e| DomainError::Da(format!("Provider error: {}", e)))
    }

    async fn get_block_number(&self) -> Result<U64, DomainError> {
        self.client.get_block_number().await.map_err(|e| DomainError::Da(format!("Provider error: {}", e)))
    }
}
