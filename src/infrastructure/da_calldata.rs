use crate::application::ports::{DaStrategy, SubmissionResult};
use crate::config::CompressionMode;
use crate::contracts::ZKRollupBridge;
use crate::domain::{batch::Batch, errors::DomainError};
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::utils::keccak256;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use metrics::counter;
use std::io::Write;
use std::{fs, sync::Arc};
use tracing::{info, warn};

pub struct CalldataStrategy<M: Middleware> {
    bridge: ZKRollupBridge<M>,
    client: Arc<M>,
    compression_mode: Option<CompressionMode>,
}

impl<M: Middleware + 'static> CalldataStrategy<M> {
    pub fn new(bridge: ZKRollupBridge<M>, compression_mode: Option<CompressionMode>) -> Self {
        let client = bridge.client();
        Self {
            bridge,
            client,
            compression_mode,
        }
    }
}

#[async_trait]
impl<M: Middleware + 'static> DaStrategy for CalldataStrategy<M> {
    fn da_id(&self) -> u8 {
        0
    }

    fn compute_commitment(&self, batch: &Batch) -> Result<H256, DomainError> {
        let mut batch_data = fs::read(&batch.data_file)
            .map_err(|e| DomainError::Da(format!("Failed to read batch file: {}", e)))?;

        if self.compression_mode.is_some() {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(&batch_data)
                .map_err(|e| DomainError::Da(format!("Compression failed: {}", e)))?;
            batch_data = encoder
                .finish()
                .map_err(|e| DomainError::Da(format!("Compression failed: {}", e)))?;
        }

        Ok(H256::from(keccak256(&batch_data)))
    }

    fn encode_da_meta(&self, _batch: &Batch) -> Result<Vec<u8>, DomainError> {
        Ok(Vec::new())
    }

    async fn submit(
        &self,
        batch: &Batch,
        proof_hex: &str,
        verifier_id: u8,
    ) -> Result<SubmissionResult, DomainError> {
        // Convert proof hex to Bytes
        let proof_bytes = ethers::utils::hex::decode(proof_hex.trim_start_matches("0x"))
            .map_err(|e| DomainError::Da(format!("Invalid proof hex: {}", e)))?;
        let proof = Bytes::from(proof_bytes);

        let mut batch_data = fs::read(&batch.data_file)
            .map_err(|e| DomainError::Da(format!("Failed to read batch file: {}", e)))?;

        if self.compression_mode.is_some() {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(&batch_data)
                .map_err(|e| DomainError::Da(format!("Compression failed: {}", e)))?;
            batch_data = encoder
                .finish()
                .map_err(|e| DomainError::Da(format!("Compression failed: {}", e)))?;
        }

        // Correctly parse hex string into [u8; 32] to avoid padding issues with H256/U256
        let root_bytes = ethers::utils::hex::decode(batch.new_root.trim_start_matches("0x"))
            .map_err(|e| DomainError::Da(format!("Invalid new root hex: {}", e)))?;
        
        let mut new_root_arr = [0u8; 32];
        if root_bytes.len() != 32 {
             return Err(DomainError::Da(format!("New Root must be 32 bytes, got {}", root_bytes.len())));
        }
        new_root_arr.copy_from_slice(&root_bytes);

        let da_meta = self.encode_da_meta(batch)?;

        let bridge = self.bridge.clone();
        let call = bridge.commit_batch(
            self.da_id(),
            verifier_id,
            batch_data.into(),
            da_meta.into(),
            new_root_arr,
            proof,
        );

        let start_time = std::time::Instant::now();
        let pending = call
            .send()
            .await
            .map_err(|e| DomainError::Da(format!("Tx send failed: {}", e)))?;

        let tx_hash = pending.tx_hash();
        info!("Calldata batch broadcasted. tx={:?}", tx_hash);

        let receipt = pending
            .await
            .map_err(|e| DomainError::Da(format!("Receipt failed: {}", e)))?
            .ok_or(DomainError::Da("Dropped".to_string()))?;

        let latency = start_time.elapsed().as_millis() as u64;
        counter!("tx_submitted_total", "mode" => "calldata").increment(1);

        let gas_used = receipt.gas_used.map(|g| g.as_u64());

        Ok(SubmissionResult {
            tx_hash: format!("{:?}", tx_hash),
            block_number: receipt.block_number.unwrap_or_default().as_u64(),
            latency_ms: latency,
            compression_ratio: None,
            gas_saved: None,
            gas_used,
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
    use crate::test_utils::MockClient;
    use ethers::middleware::SignerMiddleware;
    use ethers::providers::Provider;
    use ethers::signers::{LocalWallet, Signer};
    use ethers::types::{Block, FeeHistory, TransactionReceipt, U64};
    use ethers::utils::hex;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_submit_calldata() {
        let mock = MockClient::new();
        let provider = Provider::new(mock.clone());
        let wallet: LocalWallet =
            "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
                .parse()
                .unwrap();
        let client = Arc::new(SignerMiddleware::new(provider, wallet.with_chain_id(1u64)));
        let bridge_addr = Address::random();
        let bridge = ZKRollupBridge::new(bridge_addr, client.clone());
        let strategy = CalldataStrategy::new(bridge, None);

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
            blob_versioned_hash: None,
            blob_index: None,
            fee: 0,
            experiment_id: None,
        };

        std::fs::write("test_data_calldata.txt", "dummy data").unwrap();

        // Populate minimal responses
        mock.push(U256::from(0));
        let mut block = Block::<H256>::default();
        block.base_fee_per_gas = Some(U256::from(100));
        mock.push(block);

        let history = FeeHistory {
            oldest_block: U256::zero(),
            base_fee_per_gas: vec![U256::from(100); 11],
            gas_used_ratio: vec![0.5; 10],
            reward: vec![],
        };
        mock.push(history);

        mock.push(U256::from(100_000));
        mock.push(H256::random());

        let proof_hex = format!("0x{}", hex::encode([0u8; 128])); // Random bytes
        let res = strategy.submit(&batch, &proof_hex, 0).await;

        let _ = std::fs::remove_file("test_data_calldata.txt");
        if let Err(e) = &res {
            println!("Submit error: {:?}", e);
        }
        assert!(res.is_ok(), "submit failed");
    }
}
