use crate::application::ports::{BridgeReader, DaStrategy, ProofProvider, Storage};
use crate::domain::{
    batch::{Batch, BatchStatus},
    errors::DomainError,
};
use ethers::types::{H256, U256};
use metrics::{counter, histogram};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

// BN254 Scalar Field Modulus
// 21888242871839275222246405745257275088548364400416034343698204186575808495617
const SNARK_SCALAR_FIELD: U256 = U256([
    0x43e1f593f0000001,
    0x2833e84879b97091,
    0xb85045b68181585d,
    0x30644e72e131a029,
]);

pub struct Orchestrator {
    storage: Arc<dyn Storage>,
    prover: Arc<dyn ProofProvider>,
    da_strategy: Arc<dyn DaStrategy>,
    bridge_reader: Arc<dyn BridgeReader>,
    max_attempts: u32,
}

impl Orchestrator {
    pub fn new(
        storage: Arc<dyn Storage>,
        prover: Arc<dyn ProofProvider>,
        da_strategy: Arc<dyn DaStrategy>,
        bridge_reader: Arc<dyn BridgeReader>,
        max_attempts: u32,
    ) -> Self {
        Self {
            storage,
            prover,
            da_strategy,
            bridge_reader,
            max_attempts,
        }
    }

    pub async fn run(&self) -> Result<(), DomainError> {
        info!("Orchestrator started");
        loop {
            if let Err(e) = self.process_pending_batches().await {
                error!("Error processing batches: {}", e);
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    pub async fn process_pending_batches(&self) -> Result<(), DomainError> {
        let batches = self.storage.get_pending_batches().await?;

        for mut batch in batches {
            self.process_batch(&mut batch).await?;
        }
        Ok(())
    }

    async fn handle_failure(
        &self,
        batch: &mut Batch,
        error_msg: String,
    ) -> Result<(), DomainError> {
        batch.attempts += 1;

        counter!("batch_failures_total", "batch_id" => batch.id.to_string()).increment(1);

        if batch.attempts >= self.max_attempts {
            warn!(
                "Batch {} FAILED permanently after {} attempts: {}",
                batch.id, batch.attempts, error_msg
            );
            batch.transition_to(BatchStatus::Failed);
            counter!("batches_failed_permanent_total").increment(1);
        } else {
            warn!(
                "Batch {} failed (attempt {}/{}): {}. Retrying...",
                batch.id, batch.attempts, self.max_attempts, error_msg
            );
        }
        self.storage.save_batch(batch).await
    }

    #[tracing::instrument(skip(self, batch), fields(batch_id = %batch.id, status = %batch.status))]
    async fn process_batch(&self, batch: &mut Batch) -> Result<(), DomainError> {
        info!("Processing batch");
        let start = Instant::now();

        match batch.status {
            BatchStatus::Discovered => {
                batch.transition_to(BatchStatus::Proving);
                self.storage.save_batch(batch).await?;
                counter!("batch_transitions_total", "from" => "Discovered", "to" => "Proving")
                    .increment(1);
            }
            BatchStatus::Proving => {
                // 1. Fetch L1 Context (BridgeReader)
                let old_root_res = self.bridge_reader.state_root().await;
                // 2. Compute Commitment (DaStrategy)
                let commitment_res = self.da_strategy.compute_commitment(batch);

                match (old_root_res, commitment_res) {
                    (Ok(old_root_h256), Ok(commitment_h256)) => {
                        // 3. Sanitize Inputs (Orchestrator)
                        let da_input = U256::from_big_endian(commitment_h256.as_bytes()) % SNARK_SCALAR_FIELD;
                        let old_root_input = U256::from_big_endian(old_root_h256.as_bytes()) % SNARK_SCALAR_FIELD;

                        // Parse new_root from hex string
                        let new_root_val = match batch.new_root.parse::<H256>() {
                            Ok(h) => U256::from_big_endian(h.as_bytes()) % SNARK_SCALAR_FIELD,
                            Err(e) => {
                                self.handle_failure(batch, format!("Invalid new_root: {}", e))
                                    .await?;
                                return Ok(());
                            }
                        };

                        // 4. Request Proof
                        // Format public inputs as bytes. The Prover likely expects 32-byte chunks.
                        // Order: daCommitment, oldRoot, newRoot
                        let mut public_inputs = Vec::with_capacity(96);
                        let mut buf = [0u8; 32];
                        da_input.to_big_endian(&mut buf);
                        public_inputs.extend_from_slice(&buf);
                        old_root_input.to_big_endian(&mut buf);
                        public_inputs.extend_from_slice(&buf);
                        new_root_val.to_big_endian(&mut buf);
                        public_inputs.extend_from_slice(&buf);

                        match self.prover.get_proof(&batch.id, &public_inputs).await {
                            Ok(response) => {
                                batch.proof = Some(response.proof);
                                batch.transition_to(BatchStatus::Proved);
                                batch.attempts = 0;
                                self.storage.save_batch(batch).await?;

                                counter!("batch_transitions_total", "from" => "Proving", "to" => "Proved")
                                    .increment(1);
                                histogram!("prove_duration_seconds").record(start.elapsed().as_secs_f64());
                            }
                            Err(e) => {
                                self.handle_failure(batch, e.to_string()).await?;
                            }
                        }
                    }
                    (Err(e), _) => {
                         self.handle_failure(batch, format!("Failed to fetch state root: {}", e)).await?;
                    }
                    (_, Err(e)) => {
                        self.handle_failure(batch, format!("Failed to compute commitment: {}", e)).await?;
                    }
                }
            }
            BatchStatus::Proved => {
                batch.transition_to(BatchStatus::Submitting);
                self.storage.save_batch(batch).await?;
                counter!("batch_transitions_total", "from" => "Proved", "to" => "Submitting")
                    .increment(1);
            }
            BatchStatus::Submitting => {
                if let Some(proof) = &batch.proof {
                    match self.da_strategy.submit(batch, proof).await {
                        Ok(tx_hash) => {
                            batch.tx_hash = Some(tx_hash);
                            batch.transition_to(BatchStatus::Submitted);
                            batch.attempts = 0;
                            self.storage.save_batch(batch).await?;

                            counter!("batch_transitions_total", "from" => "Submitting", "to" => "Submitted").increment(1);
                            histogram!("submit_tx_duration_seconds")
                                .record(start.elapsed().as_secs_f64());
                        }
                        Err(e) => {
                            self.handle_failure(batch, e.to_string()).await?;
                        }
                    }
                } else {
                    error!("Missing proof for batch {}", batch.id);
                    batch.transition_to(BatchStatus::Failed);
                    self.storage.save_batch(batch).await?;
                    counter!("batches_failed_permanent_total", "reason" => "missing_proof")
                        .increment(1);
                }
            }
            BatchStatus::Submitted => {
                if let Some(tx_hash) = &batch.tx_hash {
                    match self.da_strategy.check_confirmation(tx_hash).await {
                        Ok(confirmed) => {
                            if confirmed {
                                batch.transition_to(BatchStatus::Confirmed);
                                self.storage.save_batch(batch).await?;
                                info!("Batch {} CONFIRMED", batch.id);

                                counter!("batch_transitions_total", "from" => "Submitted", "to" => "Confirmed").increment(1);
                                counter!("batches_completed_total").increment(1);

                                // Calculate total duration since creation
                                let total_duration =
                                    chrono::Utc::now().signed_duration_since(batch.created_at);
                                histogram!("batch_e2e_duration_seconds")
                                    .record(total_duration.num_seconds() as f64);
                            } else {
                                info!("Batch {} still pending confirmation", batch.id);
                            }
                        }
                        Err(e) => {
                            warn!("Error checking confirmation for {}: {}", batch.id, e);
                            // If it's a transient check error, we might not want to count as failure attempt?
                            // But if the check fails permanently (e.g. reverted), we should handle failure.
                            // Currently check_confirmation returns false if pending, Error if reverted or rpc error.
                            // Ideally we distinguish Revert vs RPC Error. For now treat as failure.
                            self.handle_failure(batch, e.to_string()).await?;
                        }
                    }
                } else {
                    batch.transition_to(BatchStatus::Submitting);
                    self.storage.save_batch(batch).await?;
                    counter!("batch_reverted_to_submitting_total").increment(1);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::{BridgeReader, DaStrategy, ProofProvider, ProofResponse, Storage};
    use crate::domain::{
        batch::{Batch, BatchId},
        errors::DomainError,
    };
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    // Mocks
    struct MockStorage {
        batch: Mutex<Option<Batch>>,
    }

    #[async_trait]
    impl Storage for MockStorage {
        async fn save_batch(&self, batch: &Batch) -> Result<(), DomainError> {
            *self.batch.lock().unwrap() = Some(batch.clone());
            Ok(())
        }
        async fn get_batch(&self, _id: BatchId) -> Result<Option<Batch>, DomainError> {
            Ok(self.batch.lock().unwrap().clone())
        }
        async fn get_pending_batches(&self) -> Result<Vec<Batch>, DomainError> {
            let b = self.batch.lock().unwrap().clone();
            Ok(b.into_iter().collect())
        }
    }

    struct MockProver {
        should_fail: bool,
    }

    #[async_trait]
    impl ProofProvider for MockProver {
        async fn get_proof(
            &self,
            _id: &BatchId,
            _input: &[u8],
        ) -> Result<ProofResponse, DomainError> {
            if self.should_fail {
                Err(DomainError::Prover("fail".into()))
            } else {
                Ok(ProofResponse { proof: "p".into() })
            }
        }
    }

    struct MockDa {
        should_fail_submit: bool,
        should_fail_confirm: bool,
        confirm_result: bool,
    }

    #[async_trait]
    impl DaStrategy for MockDa {
        fn da_id(&self) -> u8 { 0 }
        fn compute_commitment(&self, _batch: &Batch) -> Result<H256, DomainError> {
            Ok(H256::zero())
        }
        fn encode_da_meta(&self, _batch: &Batch) -> Result<Vec<u8>, DomainError> {
             Ok(vec![])
        }

        async fn submit(&self, _b: &Batch, _p: &str) -> Result<String, DomainError> {
            if self.should_fail_submit {
                Err(DomainError::Da("fail".into()))
            } else {
                Ok("0xhash".into())
            }
        }
        async fn check_confirmation(&self, _tx: &str) -> Result<bool, DomainError> {
            if self.should_fail_confirm {
                Err(DomainError::Da("revert".into()))
            } else {
                Ok(self.confirm_result)
            }
        }
    }

    struct MockBridgeReader;
    #[async_trait]
    impl BridgeReader for MockBridgeReader {
        async fn state_root(&self) -> Result<H256, DomainError> {
            Ok(H256::zero())
        }
    }

    fn create_orchestrator(
        batch: Batch,
        prover_fail: bool,
        da_fail: bool,
        da_confirm_fail: bool,
    ) -> (Orchestrator, Arc<MockStorage>) {
        let storage = Arc::new(MockStorage {
            batch: Mutex::new(Some(batch)),
        });
        let prover = Arc::new(MockProver {
            should_fail: prover_fail,
        });
        let da = Arc::new(MockDa {
            should_fail_submit: da_fail,
            should_fail_confirm: da_confirm_fail,
            confirm_result: true,
        });
        let reader = Arc::new(MockBridgeReader);

        (
            Orchestrator::new(storage.clone(), prover, da, reader, 5),
            storage,
        )
    }

    // Valid 32-byte hex for tests
    const VALID_HASH: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";

    #[tokio::test]
    async fn test_proving_success() {
        let batch = Batch::new(1, "b", "f".into(), "h".into(), VALID_HASH.into(), "m".into());
        let (orch, store) = create_orchestrator(batch.clone(), false, false, false);

        // Discovered -> Proving
        orch.process_pending_batches().await.unwrap();
        // Proving -> Proved
        orch.process_pending_batches().await.unwrap();

        let updated = store.get_batch(batch.id).await.unwrap().unwrap();
        assert_eq!(updated.status, BatchStatus::Proved);
        assert!(updated.proof.is_some());
    }

    #[tokio::test]
    async fn test_proving_retry() {
        let mut batch = Batch::new(1, "b", "f".into(), "h".into(), VALID_HASH.into(), "m".into());
        batch.status = BatchStatus::Proving;

        let (orch, store) = create_orchestrator(batch.clone(), true, false, false);

        orch.process_pending_batches().await.unwrap();

        let updated = store.get_batch(batch.id).await.unwrap().unwrap();
        assert_eq!(updated.status, BatchStatus::Proving);
        assert_eq!(updated.attempts, 1);
    }

    #[tokio::test]
    async fn test_proving_dead_letter() {
        let mut batch = Batch::new(1, "b", "f".into(), "h".into(), VALID_HASH.into(), "m".into());
        batch.status = BatchStatus::Proving;
        batch.attempts = 4; // Max is 5

        let (orch, store) = create_orchestrator(batch.clone(), true, false, false);

        orch.process_pending_batches().await.unwrap();

        let updated = store.get_batch(batch.id).await.unwrap().unwrap();
        assert_eq!(updated.status, BatchStatus::Failed);
    }

    #[tokio::test]
    async fn test_submitting_missing_proof() {
        let mut batch = Batch::new(1, "b", "f".into(), "h".into(), VALID_HASH.into(), "m".into());
        batch.status = BatchStatus::Submitting;
        batch.proof = None; // Should fail

        let (orch, store) = create_orchestrator(batch.clone(), false, false, false);

        orch.process_pending_batches().await.unwrap();

        let updated = store.get_batch(batch.id).await.unwrap().unwrap();
        assert_eq!(updated.status, BatchStatus::Failed);
    }

    #[tokio::test]
    async fn test_submitted_revert() {
        let mut batch = Batch::new(1, "b", "f".into(), "h".into(), VALID_HASH.into(), "m".into());
        batch.status = BatchStatus::Submitted;
        batch.tx_hash = Some("0x123".into());

        // Simulate Revert (error in check_confirmation)
        let (orch, store) = create_orchestrator(batch.clone(), false, false, true);

        orch.process_pending_batches().await.unwrap();

        let updated = store.get_batch(batch.id).await.unwrap().unwrap();
        assert_eq!(updated.attempts, 1); // Should count as failure
    }
}
