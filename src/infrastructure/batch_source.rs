use crate::proto::rollup::rollup_service_client::RollupServiceClient;
use crate::proto::rollup::BatchPayload;
use crate::proto::rollup::StreamRequest;
use anyhow::{Context, Result};
use async_trait::async_trait;
use ethers::types::{Bytes, H256};
use ethers::utils::hex;
use serde::Deserialize;
use std::path::PathBuf;
use tokio::time::Duration;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct FetchedBatch {
    pub batch_id: String,
    pub batch_data: Vec<u8>,
    pub pre_state_root: H256,
    pub post_state_root: H256,
    pub da_commitment: String,
    pub proof: Bytes,
    pub experiment_id: Option<String>,
}

#[async_trait]
pub trait BatchSource: Send {
    async fn next_batch(&mut self) -> Result<FetchedBatch>;
}

pub struct FileBatchSource {
    path: PathBuf,
    last_processed_id: String,
}

impl FileBatchSource {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            last_processed_id: String::new(),
        }
    }
}

#[derive(Deserialize)]
struct JsonBatchOutput {
    batch_id: String,
    batch_data: String,
    pre_state_root: String,
    post_state_root: String,
    da_commitment: String,
    proof: String,
    experiment_id: Option<String>,
}

#[async_trait]
impl BatchSource for FileBatchSource {
    async fn next_batch(&mut self) -> Result<FetchedBatch> {
        loop {
            if let Ok(content) = tokio::fs::read_to_string(&self.path).await {
                if let Ok(output) = serde_json::from_str::<JsonBatchOutput>(&content) {
                    if output.batch_id != self.last_processed_id {
                        // Parse fields
                        let batch_data = hex::decode(output.batch_data.trim_start_matches("0x"))?;
                        let pre_root = output.pre_state_root.parse()?;
                        let post_root = output.post_state_root.parse()?;
                        let proof =
                            Bytes::from(hex::decode(output.proof.trim_start_matches("0x"))?);

                        self.last_processed_id = output.batch_id.clone();

                        return Ok(FetchedBatch {
                            batch_id: output.batch_id,
                            batch_data,
                            pre_state_root: pre_root,
                            post_state_root: post_root,
                            da_commitment: output.da_commitment,
                            proof,
                            experiment_id: output.experiment_id,
                        });
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

pub struct GrpcBatchSource {
    client: Option<RollupServiceClient<tonic::transport::Channel>>,
    url: String,
    stream: Option<tonic::Streaming<BatchPayload>>,
}

impl GrpcBatchSource {
    pub fn new(url: String) -> Self {
        Self {
            client: None,
            url,
            stream: None,
        }
    }

    async fn connect(&mut self) -> Result<()> {
        info!("Connecting to Executor gRPC at {}...", self.url);
        let client = RollupServiceClient::connect(self.url.clone()).await?;
        self.client = Some(client);
        info!("Connected to Executor gRPC");
        Ok(())
    }

    async fn get_stream(&mut self) -> Result<()> {
        if self.client.is_none() {
            self.connect().await?;
        }

        if let Some(client) = &mut self.client {
            let req = tonic::Request::new(StreamRequest {});
            let stream = client.stream_batches(req).await?.into_inner();
            self.stream = Some(stream);
            Ok(())
        } else {
            anyhow::bail!("Client not connected")
        }
    }
}

fn bytes_to_h256(label: &str, b: &[u8], batch_id: &str) -> Option<H256> {
    if b.len() != 32 {
        error!(
            "{}: expected 32 bytes for state root, got {} bytes (batch_id={})",
            label,
            b.len(),
            batch_id
        );
        return None;
    }
    Some(H256::from_slice(b))
}

#[async_trait]
impl BatchSource for GrpcBatchSource {
    async fn next_batch(&mut self) -> Result<FetchedBatch> {
        loop {
            if self.stream.is_none() {
                if let Err(e) = self.get_stream().await {
                    error!(
                        "Failed to connect/stream from gRPC: {}. Retrying in 2s...",
                        e
                    );
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            }

            if let Some(stream) = &mut self.stream {
                match stream.message().await {
                    Ok(Some(payload)) => {
                        // Validate & Convert Payload
                        let pre_root = match bytes_to_h256(
                            "pre_state_root",
                            &payload.pre_state_root,
                            &payload.batch_id,
                        ) {
                            Some(r) => r,
                            None => continue,
                        };

                        let post_root = match bytes_to_h256(
                            "post_state_root",
                            &payload.post_state_root,
                            &payload.batch_id,
                        ) {
                            Some(r) => r,
                            None => continue,
                        };

                        let proof = Bytes::from(payload.proof);
                        let da_commitment = format!("0x{}", hex::encode(payload.da_commitment));
                        let experiment_id = if payload.experiment_id.is_empty() {
                            None
                        } else {
                            Some(payload.experiment_id)
                        };

                        return Ok(FetchedBatch {
                            batch_id: payload.batch_id,
                            batch_data: payload.batch_data,
                            pre_state_root: pre_root,
                            post_state_root: post_root,
                            da_commitment,
                            proof,
                            experiment_id,
                        });
                    }
                    Ok(None) => {
                        warn!("gRPC stream ended. Reconnecting...");
                        self.stream = None;
                        self.client = None;
                    }
                    Err(e) => {
                        error!("gRPC stream error: {}. Reconnecting...", e);
                        self.stream = None;
                        self.client = None;
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_h256_validation() {
        let valid = vec![0u8; 32];
        let short = vec![0u8; 31];
        let long = vec![0u8; 33];
        let empty = vec![];

        assert!(bytes_to_h256("valid", &valid, "test").is_some());
        assert!(bytes_to_h256("short", &short, "test").is_none());
        assert!(bytes_to_h256("long", &long, "test").is_none());
        assert!(bytes_to_h256("empty", &empty, "test").is_none());
    }
}
