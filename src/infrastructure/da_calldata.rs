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
