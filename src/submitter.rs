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
