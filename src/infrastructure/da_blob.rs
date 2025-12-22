use crate::application::ports::DaStrategy;
use crate::contracts::{Groth16Proof, ZKRollupBridge};
use crate::domain::{batch::Batch, errors::DomainError};
use anyhow::Context;
use async_trait::async_trait;
use ethers::prelude::*;
use std::sync::Arc;
use tracing::{info, warn};
use metrics::counter;

pub struct BlobStrategy<M: Middleware> {
    bridge: ZKRollupBridge<M>,
    client: Arc<M>,
    blob_versioned_hash: H256,
    blob_index: u8,
    use_opcode: bool,
}

impl<M: Middleware + 'static> BlobStrategy<M> {
    pub fn new(
        bridge: ZKRollupBridge<M>, 
        blob_versioned_hash: H256, 
        blob_index: u8, 
        use_opcode: bool
    ) -> Self {
        let client = bridge.client();
        Self { 
            bridge,
            client,
            blob_versioned_hash,
            blob_index,
            use_opcode,
        }
    }
}

#[async_trait]
impl<M: Middleware + 'static> DaStrategy for BlobStrategy<M> {
    async fn submit(&self, batch: &Batch, _proof: &str) -> Result<String, DomainError> {
        let proof = Groth16Proof {
            a: [U256::zero(), U256::zero()],
            b: [[U256::zero(), U256::zero()], [U256::zero(), U256::zero()]],
            c: [U256::zero(), U256::zero()],
        };

        let new_root: H256 = batch.new_root.parse()
            .map_err(|e| DomainError::Da(format!("Invalid new root: {}", e)))?;

        let bridge = self.bridge.clone();
        let call = bridge.commit_batch_blob(
            self.blob_versioned_hash.into(),
            self.blob_index,
            self.use_opcode,
            new_root.into(),
            proof,
        );
        
        // Just send, do not wait
        let pending = call.send().await
             .map_err(|e| DomainError::Da(format!("Tx send failed: {}", e)))?;

        let tx_hash = pending.tx_hash();
        info!("Blob batch broadcasted. tx={:?}", tx_hash);

        counter!("tx_submitted_total", "mode" => "blob").increment(1);
        
        Ok(format!("{:?}", tx_hash))
    }

    async fn check_confirmation(&self, tx_hash: &str) -> Result<bool, DomainError> {
        let hash: H256 = tx_hash.parse().map_err(|e| DomainError::Da(format!("Invalid hash: {}", e)))?;
        let receipt = self.client.get_transaction_receipt(hash).await
            .map_err(|e| DomainError::Da(format!("Provider error: {}", e)))?;

        if let Some(r) = receipt {
             // Check status (1 = success, 0 = failure)
             if let Some(status) = r.status {
                 if status.as_u64() == 1 {
                     // Check confirmations
                     let block_number = r.block_number.unwrap_or_default();
                     let current_block = self.client.get_block_number().await
                        .map_err(|e| DomainError::Da(format!("Provider error: {}", e)))?;
                     
                     let confs = current_block.as_u64().saturating_sub(block_number.as_u64());
                     
                     if confs >= 1 {
                         return Ok(true);
                     } else {
                         info!("Tx mined but waiting for confirmations (current: {})", confs);
                         return Ok(false);
                     }
                 } else {
                     warn!("Tx {} reverted!", tx_hash);
                     return Err(DomainError::Da("Transaction reverted on-chain".to_string()));
                 }
             }
             Ok(true)
        } else {
            Ok(false)
        }
    }
}
