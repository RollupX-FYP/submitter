use crate::application::ports::{DaStrategy, SubmissionResult};
use crate::contracts::ZKRollupBridge;
use crate::domain::{batch::Batch, errors::DomainError};
use crate::infrastructure::compression::CompressionStrategy;
use async_trait::async_trait;
use ethers::abi::{encode, Token};
use ethers::prelude::*;
use metrics::counter;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn};

pub struct BlobStrategy<M: Middleware> {
    bridge: ZKRollupBridge<M>,
    client: Arc<M>,
    blob_versioned_hash: H256,
    blob_index: u8,
    archiver_url: Option<String>,
}

impl<M: Middleware + 'static> BlobStrategy<M> {
    pub fn new(
        bridge: ZKRollupBridge<M>,
        blob_versioned_hash: H256,
        blob_index: u8,
        _use_opcode: bool, // Deprecated
        archiver_url: Option<String>,
    ) -> Self {
        let client = bridge.client();
        Self {
            bridge,
            client,
            blob_versioned_hash,
            blob_index,
            archiver_url,
        }
    }
}

#[async_trait]
impl<M: Middleware + 'static> DaStrategy for BlobStrategy<M> {
    fn da_id(&self) -> u8 {
        1
    }

    fn compute_commitment(&self, batch: &Batch) -> Result<H256, DomainError> {
        if let Some(ref hash_str) = batch.blob_versioned_hash {
            H256::from_str(hash_str)
                .map_err(|e| DomainError::Da(format!("Invalid blob versioned hash: {}", e)))
        } else {
            Ok(self.blob_versioned_hash)
        }
    }

    fn encode_da_meta(&self, batch: &Batch) -> Result<Vec<u8>, DomainError> {
        let hash = if let Some(ref hash_str) = batch.blob_versioned_hash {
            H256::from_str(hash_str)
                .map_err(|e| DomainError::Da(format!("Invalid blob versioned hash: {}", e)))?
        } else {
            self.blob_versioned_hash
        };

        let index = batch.blob_index.unwrap_or(self.blob_index);

        Ok(encode(&[
            Token::FixedBytes(hash.as_bytes().to_vec()),
            Token::Uint(index.into()),
        ]))
    }

    async fn submit(
        &self,
        batch: &Batch,
        proof_hex: &str,
        verifier_id: u8,
    ) -> Result<SubmissionResult, DomainError> {
        // 1. Read Payload Data
        let data = std::fs::read(&batch.data_file)
            .map_err(|e| DomainError::Da(format!("Failed to read batch data file: {}", e)))?;

        // Compression
        let (payload_data, metrics) = CompressionStrategy::compress(&data);
        info!(
            "Blob Compression: Ratio={:.2}, GasSaved={}",
            metrics.compression_ratio, metrics.gas_saved
        );

        // 2. Archiver: POST data to external service
        if let Some(url) = &self.archiver_url {
            let client = reqwest::Client::new();
            let res = client
                .post(url)
                .header("Content-Type", "application/octet-stream")
                .body(payload_data.clone())
                .send()
                .await
                .map_err(|e| DomainError::Da(format!("Archiver request failed: {}", e)))?;

            if !res.status().is_success() {
                return Err(DomainError::Da(format!(
                    "Archiver rejected payload: {}",
                    res.status()
                )));
            }
            info!("Blob data archived successfully to {}", url);
        }

        // 3. Construct EIP-4844 Transaction

        // Parse inputs
        let proof_bytes = ethers::utils::hex::decode(proof_hex.trim_start_matches("0x"))
            .map_err(|e| DomainError::Da(format!("Invalid proof hex: {}", e)))?;
        let proof = Bytes::from(proof_bytes);

        let root_bytes = ethers::utils::hex::decode(batch.new_root.trim_start_matches("0x"))
            .map_err(|e| DomainError::Da(format!("Invalid new root hex: {}", e)))?;
        
        let mut new_root_arr = [0u8; 32];
        if root_bytes.len() != 32 {
             return Err(DomainError::Da(format!("New Root must be 32 bytes, got {}", root_bytes.len())));
        }
        new_root_arr.copy_from_slice(&root_bytes);

        let da_meta = self.encode_da_meta(batch)?;

        let call = self.bridge.commit_batch(
            self.da_id(),
            verifier_id,
            Bytes::new(), // batchData is empty for Blob
            da_meta.into(),
            new_root_arr,
            proof,
        );
        let calldata = call
            .calldata()
            .ok_or(DomainError::Da("Failed to encode calldata".into()))?;

        let tx_req = Eip1559TransactionRequest::new()
            .to(self.bridge.address())
            .data(calldata);

        let start_time = std::time::Instant::now();
        let pending = self
            .client
            .send_transaction(tx_req, None)
            .await
            .map_err(|e| DomainError::Da(format!("Tx send failed: {}", e)))?;

        let tx_hash = pending.tx_hash();
        info!("Blob batch broadcasted. tx={:?}", tx_hash);

        let receipt = pending
            .await
            .map_err(|e| DomainError::Da(format!("Receipt failed: {}", e)))?
            .ok_or(DomainError::Da("Dropped".to_string()))?;

        let latency = start_time.elapsed().as_millis() as u64;
        counter!("tx_submitted_total", "mode" => "blob").increment(1);

        let gas_used = receipt.gas_used.map(|g| g.as_u64());

        Ok(SubmissionResult {
            tx_hash: format!("{:?}", tx_hash),
            block_number: receipt.block_number.unwrap_or_default().as_u64(),
            latency_ms: latency,
            compression_ratio: Some(metrics.compression_ratio),
            gas_saved: Some(metrics.gas_saved),
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
    async fn test_submit_blob_with_archiver() {
        let mock = MockClient::new();
        let provider = Provider::new(mock.clone());
        let wallet: LocalWallet =
            "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
                .parse()
                .unwrap();
        let client = Arc::new(SignerMiddleware::new(provider, wallet.with_chain_id(1u64)));
        let bridge_addr = Address::random();
        let bridge = ZKRollupBridge::new(bridge_addr, client.clone());

        let blob_hash = H256::random();
        let strategy = BlobStrategy::new(
            bridge,
            blob_hash,
            0,
            false,
            Some("http://mock-archiver".into()),
        );

        // Create dummy data file
        std::fs::write("test_data_blob_arch.txt", "payload").unwrap();

        let batch = Batch {
            id: crate::domain::batch::BatchId(uuid::Uuid::new_v4()),
            data_file: "test_data_blob_arch.txt".to_string(),
            new_root: format!("{:#x}", H256::zero()),
            status: crate::domain::batch::BatchStatus::Proving,
            da_mode: "blob".to_string(),
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

        // Populate responses
        mock.push(U256::from(0)); // nonce
        let mut block = Block::<H256>::default();
        block.base_fee_per_gas = Some(U256::from(100));
        mock.push(block); // Block
        mock.push(FeeHistory {
            oldest_block: U256::zero(),
            base_fee_per_gas: vec![U256::from(100)],
            gas_used_ratio: vec![],
            reward: vec![],
        }); // FeeHistory
        mock.push(U256::from(100_000)); // estimateGas
        mock.push(H256::random()); // hash

        let proof_hex = format!("0x{}", hex::encode([0u8; 128]));

        let res = strategy.submit(&batch, &proof_hex, 0).await;
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Archiver request failed"));

        std::fs::remove_file("test_data_blob_arch.txt").unwrap();
    }
}
