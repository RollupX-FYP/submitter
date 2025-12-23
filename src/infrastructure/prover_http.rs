use crate::application::ports::{ProofProvider, ProofResponse};
use crate::domain::{batch::BatchId, errors::DomainError};
use async_trait::async_trait;
use backoff::{future::retry, ExponentialBackoff};
use metrics::{counter, histogram};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

pub struct HttpProofProvider {
    client: Client,
    url: String,
    circuit_state: Arc<Mutex<CircuitState>>,
    failure_count: Arc<Mutex<u32>>,
    failure_threshold: u32,
    last_failure: Arc<Mutex<std::time::Instant>>,
    backoff_settings: ExponentialBackoff,
}

impl HttpProofProvider {
    pub fn new(url: String, failure_threshold: u32) -> Self {
        Self {
            client: Client::new(),
            url,
            circuit_state: Arc::new(Mutex::new(CircuitState::Closed)),
            failure_count: Arc::new(Mutex::new(0)),
            failure_threshold,
            last_failure: Arc::new(Mutex::new(std::time::Instant::now())),
            backoff_settings: ExponentialBackoff::default(),
        }
    }

    // For testing purposes
    pub fn with_backoff(mut self, backoff: ExponentialBackoff) -> Self {
        self.backoff_settings = backoff;
        self
    }

    async fn check_circuit(&self) -> Result<(), DomainError> {
        let mut state = self.circuit_state.lock().await;
        match *state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => {
                let last = *self.last_failure.lock().await;
                if last.elapsed() > Duration::from_secs(30) {
                    *state = CircuitState::HalfOpen;
                    info!("Circuit Breaker HALF-OPEN");
                    Ok(())
                } else {
                    counter!("prover_circuit_open_hits_total").increment(1);
                    Err(DomainError::Prover("Circuit Breaker is OPEN".to_string()))
                }
            }
            CircuitState::HalfOpen => Ok(()),
        }
    }

    async fn record_success(&self) {
        let mut state = self.circuit_state.lock().await;
        if *state != CircuitState::Closed {
            info!("Circuit Breaker closed (recovered)");
            *state = CircuitState::Closed;
            *self.failure_count.lock().await = 0;
        }
    }

    async fn record_failure(&self) {
        let mut count = self.failure_count.lock().await;
        *count += 1;
        *self.last_failure.lock().await = std::time::Instant::now();

        if *count >= self.failure_threshold {
            let mut state = self.circuit_state.lock().await;
            *state = CircuitState::Open;
            warn!("Circuit Breaker tripped to OPEN");
            counter!("prover_circuit_tripped_total").increment(1);
        }
    }
}

#[async_trait]
impl ProofProvider for HttpProofProvider {
    async fn get_proof(
        &self,
        batch_id: &BatchId,
        public_inputs: &[u8],
    ) -> Result<ProofResponse, DomainError> {
        self.check_circuit().await?;

        let start = Instant::now();

        let operation = || async {
            let res = self
                .client
                .post(format!("{}/prove", self.url))
                .json(&serde_json::json!({
                    "batch_id": batch_id,
                    "public_inputs": public_inputs
                }))
                .send()
                .await
                .map_err(|e| backoff::Error::transient(DomainError::Prover(e.to_string())))?;

            if !res.status().is_success() {
                // If it's a 4xx error, maybe we shouldn't retry? But for this test we simulate 500.
                return Err(backoff::Error::transient(DomainError::Prover(format!(
                    "Status: {}",
                    res.status()
                ))));
            }

            let body: ProofResponse = res.json().await.map_err(|e| {
                backoff::Error::permanent(DomainError::Prover(format!("Parse error: {}", e)))
            })?;

            Ok(body)
        };

        // Clone settings for this run
        let backoff = self.backoff_settings.clone();

        match retry(backoff, operation).await {
            Ok(proof) => {
                self.record_success().await;
                histogram!("prover_request_duration_seconds").record(start.elapsed().as_secs_f64());
                counter!("prover_requests_total", "result" => "success").increment(1);
                Ok(proof)
            }
            Err(e) => {
                self.record_failure().await;
                counter!("prover_requests_total", "result" => "error").increment(1);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_circuit_breaker_trip() {
        let mock_server = MockServer::start().await;

        // Configure backoff to fail fast (1ms max elapsed time)
        let backoff = ExponentialBackoff {
            max_elapsed_time: Some(Duration::from_millis(1)),
            ..ExponentialBackoff::default()
        };

        let provider = HttpProofProvider::new(mock_server.uri(), 5).with_backoff(backoff);
        let id = BatchId::new();

        Mock::given(method("POST"))
            .and(path("/prove"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        // Trip the breaker (need 5 failures)
        for _ in 0..6 {
            let _ = provider.get_proof(&id, &[]).await;
        }

        // Verify state
        let state = *provider.circuit_state.lock().await;
        assert_eq!(state, CircuitState::Open);
    }

    #[tokio::test]
    async fn test_circuit_breaker_recovery() {
        let mock_server = MockServer::start().await;

        let backoff = ExponentialBackoff {
            max_elapsed_time: Some(Duration::from_millis(1)),
            ..ExponentialBackoff::default()
        };

        let provider = HttpProofProvider::new(mock_server.uri(), 5).with_backoff(backoff);
        let id = BatchId::new();

        // 1. Trip breaker
        Mock::given(method("POST"))
            .and(path("/prove"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        for _ in 0..5 {
            let _ = provider.get_proof(&id, &[]).await;
        }

        // 2. Force state to Open manually
        *provider.last_failure.lock().await = std::time::Instant::now() - Duration::from_secs(31);

        // 3. Next call should be HalfOpen allowed, succeed
        mock_server.reset().await;
        Mock::given(method("POST"))
            .and(path("/prove"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "proof": "valid"
            })))
            .mount(&mock_server)
            .await;

        let res = provider.get_proof(&id, &[]).await;
        assert!(res.is_ok());

        // 4. State should be Closed
        let state = *provider.circuit_state.lock().await;
        assert_eq!(state, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_custom_threshold() {
        let mock_server = MockServer::start().await;

        let backoff = ExponentialBackoff {
            max_elapsed_time: Some(Duration::from_millis(1)),
            ..ExponentialBackoff::default()
        };

        // Threshold = 2
        let provider = HttpProofProvider::new(mock_server.uri(), 2).with_backoff(backoff);
        let id = BatchId::new();

        Mock::given(method("POST"))
            .and(path("/prove"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        // 1. Fail once
        let _ = provider.get_proof(&id, &[]).await;
        {
            let state = *provider.circuit_state.lock().await;
            assert_eq!(state, CircuitState::Closed);
        }

        // 2. Fail twice (hits threshold)
        let _ = provider.get_proof(&id, &[]).await;
        {
            let state = *provider.circuit_state.lock().await;
            assert_eq!(state, CircuitState::Open);
        }
    }
}
