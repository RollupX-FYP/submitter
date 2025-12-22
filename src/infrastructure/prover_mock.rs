use crate::application::ports::{ProofProvider, ProofResponse};
use crate::domain::{batch::BatchId, errors::DomainError};
use async_trait::async_trait;
use tracing::info;

pub struct MockProofProvider;

#[async_trait]
impl ProofProvider for MockProofProvider {
    async fn get_proof(&self, batch_id: &BatchId, _public_inputs: &[u8]) -> Result<ProofResponse, DomainError> {
        info!("Mock proving for batch {}", batch_id);
        // Simulate delay
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        Ok(ProofResponse {
            proof: "mock_proof_data".to_string(),
        })
    }
}
