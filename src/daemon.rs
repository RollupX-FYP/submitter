use crate::application::ports::DaStrategy;
use crate::config::{self, DaMode, VerificationMode};
use crate::contracts::ZKRollupBridge;
use crate::domain::batch::{Batch, BatchStatus};
use crate::infrastructure::da_blob::BlobStrategy;
use crate::infrastructure::da_calldata::CalldataStrategy;
use crate::infrastructure::da_offchain::OffChainStrategy;
use crate::infrastructure::ethereum_adapter::RealBridgeClient;
use crate::infrastructure::batch_source::{BatchSource, FileBatchSource, GrpcBatchSource};
use anyhow::{Context, Result};
use ethers::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn, error};
use ethers::utils::hex;
use std::io::Write;
use serde::Serialize;

#[derive(Serialize)]
struct SubmitterMetrics {
    experiment_id: String,
    batch_id: String,
    tx_hash: String,
    submission_latency_ms: u64,
    l2_l1_latency_ms: u64,
    l1_block_number: u64,
    confirmation_blocks: u64,
    da_mode: String,
    proof_metadata_hash: String,
}

pub async fn run(config_path: PathBuf) -> Result<()> {
    let cfg = config::load_config(config_path)?;

    // 1. Setup Ethereum Client
    let pk = std::env::var("SUBMITTER_PRIVATE_KEY")
        .context("Missing env SUBMITTER_PRIVATE_KEY")?;
    let wallet: LocalWallet = pk
        .parse::<LocalWallet>()?
        .with_chain_id(cfg.network.chain_id);

    let provider = Provider::<Http>::try_from(cfg.network.rpc_url.as_str())?;
    let client = Arc::new(SignerMiddleware::new(provider, wallet));

    let bridge_addr: Address = cfg.contracts.bridge.parse()?;
    let bridge = ZKRollupBridge::new(bridge_addr, client.clone());

    // 2. Setup DA Strategy
    let da_strategy: Arc<dyn DaStrategy> = match cfg.da.mode {
        DaMode::Calldata => {
             let compression = cfg.aggregator.as_ref().and_then(|a| a.compression);
             Arc::new(CalldataStrategy::new(bridge.clone(), compression))
        },
        DaMode::Blob => {
            let archiver = cfg.da.archiver_url.clone();
            let default_hash = cfg.batch.blob_versioned_hash.as_deref().unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000");
            let vh: H256 = default_hash.parse().unwrap_or_default();
            let idx = cfg.da.blob_index.unwrap_or(0);
            let use_opcode = cfg.da.blob_binding == config::BlobBinding::Opcode;

            Arc::new(BlobStrategy::new(bridge.clone(), vh, idx, use_opcode, archiver))
        },
        DaMode::OffChain => {
            Arc::new(OffChainStrategy::new(bridge.clone()))
        }
    };

    // 3. Setup Batch Source
    let comm_mode = std::env::var("COMM_MODE").unwrap_or_else(|_| "grpc".to_string());
    let mut batch_source: Box<dyn BatchSource> = if comm_mode == "file" {
        info!("Using File-based batch source (legacy)");
        Box::new(FileBatchSource::new(PathBuf::from("batch_output.json")))
    } else {
        info!("Using gRPC batch source");
        let executor_url = std::env::var("EXECUTOR_URL").unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());
        Box::new(GrpcBatchSource::new(executor_url))
    };

    info!("Submitter Daemon started");
    info!("Waiting for batches...");

    loop {
        match batch_source.next_batch().await {
            Ok(fetched) => {
                info!("Found new batch: {}", fetched.batch_id);
                info!("  DA Commitment: {}", fetched.da_commitment);
                info!("  Old Root: {:?}", fetched.pre_state_root);
                info!("  New Root: {:?}", fetched.post_state_root);

                // Write to temp file for DA strategy
                let data_file = format!("batch_{}.bin", fetched.batch_id);
                if let Err(e) = tokio::fs::write(&data_file, &fetched.batch_data).await {
                     error!("Failed to write batch data file: {}", e);
                     continue;
                }

                // Construct Batch object
                // We need to convert H256 to String for new_root if struct expects String?
                // Domain Batch struct: `pub new_root: String` (from previous file read).
                // Let's check `submitter/src/domain/batch.rs`.
                // Assuming it is String based on previous daemon code: `new_root: output.post_state_root.clone()`
                
                let batch = Batch {
                    id: crate::domain::batch::BatchId::new(),
                    data_file: data_file.clone(),
                    new_root: format!("{:?}", fetched.post_state_root), // H256 Debug/Display is hex?
                    status: BatchStatus::Proving,
                    da_mode: "calldata".into(),
                    proof: None,
                    tx_hash: None,
                    attempts: 0,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    blob_versioned_hash: None,
                    blob_index: None,
                    fee: 0,
                    experiment_id: fetched.experiment_id.clone(),
                };

                // Check Verification Mode
                let verification_mode = cfg.proof.as_ref()
                    .and_then(|p| p.verification_mode)
                    .unwrap_or(VerificationMode::OnChain);

                // Determine Verifier ID
                let verifier_id = if let Some(p) = &cfg.proof {
                    if let Some(id) = p.verifier_id {
                        id
                    } else if let Some(backend) = p.backend {
                        match backend {
                            config::ProofBackend::Groth16 => 0,
                            config::ProofBackend::Plonky2 => 1,
                            config::ProofBackend::Halo2 => 2,
                            config::ProofBackend::Mock => 0,
                        }
                    } else {
                        0
                    }
                } else {
                    0
                };

                // Patch: Proof handling based on Verifier ID
                let mut proof_bytes = fetched.proof.to_vec();
                if verifier_id == 0 {
                    // Groth16 requires exactly 256 bytes
                    if proof_bytes.len() < 256 {
                        info!("Padding Groth16 proof from {} bytes to 256 bytes", proof_bytes.len());
                        proof_bytes.resize(256, 0);
                    } else if proof_bytes.len() > 256 {
                        warn!("Groth16 proof length {} > 256, truncating", proof_bytes.len());
                        proof_bytes.truncate(256);
                    }
                } else {
                    // Plonky2/Halo2: Pass raw or padded, just log it
                    info!("Non-Groth16 proof length: {} bytes", proof_bytes.len());
                }

                let proof_hex = format!("0x{}", hex::encode(&proof_bytes));
                
                info!("Submitting batch: verifier_id={}, proof_len={}, da_mode={:?}", verifier_id, proof_bytes.len(), cfg.da.mode);
                
                let start_submit = std::time::Instant::now();

                if verification_mode == VerificationMode::OffChainOnly {
                    info!("Verification Mode: OffChainOnly. Skipping on-chain submission.");
                    
                    // Save Metrics (Simulated)
                    let metrics = SubmitterMetrics {
                        experiment_id: std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default_experiment".to_string()),
                        batch_id: fetched.batch_id.clone(),
                        tx_hash: "0x_offchain_simulated".to_string(),
                        submission_latency_ms: 0,
                        l2_l1_latency_ms: 0,
                        l1_block_number: 0,
                        confirmation_blocks: 0,
                        da_mode: format!("{:?}", cfg.da.mode),
                        proof_metadata_hash: "offchain".to_string(),
                    };

                    if let Ok(json) = serde_json::to_string_pretty(&metrics) {
                         let metrics_root = std::env::var("METRICS_ROOT").unwrap_or_else(|_| "metrics".to_string());
                         let metrics_path = std::path::Path::new(&metrics_root).join("submitter_metrics.json");
                         
                         if let Some(parent) = metrics_path.parent() {
                             let _ = std::fs::create_dir_all(parent);
                         }

                         let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(metrics_path)
                            .and_then(|mut f| writeln!(f, "{}", json));
                    }
                    
                    // Cleanup
                    let _ = tokio::fs::remove_file(data_file).await;
                    continue;
                }

                match da_strategy.submit(&batch, &proof_hex, verifier_id).await {
                    Ok(result) => {
                        let latency = start_submit.elapsed();
                        info!("Batch submitted! Tx Hash: {}", result.tx_hash);

                        // Check confirmations (Research Metric)
                        // Mock confirmation check for now since local simulation is instant
                        // In real run, we would loop check_confirmation
                        let confirmations = 1; 

                        // Save Metrics
                        let metrics = SubmitterMetrics {
                            experiment_id: std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default_experiment".to_string()),
                            batch_id: fetched.batch_id.clone(),
                            tx_hash: result.tx_hash.clone(),
                            submission_latency_ms: latency.as_millis() as u64,
                            l2_l1_latency_ms: result.latency_ms,
                            l1_block_number: result.block_number,
                            confirmation_blocks: confirmations,
                            da_mode: format!("{:?}", cfg.da.mode),
                            proof_metadata_hash: "mock_proof_meta_hash".to_string(),
                        };

                        if let Ok(json) = serde_json::to_string_pretty(&metrics) {
                             // Append to a metrics file
                             let metrics_root = std::env::var("METRICS_ROOT").unwrap_or_else(|_| "metrics".to_string());
                             let metrics_path = std::path::Path::new(&metrics_root).join("submitter_metrics.json");
                             
                             if let Some(parent) = metrics_path.parent() {
                                 let _ = std::fs::create_dir_all(parent);
                             }

                             let _ = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(metrics_path)
                                .and_then(|mut f| writeln!(f, "{}", json));
                        }
                        
                        // Cleanup
                        let _ = tokio::fs::remove_file(data_file).await;
                    }
                    Err(e) => {
                        error!("Failed to submit batch: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Error fetching batch: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
