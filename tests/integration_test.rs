use async_trait::async_trait;
use ethers::types::H256;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use submitter_rs::{
    application::{
        orchestrator::Orchestrator,
        ports::{BridgeReader, DaStrategy, ProofProvider, ProofResponse, Storage},
    },
    domain::{
        batch::{Batch, BatchId, BatchStatus},
        errors::DomainError,
    },
    infrastructure::storage_sqlite::SqliteStorage,
};
use uuid::Uuid;

// Mock Bridge Reader
struct MockBridgeReader;
#[async_trait]
impl BridgeReader for MockBridgeReader {
    async fn state_root(&self) -> Result<H256, DomainError> {
        Ok(H256::zero())
    }
}

// Mock DA Strategy
struct MockDaStrategy {
    tx_hash: StdMutex<Option<String>>,
}

impl MockDaStrategy {
    fn new() -> Self {
        Self {
            tx_hash: StdMutex::new(None),
        }
    }
}

#[async_trait]
impl DaStrategy for MockDaStrategy {
    fn da_id(&self) -> u8 {
        0
    }

    fn compute_commitment(&self, _batch: &Batch) -> Result<H256, DomainError> {
        Ok(H256::zero())
    }

    fn encode_da_meta(&self, _batch: &Batch) -> Result<Vec<u8>, DomainError> {
        Ok(vec![])
    }

    async fn submit(&self, _batch: &Batch, _proof: &str) -> Result<String, DomainError> {
        let hash = format!("0x{}", Uuid::new_v4().simple());
        *self.tx_hash.lock().unwrap() = Some(hash.clone());
        Ok(hash)
    }

    async fn check_confirmation(&self, tx_hash: &str) -> Result<bool, DomainError> {
        let stored = self.tx_hash.lock().unwrap().clone();
        if let Some(h) = stored {
            Ok(h == tx_hash)
        } else {
            Ok(false)
        }
    }
}

// Mock Proof Provider
struct TestProofProvider;
#[async_trait]
impl ProofProvider for TestProofProvider {
    async fn get_proof(
        &self,
        _batch_id: &BatchId,
        _public_inputs: &[u8],
    ) -> Result<ProofResponse, DomainError> {
        Ok(ProofResponse {
            proof: "test_proof".to_string(),
        })
    }
}

#[tokio::test]
async fn test_batch_lifecycle() {
    // 1. Setup Storage
    let db_url = "sqlite::memory:";
    let storage = Arc::new(
        SqliteStorage::new(db_url)
            .await
            .expect("Failed to create storage"),
    );

    // 2. Setup Components
    let prover = Arc::new(TestProofProvider);
    let da = Arc::new(MockDaStrategy::new());
    let reader = Arc::new(MockBridgeReader);
    let orchestrator = Orchestrator::new(storage.clone(), prover, da, reader, 5);

    // 3. Create a batch
    let batch = Batch::new(
        1,
        "0xBridge",
        "data.txt".to_string(),
        "hash123".to_string(),
        "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(), // Valid hex
        "calldata".to_string(),
    );
    storage
        .save_batch(&batch)
        .await
        .expect("Failed to save batch");

    // 4. Run one iteration of orchestrator (Discovered -> Proving)
    orchestrator
        .process_pending_batches()
        .await
        .expect("Failed to process");

    let updated = storage.get_batch(batch.id).await.unwrap().unwrap();
    assert_eq!(updated.status, BatchStatus::Proving);

    // 5. Run iteration (Proving -> Proved)
    orchestrator
        .process_pending_batches()
        .await
        .expect("Failed to process");
    let updated = storage.get_batch(batch.id).await.unwrap().unwrap();
    assert_eq!(updated.status, BatchStatus::Proved);
    assert!(updated.proof.is_some());

    // 6. Run iteration (Proved -> Submitting)
    // Moves to Submitting state
    orchestrator
        .process_pending_batches()
        .await
        .expect("Failed to process");
    let updated = storage.get_batch(batch.id).await.unwrap().unwrap();
    assert_eq!(updated.status, BatchStatus::Submitting);

    // 7. Run iteration (Submitting -> Submitted)
    // Actually submits and gets tx hash
    orchestrator
        .process_pending_batches()
        .await
        .expect("Failed to process");
    let updated = storage.get_batch(batch.id).await.unwrap().unwrap();
    assert_eq!(updated.status, BatchStatus::Submitted);
    assert!(updated.tx_hash.is_some());

    // 8. Run iteration (Submitted -> Confirmed)
    orchestrator
        .process_pending_batches()
        .await
        .expect("Failed to process");
    let updated = storage.get_batch(batch.id).await.unwrap().unwrap();
    assert_eq!(updated.status, BatchStatus::Confirmed);
}
