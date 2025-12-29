use crate::application::ports::{ProofProvider, ProofResponse};
use crate::domain::{batch::BatchId, errors::DomainError};
use async_trait::async_trait;
use tracing::info;

pub struct MockProofProvider {
    delay_ms: u64,
}

impl MockProofProvider {
    pub fn new(delay_ms: u64) -> Self {
        Self { delay_ms }
    }
}

#[async_trait]
impl ProofProvider for MockProofProvider {
    async fn get_proof(
        &self,
        batch_id: &BatchId,
        _public_inputs: &[u8],
    ) -> Result<ProofResponse, DomainError> {
        info!("Mock proving for batch {} (delay: {}ms)", batch_id, self.delay_ms);
        // Simulate delay
        if self.delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.delay_ms)).await;
        }

        // Return valid 256-byte hex string (512 chars)
        // 8 * 32-byte elements: a[2], b[2][2], c[2]
        // Just using zeroes is fine for a mock
        let valid_proof = "00".repeat(256);

        Ok(ProofResponse {
            proof: valid_proof,
        })
    }
}
