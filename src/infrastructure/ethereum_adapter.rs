use crate::contracts::{Groth16Proof, ZKRollupBridge};
use crate::domain::errors::DomainError;
use async_trait::async_trait;
use ethers::prelude::*;
use std::sync::Arc;

use crate::application::ports::BridgeReader;

#[async_trait]
pub trait BridgeClient: BridgeReader + Send + Sync {
    async fn commit_batch(
        &self,
        da_id: u8,
        batch_data: Bytes,
        da_meta: Bytes,
        new_root: [u8; 32],
        proof: Groth16Proof,
    ) -> Result<H256, DomainError>;

    async fn get_transaction_receipt(
        &self,
        hash: H256,
    ) -> Result<Option<TransactionReceipt>, DomainError>;
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
impl<M: Middleware + 'static> BridgeReader for RealBridgeClient<M> {
    async fn state_root(&self) -> Result<H256, DomainError> {
        let root = self
            .bridge
            .state_root()
            .call()
            .await
            .map_err(|e| DomainError::Da(format!("Failed to fetch state root: {}", e)))?;
        Ok(H256::from(root))
    }
}

#[cfg(not(tarpaulin_include))]
#[async_trait]
impl<M: Middleware + 'static> BridgeClient for RealBridgeClient<M> {
    async fn commit_batch(
        &self,
        da_id: u8,
        batch_data: Bytes,
        da_meta: Bytes,
        new_root: [u8; 32],
        proof: Groth16Proof,
    ) -> Result<H256, DomainError> {
        let call = self
            .bridge
            .commit_batch(da_id, batch_data, da_meta, new_root, proof);
        let pending = call
            .send()
            .await
            .map_err(|e| DomainError::Da(format!("Tx send failed: {}", e)))?;
        Ok(pending.tx_hash())
    }

    async fn get_transaction_receipt(
        &self,
        hash: H256,
    ) -> Result<Option<TransactionReceipt>, DomainError> {
        self.client
            .get_transaction_receipt(hash)
            .await
            .map_err(|e| DomainError::Da(format!("Provider error: {}", e)))
    }

    async fn get_block_number(&self) -> Result<U64, DomainError> {
        self.client
            .get_block_number()
            .await
            .map_err(|e| DomainError::Da(format!("Provider error: {}", e)))
    }
}
