use crate::application::ports::DaStrategy;
use crate::contracts::Groth16Proof;
use crate::domain::{batch::Batch, errors::DomainError};
use crate::infrastructure::ethereum_adapter::BridgeClient;
use async_trait::async_trait;
use ethers::prelude::*;
use metrics::counter;
use std::{fs, sync::Arc};
use tracing::{info, warn};

pub struct CalldataStrategy {
    client: Arc<dyn BridgeClient>,
}

impl CalldataStrategy {
    pub fn new(client: Arc<dyn BridgeClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl DaStrategy for CalldataStrategy {
    async fn submit(&self, batch: &Batch, _proof: &str) -> Result<String, DomainError> {
        let proof = Groth16Proof {
            a: [U256::zero(), U256::zero()],
            b: [[U256::zero(), U256::zero()], [U256::zero(), U256::zero()]],
            c: [U256::zero(), U256::zero()],
        };

        let batch_data = fs::read(&batch.data_file)
            .map_err(|e| DomainError::Da(format!("Failed to read batch file: {}", e)))?;

        let new_root: [u8; 32] = batch
            .new_root
            .parse::<H256>()
            .map_err(|e| DomainError::Da(format!("Invalid new root: {}", e)))?
            .into();

        let tx_hash = self
            .client
            .commit_batch_calldata(batch_data.into(), new_root, proof)
            .await?;

        info!("Calldata batch broadcasted. tx={:?}", tx_hash);

        counter!("tx_submitted_total", "mode" => "calldata").increment(1);

        Ok(format!("{:?}", tx_hash))
    }

    async fn check_confirmation(&self, tx_hash: &str) -> Result<bool, DomainError> {
        let hash: H256 = tx_hash
            .parse()
            .map_err(|e| DomainError::Da(format!("Invalid hash: {}", e)))?;
        let receipt = self.client.get_transaction_receipt(hash).await?;

        if let Some(r) = receipt {
            // Check status (1 = success, 0 = failure)
            if let Some(status) = r.status {
                if status.as_u64() == 1 {
                    // Check confirmations
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
            Ok(self.tx_hash)
        }

        async fn commit_batch_blob(
            &self,
            _versioned_hash: [u8; 32],
            _blob_index: u8,
            _use_opcode: bool,
            _new_root: [u8; 32],
            _proof: Groth16Proof,
        ) -> Result<H256, DomainError> {
            unimplemented!()
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
    async fn test_submit_calldata() {
        let batch_file = "test_calldata.txt";
        fs::write(batch_file, "data").unwrap();

        let mock = Arc::new(MockBridge {
            tx_hash: H256::repeat_byte(1),
            receipt: Mutex::new(None),
            block: 10,
        });

        let strategy = CalldataStrategy::new(mock);
        let batch = Batch::new(
            1,
            "0xBridge",
            batch_file.to_string(),
            "hash".to_string(),
            "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            "calldata".to_string(),
        );

        let res = strategy.submit(&batch, "proof").await;
        fs::remove_file(batch_file).unwrap();

        assert!(res.is_ok());
        assert_eq!(res.unwrap(), format!("{:?}", H256::repeat_byte(1)));
    }

    #[tokio::test]
    async fn test_confirm_success() {
        let mock = Arc::new(MockBridge {
            tx_hash: H256::repeat_byte(1),
            receipt: Mutex::new(Some(TransactionReceipt {
                status: Some(U64::from(1)),
                block_number: Some(U64::from(5)),
                ..Default::default()
            })),
            block: 10,
        });
        let strategy = CalldataStrategy::new(mock);
        let res = strategy.check_confirmation(&format!("{:?}", H256::repeat_byte(1))).await;
        assert!(res.unwrap());
    }

    #[tokio::test]
    async fn test_submit_calldata_file_error() {
        let mock = Arc::new(MockBridge {
            tx_hash: H256::zero(),
            receipt: Mutex::new(None),
            block: 0,
        });
        let strategy = CalldataStrategy::new(mock);
        let batch = Batch::new(
            1,
            "0xBridge",
            "non_existent_file.txt".to_string(),
            "hash".to_string(),
            "0x00".to_string(),
            "calldata".to_string(),
        );
        let res = strategy.submit(&batch, "proof").await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_submit_calldata_root_error() {
        let batch_file = "test_calldata_root.txt";
        fs::write(batch_file, "data").unwrap();

        let mock = Arc::new(MockBridge {
            tx_hash: H256::zero(),
            receipt: Mutex::new(None),
            block: 0,
        });
        let strategy = CalldataStrategy::new(mock);
        let batch = Batch::new(
            1,
            "0xBridge",
            batch_file.to_string(),
            "hash".to_string(),
            "invalid_hex".to_string(),
            "calldata".to_string(),
        );
        let res = strategy.submit(&batch, "proof").await;
        fs::remove_file(batch_file).unwrap();
        assert!(res.is_err());
    }
}
