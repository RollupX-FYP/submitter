use crate::contracts::{Groth16Proof, ZKRollupBridge};
use anyhow::{Context, Result};
use ethers::prelude::*;

pub struct Submitter<M: Middleware> {
    bridge: ZKRollupBridge<M>,
}

impl<M: Middleware + 'static> Submitter<M> {
    pub fn new(bridge: ZKRollupBridge<M>) -> Self {
        Self { bridge }
    }

    pub async fn submit_calldata(
        &self,
        batch_data: Vec<u8>,
        new_root: [u8; 32],
        proof: Groth16Proof,
    ) -> Result<H256> {
        // Break down the chain to manage lifetimes
        let bridge = self.bridge.clone();
        let call = bridge.commit_batch_calldata(batch_data.into(), new_root, proof);
        let pending = call.send().await?;

        let receipt = pending.await?.context("tx dropped")?;
        Ok(receipt.transaction_hash)
    }

    pub async fn submit_blob(
        &self,
        expected_versioned_hash: [u8; 32],
        blob_index: u8,
        use_opcode: bool,
        new_root: [u8; 32],
        proof: Groth16Proof,
    ) -> Result<H256> {
        let bridge = self.bridge.clone();
        let call = bridge.commit_batch_blob(
            expected_versioned_hash,
            blob_index,
            use_opcode,
            new_root,
            proof,
        );
        let pending = call.send().await?;

        let receipt = pending.await?.context("tx dropped")?;
        Ok(receipt.transaction_hash)
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
    use std::sync::{Arc, Mutex};
    use std::fmt::Debug;

    #[derive(Clone, Debug)]
    struct MockClient {
        responses: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl MockClient {
        fn new() -> Self {
            Self { responses: Arc::new(Mutex::new(Vec::new())) }
        }
        fn push<T: Serialize>(&self, res: T) {
            self.responses.lock().unwrap().push(serde_json::to_value(res).unwrap());
        }
    }

    #[async_trait::async_trait]
    impl JsonRpcClient for MockClient {
        type Error = ethers::providers::ProviderError;

        async fn request<T, R>(&self, method: &str, params: T) -> Result<R, Self::Error>
        where
            T: Debug + Serialize + Send + Sync,
            R: DeserializeOwned + Send,
        {
            println!("Request: {} {:?}", method, params);
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                return Err(ethers::providers::ProviderError::CustomError(format!("No responses for {}", method)));
            }
            let res = responses.remove(0);
            serde_json::from_value(res).map_err(|e| ethers::providers::ProviderError::SerdeJson(e))
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_submitter_calldata() {
        let mock = MockClient::new();
        let provider = Provider::new(mock.clone());
        let wallet: LocalWallet = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20".parse().unwrap();
        let client = Arc::new(SignerMiddleware::new(provider, wallet.with_chain_id(1u64)));
        let bridge_addr = Address::random();
        let bridge = ZKRollupBridge::new(bridge_addr, client.clone());
        let submitter = Submitter::new(bridge);
        
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
        let tx_hash = H256::random();
        mock.push(tx_hash);
        
        mock.push(TransactionReceipt {
            status: Some(U64::from(1)),
            block_number: Some(U64::from(100)),
            transaction_hash: tx_hash,
            ..Default::default()
        });
        mock.push(U64::from(101));

        let proof = Groth16Proof {
            a: [U256::zero(), U256::zero()],
            b: [[U256::zero(), U256::zero()], [U256::zero(), U256::zero()]],
            c: [U256::zero(), U256::zero()],
        };

        let res = submitter.submit_calldata(vec![0u8; 32], [0u8; 32], proof).await;
        
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), tx_hash);
    }
    
    #[tokio::test]
    #[ignore]
    async fn test_submitter_blob() {
        let mock = MockClient::new();
        let provider = Provider::new(mock.clone());
        let wallet: LocalWallet = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20".parse().unwrap();
        let client = Arc::new(SignerMiddleware::new(provider, wallet.with_chain_id(1u64)));
        let bridge_addr = Address::random();
        let bridge = ZKRollupBridge::new(bridge_addr, client.clone());
        let submitter = Submitter::new(bridge);
        
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
        let tx_hash = H256::random();
        mock.push(tx_hash);
        
        mock.push(TransactionReceipt {
            status: Some(U64::from(1)),
            block_number: Some(U64::from(100)),
            transaction_hash: tx_hash,
            ..Default::default()
        });
        mock.push(U64::from(101));

        let proof = Groth16Proof {
            a: [U256::zero(), U256::zero()],
            b: [[U256::zero(), U256::zero()], [U256::zero(), U256::zero()]],
            c: [U256::zero(), U256::zero()],
        };

        let res = submitter.submit_blob([0u8; 32], 0, false, [0u8; 32], proof).await;
        
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), tx_hash);
    }
}
