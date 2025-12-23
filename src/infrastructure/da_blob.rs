use crate::application::ports::DaStrategy;
use crate::contracts::{Groth16Proof, ZKRollupBridge};
use crate::domain::{batch::Batch, errors::DomainError};
use async_trait::async_trait;
use ethers::prelude::*;
use metrics::counter;
use std::sync::Arc;
use tracing::{info, warn};

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
        use_opcode: bool,
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

        let new_root: H256 = batch
            .new_root
            .parse()
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
        let pending = call
            .send()
            .await
            .map_err(|e| DomainError::Da(format!("Tx send failed: {}", e)))?;

        let tx_hash = pending.tx_hash();
        info!("Blob batch broadcasted. tx={:?}", tx_hash);

        counter!("tx_submitted_total", "mode" => "blob").increment(1);

        Ok(format!("{:?}", tx_hash))
    }

    async fn check_confirmation(&self, tx_hash: &str) -> Result<bool, DomainError> {
        let hash: H256 = tx_hash
            .parse()
            .map_err(|e| DomainError::Da(format!("Invalid hash: {}", e)))?;
        let receipt = self
            .client
            .get_transaction_receipt(hash)
            .await
            .map_err(|e| DomainError::Da(format!("Provider error: {}", e)))?;

        if let Some(r) = receipt {
            // Check status (1 = success, 0 = failure)
            if let Some(status) = r.status {
                if status.as_u64() == 1 {
                    // Check confirmations
                    let block_number = r.block_number.unwrap_or_default();
                    let current_block = self
                        .client
                        .get_block_number()
                        .await
                        .map_err(|e| DomainError::Da(format!("Provider error: {}", e)))?;

                    let confs = current_block.as_u64().saturating_sub(block_number.as_u64());

                    if confs >= 1 {
                        return Ok(true);
                    } else {
                        info!(
                            "Tx mined but waiting for confirmations (current: {})",
                            confs
                        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::providers::{Provider, JsonRpcClient};
    use ethers::signers::{LocalWallet, Signer};
    use ethers::middleware::SignerMiddleware;
    use ethers::types::{Block, U64, TransactionReceipt, FeeHistory};
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use std::sync::Arc;
    use crate::test_utils::MockClient;

    #[tokio::test]
    async fn test_submit_blob() {
        let mock = MockClient::new();
        let provider = Provider::new(mock.clone());
        let wallet: LocalWallet = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20".parse().unwrap();
        let client = Arc::new(SignerMiddleware::new(provider, wallet.with_chain_id(1u64)));
        let bridge_addr = Address::random();
        let bridge = ZKRollupBridge::new(bridge_addr, client.clone());
        
        let blob_hash = H256::random();
        let strategy = BlobStrategy::new(bridge, blob_hash, 0, false);

        let batch = Batch {
             id: crate::domain::batch::BatchId(uuid::Uuid::new_v4()),
             data_file: "test_data_blob.txt".to_string(), 
             new_root: format!("{:#x}", H256::zero()),
             status: crate::domain::batch::BatchStatus::Proving,
             da_mode: "blob".to_string(),
             proof: None,
             tx_hash: None,
             attempts: 0,
             created_at: chrono::Utc::now(),
             updated_at: chrono::Utc::now(),
        };

        // Populate responses
        mock.push(U256::from(0)); // nonce
        let mut block = Block::<H256>::default();
        block.base_fee_per_gas = Some(U256::from(100));
        mock.push(block); // Block
        
        let history = FeeHistory {
            oldest_block: U256::zero(),
            base_fee_per_gas: vec![U256::from(100); 11], 
            gas_used_ratio: vec![0.5; 10],
            reward: vec![],
        };
        mock.push(history); // FeeHistory
        
        mock.push(U256::from(100_000)); // estimateGas
        mock.push(H256::random()); // hash

        let res = strategy.submit(&batch, "proof").await;
        
        if let Err(e) = &res {
            println!("Submit error: {:?}", e);
        }
        assert!(res.is_ok(), "submit failed");
    }
    
    #[tokio::test]
    async fn test_check_confirmation_blob() {
        let mock = MockClient::new();
        let provider = Provider::new(mock.clone());
        let wallet: LocalWallet = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20".parse().unwrap();
        let client = Arc::new(SignerMiddleware::new(provider, wallet.with_chain_id(1u64)));
        let bridge_addr = Address::random();
        let bridge = ZKRollupBridge::new(bridge_addr, client.clone());
        let strategy = BlobStrategy::new(bridge, H256::random(), 0, false);
        
        let tx_hash = H256::random();
        
        mock.push(TransactionReceipt {
            status: Some(U64::from(1)),
            block_number: Some(U64::from(100)),
            ..Default::default()
        });
        
        mock.push(U64::from(105)); 
        
        let res = strategy.check_confirmation(&format!("{:#x}", tx_hash)).await;
        assert!(res.is_ok());
        assert!(res.unwrap());
    }
}
