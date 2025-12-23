use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use submitter_rs::{
    application::{
        orchestrator::Orchestrator,
        ports::{DaStrategy, ProofProvider, ProofResponse, Storage},
    },
    domain::{
        batch::{Batch, BatchId, BatchStatus},
        errors::DomainError,
    },
    infrastructure::storage_sqlite::SqliteStorage,
};
use uuid::Uuid;

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
    let orchestrator = Orchestrator::new(storage.clone(), prover, da, 5);

    // 3. Create a batch
    let batch = Batch::new(
        1,
        "0xBridge",
        "data.txt".to_string(),
        "hash123".to_string(),
        "0x123".to_string(),
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
