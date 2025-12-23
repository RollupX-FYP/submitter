use crate::application::ports::DaStrategy;
use crate::contracts::{Groth16Proof, ZKRollupBridge};
use crate::domain::{batch::Batch, errors::DomainError};
use async_trait::async_trait;
use ethers::prelude::*;
use metrics::counter;
use std::{fs, sync::Arc};
use tracing::{info, warn};

pub struct CalldataStrategy<M: Middleware> {
    bridge: ZKRollupBridge<M>,
    client: Arc<M>,
}

impl<M: Middleware + 'static> CalldataStrategy<M> {
    pub fn new(bridge: ZKRollupBridge<M>) -> Self {
        let client = bridge.client();
        Self { bridge, client }
    }
}

#[async_trait]
impl<M: Middleware + 'static> DaStrategy for CalldataStrategy<M> {
    async fn submit(&self, batch: &Batch, _proof: &str) -> Result<String, DomainError> {
        let proof = Groth16Proof {
            a: [U256::zero(), U256::zero()],
            b: [[U256::zero(), U256::zero()], [U256::zero(), U256::zero()]],
            c: [U256::zero(), U256::zero()],
        };

        let batch_data = fs::read(&batch.data_file)
            .map_err(|e| DomainError::Da(format!("Failed to read batch file: {}", e)))?;

        let new_root: H256 = batch
            .new_root
            .parse()
            .map_err(|e| DomainError::Da(format!("Invalid new root: {}", e)))?;

        let bridge = self.bridge.clone();
        let call = bridge.commit_batch_calldata(batch_data.into(), new_root.into(), proof);

        // Just send, do not wait for mining
        let pending = call
            .send()
            .await
            .map_err(|e| DomainError::Da(format!("Tx send failed: {}", e)))?;

        let tx_hash = pending.tx_hash();
        info!("Calldata batch broadcasted. tx={:?}", tx_hash);

        counter!("tx_submitted_total", "mode" => "calldata").increment(1);

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
                    // In a real env, we might wait for N confirmations.
                    // For MVP, 1 confirmation (mined) with success status is acceptable.
                    // But let's check strict safety if possible.
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
            // If status is missing (pre-Byzantium), assume success if mined (risky but standard for old chains)
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
    async fn test_submit_calldata() {
        let mock = MockClient::new();
        let provider = Provider::new(mock.clone());
        let wallet: LocalWallet = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20".parse().unwrap();
        let client = Arc::new(SignerMiddleware::new(provider, wallet.with_chain_id(1u64)));
        let bridge_addr = Address::random();
        let bridge = ZKRollupBridge::new(bridge_addr, client.clone());
        let strategy = CalldataStrategy::new(bridge);

        let batch = Batch {
             id: crate::domain::batch::BatchId(uuid::Uuid::new_v4()),
             data_file: "test_data_calldata.txt".to_string(), 
             new_root: format!("{:#x}", H256::zero()),
             status: crate::domain::batch::BatchStatus::Proving,
             da_mode: "calldata".to_string(),
             proof: None,
             tx_hash: None,
             attempts: 0,
             created_at: chrono::Utc::now(),
             updated_at: chrono::Utc::now(),
        };

        std::fs::write("test_data_calldata.txt", "dummy data").unwrap();

        // Populate minimal responses based on observation
        mock.push(U256::from(0)); // nonce (eth_getTransactionCount)
        let mut block = Block::<H256>::default();
        block.base_fee_per_gas = Some(U256::from(100));
        mock.push(block); // getBlockByNumber (eth_getBlockByNumber)
        
        let history = FeeHistory {
            oldest_block: U256::zero(),
            base_fee_per_gas: vec![U256::from(100); 11], 
            gas_used_ratio: vec![0.5; 10],
            reward: vec![],
        };
        mock.push(history); // eth_feeHistory
        
        mock.push(U256::from(100_000)); // estimateGas (eth_estimateGas)
        mock.push(H256::random()); // sendRawTransaction (eth_sendRawTransaction)

        let res = strategy.submit(&batch, "proof").await;
        
        let _ = std::fs::remove_file("test_data_calldata.txt");
        if let Err(e) = &res {
            println!("Submit error: {:?}", e);
        }
        assert!(res.is_ok(), "submit failed");
    }

    #[tokio::test]
    async fn test_check_confirmation_success() {
        let mock = MockClient::new();
        let provider = Provider::new(mock.clone());
        let wallet: LocalWallet = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20".parse().unwrap();
        let client = Arc::new(SignerMiddleware::new(provider, wallet.with_chain_id(1u64)));
        let bridge_addr = Address::random();
        let bridge = ZKRollupBridge::new(bridge_addr, client.clone());
        let strategy = CalldataStrategy::new(bridge);
        
        let tx_hash = H256::random();
        
        mock.push(TransactionReceipt {
            status: Some(U64::from(1)),
            block_number: Some(U64::from(100)),
            ..Default::default()
        });
        
        mock.push(U64::from(105)); 
        
        let res = strategy.check_confirmation(&format!("{:#x}", tx_hash)).await;
        if let Err(e) = &res {
            println!("Check conf error: {:?}", e);
        }
        assert!(res.is_ok());
        assert!(res.unwrap());
    }
}
