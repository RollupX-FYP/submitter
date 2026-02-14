use crate::application::ports::{DaStrategy, SubmissionResult};
use crate::contracts::ZKRollupBridge;
use crate::domain::{batch::Batch, errors::DomainError};
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::utils::{hex, keccak256};
use std::path::Path;
use std::{fs, sync::Arc};
use tracing::info;

pub struct OffChainStrategy<M: Middleware> {
    bridge: ZKRollupBridge<M>,
    client: Arc<M>,
    store_dir: String,
}

impl<M: Middleware + 'static> OffChainStrategy<M> {
    pub fn new(bridge: ZKRollupBridge<M>) -> Self {
        let client = bridge.client();
        Self {
            bridge,
            client,
            store_dir: "offchain_store".to_string(),
        }
    }
}

#[async_trait]
impl<M: Middleware + 'static> DaStrategy for OffChainStrategy<M> {
    fn da_id(&self) -> u8 {
        2 // OffChain Mode
    }

    fn compute_commitment(&self, batch: &Batch) -> Result<H256, DomainError> {
        // Commitment is still keccak256 of data, same as Prover used.
        let batch_data = fs::read(&batch.data_file)
            .map_err(|e| DomainError::Da(format!("Failed to read batch file: {}", e)))?;
        Ok(H256::from(keccak256(&batch_data)))
    }

    fn encode_da_meta(&self, batch: &Batch) -> Result<Vec<u8>, DomainError> {
        // For OffChain, daMeta is the pointer/CID. We use the hash of data as CID.
        let commitment = self.compute_commitment(batch)?;
        Ok(commitment.as_bytes().to_vec())
    }

    async fn submit(
        &self,
        batch: &Batch,
        proof_hex: &str,
        verifier_id: u8,
    ) -> Result<SubmissionResult, DomainError> {
        // 1. Off-load Data
        let batch_data = fs::read(&batch.data_file)
            .map_err(|e| DomainError::Da(format!("Failed to read batch file: {}", e)))?;

        let cid = keccak256(&batch_data);
        let cid_hex = hex::encode(cid);

        let file_path = format!("{}/{}.bin", self.store_dir, cid_hex);

        // Ensure store exists
        tokio::fs::create_dir_all(&self.store_dir)
            .await
            .map_err(|e| DomainError::Da(format!("Failed to create offchain store: {}", e)))?;

        tokio::fs::write(&file_path, &batch_data)
            .await
            .map_err(|e| DomainError::Da(format!("Failed to write offchain data: {}", e)))?;

        info!("OffChain DA: Stored data at {}", file_path);

        // 2. Submit Transaction
        let proof_bytes = hex::decode(proof_hex.trim_start_matches("0x"))
            .map_err(|e| DomainError::Da(format!("Invalid proof hex: {}", e)))?;
        let proof = Bytes::from(proof_bytes);

        let new_root: H256 = batch
            .new_root
            .parse()
            .map_err(|e| DomainError::Da(format!("Invalid new root: {}", e)))?;

        let da_meta = self.encode_da_meta(batch)?;

        // Send empty batchData to save gas
        let empty_batch_data = Bytes::new();

        let bridge = self.bridge.clone();
        let call = bridge.commit_batch(
            self.da_id(),
            verifier_id,
            empty_batch_data,
            da_meta.into(),
            new_root.into(),
            proof,
        );

        let start_time = std::time::Instant::now();
        let pending = call
            .send()
            .await
            .map_err(|e| DomainError::Da(format!("Tx send failed: {}", e)))?;

        let tx_hash = pending.tx_hash();
        info!("OffChain batch submitted (pointer only). tx={:?}", tx_hash);

        let receipt = pending
            .await
            .map_err(|e| DomainError::Da(format!("Receipt failed: {}", e)))?
            .ok_or(DomainError::Da("Dropped".to_string()))?;

        let latency = start_time.elapsed().as_millis() as u64;

        Ok(SubmissionResult {
            tx_hash: format!("{:?}", tx_hash),
            block_number: receipt.block_number.unwrap_or_default().as_u64(),
            latency_ms: latency,
            compression_ratio: None,
            gas_saved: None,
        })
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
                        return Ok(false);
                    }
                } else {
                    return Err(DomainError::Da("Transaction reverted on-chain".to_string()));
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
