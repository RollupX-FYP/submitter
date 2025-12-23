use crate::application::ports::DaStrategy;
use crate::contracts::Groth16Proof;
use crate::domain::{batch::Batch, errors::DomainError};
use crate::infrastructure::ethereum_adapter::BridgeClient;
use async_trait::async_trait;
use ethers::prelude::*;
use metrics::counter;
use std::sync::Arc;
use tracing::{info, warn};

pub struct BlobStrategy {
    client: Arc<dyn BridgeClient>,
    blob_versioned_hash: H256,
    blob_index: u8,
    use_opcode: bool,
}

impl BlobStrategy {
    pub fn new(
        client: Arc<dyn BridgeClient>,
        blob_versioned_hash: H256,
        blob_index: u8,
        use_opcode: bool,
    ) -> Self {
        Self {
            client,
            blob_versioned_hash,
            blob_index,
            use_opcode,
        }
    }
}

#[async_trait]
impl DaStrategy for BlobStrategy {
    async fn submit(&self, batch: &Batch, _proof: &str) -> Result<String, DomainError> {
        let proof = Groth16Proof {
            a: [U256::zero(), U256::zero()],
            b: [[U256::zero(), U256::zero()], [U256::zero(), U256::zero()]],
            c: [U256::zero(), U256::zero()],
        };

        let new_root: [u8; 32] = batch
            .new_root
            .parse::<H256>()
            .map_err(|e| DomainError::Da(format!("Invalid new root: {}", e)))?
            .into();

        let tx_hash = self
            .client
            .commit_batch_blob(
                self.blob_versioned_hash.into(),
                self.blob_index,
                self.use_opcode,
                new_root,
                proof,
            )
            .await?;

        info!("Blob batch broadcasted. tx={:?}", tx_hash);

        counter!("tx_submitted_total", "mode" => "blob").increment(1);

        Ok(format!("{:?}", tx_hash))
    }

    async fn check_confirmation(&self, tx_hash: &str) -> Result<bool, DomainError> {
        let hash: H256 = tx_hash
            .parse()
            .map_err(|e| DomainError::Da(format!("Invalid hash: {}", e)))?;
        let receipt = self.client.get_transaction_receipt(hash).await?;

        if let Some(r) = receipt {
            if let Some(status) = r.status {
                if status.as_u64() == 1 {
                    let block_number = r.block_number.unwrap_or_default();
                    let current_block = self.client.get_block_number().await?;

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
    use crate::infrastructure::ethereum_adapter::BridgeClient;
    use tokio::sync::Mutex;

    struct MockBridge {
        tx_hash: H256,
        receipt: Mutex<Option<TransactionReceipt>>,
        block: u64,
    }

    #[async_trait]
    impl BridgeClient for MockBridge {
        async fn commit_batch_calldata(
            &self,
            _batch_data: Bytes,
            _new_root: [u8; 32],
            _proof: Groth16Proof,
        ) -> Result<H256, DomainError> {
            unimplemented!()
        }

        async fn commit_batch_blob(
            &self,
            _versioned_hash: [u8; 32],
            _blob_index: u8,
            _use_opcode: bool,
            _new_root: [u8; 32],
            _proof: Groth16Proof,
        ) -> Result<H256, DomainError> {
            Ok(self.tx_hash)
        }

        async fn get_transaction_receipt(
            &self,
            _hash: H256,
        ) -> Result<Option<TransactionReceipt>, DomainError> {
            let r = self.receipt.lock().await.clone();
            Ok(r)
        }

        async fn get_block_number(&self) -> Result<U64, DomainError> {
            Ok(U64::from(self.block))
        }
    }

    #[tokio::test]
    async fn test_submit_blob() {
        let mock = Arc::new(MockBridge {
            tx_hash: H256::repeat_byte(2),
            receipt: Mutex::new(None),
            block: 10,
        });

        let strategy = BlobStrategy::new(mock, H256::zero(), 0, false);
        let batch = Batch::new(
            1,
            "0xBridge",
            "file".to_string(),
            "hash".to_string(),
            "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            "blob".to_string(),
        );

        let res = strategy.submit(&batch, "proof").await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), format!("{:?}", H256::repeat_byte(2)));
    }

    #[tokio::test]
    async fn test_confirm_success() {
        let mock = Arc::new(MockBridge {
            tx_hash: H256::repeat_byte(2),
            receipt: Mutex::new(Some(TransactionReceipt {
                status: Some(U64::from(1)),
                block_number: Some(U64::from(5)),
                ..Default::default()
            })),
            block: 10,
        });
        let strategy = BlobStrategy::new(mock, H256::zero(), 0, false);
        let res = strategy.check_confirmation(&format!("{:?}", H256::repeat_byte(2))).await;
        assert!(res.unwrap());
    }

    #[tokio::test]
    async fn test_submit_blob_root_error() {
        let mock = Arc::new(MockBridge {
            tx_hash: H256::zero(),
            receipt: Mutex::new(None),
            block: 0,
        });
        let strategy = BlobStrategy::new(mock, H256::zero(), 0, false);
        let batch = Batch::new(
            1,
            "0xBridge",
            "file".to_string(),
            "hash".to_string(),
            "invalid_hex".to_string(),
            "blob".to_string(),
        );
        let res = strategy.submit(&batch, "proof").await;
        assert!(res.is_err());
    }
}
