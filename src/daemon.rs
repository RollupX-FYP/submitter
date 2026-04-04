use crate::application::ports::DaStrategy;
use crate::config::{self, DaMode, VerificationMode};
use crate::contracts::ZKRollupBridge;
use crate::domain::batch::{Batch, BatchStatus};
use crate::infrastructure::da_blob::BlobStrategy;
use crate::infrastructure::da_calldata::CalldataStrategy;
use crate::infrastructure::da_offchain::OffChainStrategy;
use crate::infrastructure::ethereum_adapter::RealBridgeClient;
use crate::infrastructure::batch_source::{BatchSource, FileBatchSource, GrpcBatchSource};
use crate::saga::{SagaOutbox, SagaState, BatchSagaRecord};
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

    // 4. Initialize Saga Outbox
    let outbox_path = std::env::var("OUTBOX_DB_PATH").unwrap_or_else(|_| "outbox.db".to_string());
    let outbox = SagaOutbox::new(&outbox_path)?;

    info!("Submitter Daemon started");

    // 5. Recovery Step: Scan for stuck/unconfirmed batches
    // GAP 2: Submitter Daemon Auto-Resume / Crash Recovery Logic.
    // Fixed / IN-SCOPE: This loop specifically evaluates SagaOutbox upon daemon startup and
    // handles the resumption logic for any batches that were interrupted mid-flight before the poll loop begins.
    let mut batches_to_resume = Vec::new();
    match outbox.get_unconfirmed_batches() {
        Ok(unconfirmed) => {
            if !unconfirmed.is_empty() {
                info!("Recovered {} unconfirmed batches from outbox", unconfirmed.len());
                for record in unconfirmed {
                    if record.state == SagaState::SubmittedToL1 {
                        let now = chrono::Utc::now().timestamp_millis();
                        if now - record.last_updated > 300_000 { // 5 minutes timeout
                            info!("Batch {} is stuck in SUBMITTED_TO_L1. Triggering gas bump.", record.batch_id);
                            
                            if let Some(tx_hash_str) = &record.tx_hash {
                                if let Ok(tx_hash) = tx_hash_str.parse::<H256>() {
                                    if let Ok(Some(mut tx)) = client.get_transaction(tx_hash).await {
                                        // Ensure we retain the same nonce
                                        if let Some(n) = record.nonce {
                                            tx.nonce = n.into();
                                        }
                                        
                                        // Calculate new gas price: gas_price * 1.2, capped at 3x original
                                        if let Some(current_gas_price) = tx.gas_price {
                                            let bump = current_gas_price / 5; // 20% bump
                                            let mut new_gas_price = current_gas_price + bump;
                                            
                                            // Enforce 3x cap based on original_gas_price
                                            if let Some(orig_gp_str) = &record.original_gas_price {
                                                if let Ok(orig_gp) = U256::from_dec_str(orig_gp_str) {
                                                    let max_gas_price = orig_gp * 3;
                                                    if new_gas_price > max_gas_price {
                                                        info!("Gas bump capped at 3x original (max: {}) for batch {}", max_gas_price, record.batch_id);
                                                        new_gas_price = max_gas_price;
                                                        
                                                        // Stop retrying if we hit the cap to prevent infinite loop / bleeding funds
                                                        if current_gas_price >= max_gas_price * 99 / 100 {
                                                            error!("Batch {} is chronically stuck. Gas price capped at {} and still not confirmed. Transitioning to FAILED state.", record.batch_id, max_gas_price);
                                                            let _ = outbox.update_state(&record.batch_id, SagaState::Failed);
                                                            continue;
                                                        }
                                                    }
                                                }
                                            }
                                            tx.gas_price = Some(new_gas_price);
                                        }

                                        let typed_tx: ethers::types::transaction::eip2718::TypedTransaction = (&tx).into();
                                        
                                        match client.send_transaction(typed_tx, None).await {
                                            Ok(pending_tx) => {
                                                let new_hash = format!("{:?}", pending_tx.tx_hash());
                                                info!("Gas bump successful for batch {}. New Tx Hash: {}", record.batch_id, new_hash);
                                                let _ = outbox.update_submission(&record.batch_id, &new_hash, record.nonce, None);
                                            },
                                            Err(e) => error!("Failed to broadcast bumped tx for batch {}: {}", record.batch_id, e),
                                        }
                                    } else {
                                        warn!("Original tx {} not found on L1 to bump gas", tx_hash_str);
                                    }
                                }
                            }
                        }
                    } else if record.state == SagaState::ReceivedFromExecutor || record.state == SagaState::Compressed {
                        if let (Some(data_json), Some(proof)) = (record.batch_data.clone(), record.proof_hex.clone()) {
                            if let Ok(batch) = serde_json::from_str::<Batch>(&data_json) {
                                info!("Queueing batch {} for submission recovery from state {:?}", record.batch_id, record.state);
                                batches_to_resume.push((batch, proof));
                            } else {
                                warn!("Failed to deserialize batch_data from outbox for batch {}", record.batch_id);
                            }
                        } else {
                            warn!("Batch {} in state {:?} is missing data/proof in outbox to resume", record.batch_id, record.state);
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!("Failed to query outbox for recovery: {}", e);
        }
    }

    // Process resumed batches before normal polling
    for (batch, proof_hex) in batches_to_resume {
        let verifier_id = 0; // Defaulting for recovery
        match da_strategy.submit(&batch, &proof_hex, verifier_id).await {
            Ok(result) => {
                info!("Recovered batch submitted! Tx Hash: {}", result.tx_hash);
                
                // For recovery submission, fetch original gas price and nonce to initialize it
                let mut orig_gp = None;
                let mut orig_nonce = None;
                if let Ok(tx_hash) = result.tx_hash.parse::<H256>() {
                    if let Ok(Some(tx)) = client.get_transaction(tx_hash).await {
                        if let Some(gp) = tx.gas_price {
                            orig_gp = Some(gp.to_string());
                        }
                        orig_nonce = Some(tx.nonce.low_u64() as i64);
                    }
                }

                if let Err(e) = outbox.update_submission(&batch.id.to_string(), &result.tx_hash, orig_nonce, orig_gp.as_deref()) {
                    error!("Failed to update state to SUBMITTED_TO_L1: {}", e);
                }
                if let Err(e) = outbox.update_state(&batch.id.to_string(), SagaState::ConfirmedOnL1) {
                     error!("Failed to update state to CONFIRMED_ON_L1: {}", e);
                }
            }
            Err(e) => error!("Failed to submit recovered batch: {}", e),
        }
    }

    info!("Waiting for batches...");

    loop {
        match batch_source.next_batch().await {
            Ok(fetched) => {
                // Deduplication check: if we already have this batch, skip it.
                if let Ok(Some(_)) = outbox.get_record(&fetched.batch_id) {
                    warn!("Skipping already known batch_id: {}", fetched.batch_id);
                    continue;
                }

                let tx_count = serde_json::from_slice::<Vec<serde_json::Value>>(&fetched.batch_data)
                    .map(|v| v.len())
                    .unwrap_or(0) as u32;

                // Construct Batch object
                let batch = Batch {
                    id: crate::domain::batch::BatchId::new(),
                    data_file: format!("batch_{}.bin", fetched.batch_id),
                    new_root: format!("{:?}", fetched.post_state_root),
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
                    tx_count,
                };

                let batch_data_json = serde_json::to_string(&batch).unwrap_or_else(|_| "{}".to_string());
                let proof_hex_init = format!("0x{}", hex::encode(&fetched.proof));

                // Insert into outbox with state RECEIVED_FROM_EXECUTOR
                match outbox.insert_or_ignore(&fetched.batch_id, &batch_data_json, &proof_hex_init) {
                    Ok(true) => {
                        info!("Found new batch: {}", fetched.batch_id);
                        let timestamp_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
                        let experiment_id = std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default".to_string());
                        tracing::info!(
                            batch_id = %fetched.batch_id,
                            from_state = "NEW",
                            to_state = "RECEIVED_FROM_EXECUTOR",
                            timestamp_ms = %timestamp_ms,
                            experiment_id = %experiment_id,
                            "State transition"
                        );
                    }
                    Ok(false) => {
                        warn!("Batch {} already exists in outbox, skipping processing to prevent duplicate broadcast", fetched.batch_id);
                        continue;
                    }
                    Err(e) => {
                        error!("Failed to write batch to outbox: {}", e);
                        continue;
                    }
                }
                info!("  DA Commitment: {}", fetched.da_commitment);
                info!("  Old Root: {:?}", fetched.pre_state_root);
                info!("  New Root: {:?}", fetched.post_state_root);

                // Write to temp file for DA strategy
                let data_file = format!("batch_{}.bin", fetched.batch_id);
                if let Err(e) = tokio::fs::write(&data_file, &fetched.batch_data).await {
                     error!("Failed to write batch data file: {}", e);
                     continue;
                }

                // Advance state to COMPRESSED (Simulation of the DA strategy's compression step)
                if let Err(e) = outbox.update_state(&fetched.batch_id, SagaState::Compressed) {
                     error!("Failed to update state to COMPRESSED: {}", e);
                } else {
                     let timestamp_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
                     let experiment_id = std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default".to_string());
                     tracing::info!(
                         batch_id = %fetched.batch_id,
                         from_state = "RECEIVED_FROM_EXECUTOR",
                         to_state = "COMPRESSED",
                         timestamp_ms = %timestamp_ms,
                         experiment_id = %experiment_id,
                         "State transition"
                     );
                }


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

                // Proof handling based on Verifier ID
                let proof_bytes = crate::domain::proof::format_proof_for_verifier(fetched.proof.to_vec(), verifier_id);

                let proof_hex = format!("0x{}", hex::encode(&proof_bytes));
                
                info!("Submitting batch: verifier_id={}, proof_len={}, da_mode={:?}", verifier_id, proof_bytes.len(), cfg.da.mode);
                
                let start_submit = std::time::Instant::now();

                if verification_mode == VerificationMode::OffChainOnly {
                    info!("Verification Mode: OffChainOnly. Skipping on-chain submission.");
                    
                    // Update state to SUBMITTED_TO_L1
                    if let Err(e) = outbox.update_submission(&fetched.batch_id, "0x_offchain_simulated", None, Some("0")) {
                        error!("Failed to update state to SUBMITTED_TO_L1: {}", e);
                    } else {
                         let timestamp_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
                         let experiment_id = std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default".to_string());
                         tracing::info!(
                             batch_id = %fetched.batch_id,
                             from_state = "COMPRESSED",
                             to_state = "SUBMITTED_TO_L1",
                             timestamp_ms = %timestamp_ms,
                             experiment_id = %experiment_id,
                             "State transition"
                         );
                    }

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
                    
                    if let Ok(Some(record)) = outbox.get_record(&fetched.batch_id) {
                        if let Some(data_json) = record.batch_data {
                            if let Ok(batch_record) = serde_json::from_str::<Batch>(&data_json) {
                                let experiment_id = std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default".to_string());
                                let batch_size = batch_record.tx_count;
                                let current_time_ms = chrono::Utc::now().timestamp_millis() as u64;
                                let created_at_ms = batch_record.created_at.timestamp_millis() as u64;
                                let e2e_latency_ms = current_time_ms.saturating_sub(created_at_ms);
                                let relay_latency_ms = current_time_ms.saturating_sub(record.last_updated as u64);
                                let gas_used = "N/A".to_string();
                                let da_mode = format!("{:?}", cfg.da.mode);

                                let csv_path = cfg.csv_output_path.as_deref().unwrap_or("results/metrics.csv");
                                match std::fs::OpenOptions::new().append(true).create(true).open(csv_path) {
                                    Ok(mut file) => {
                                        if let Ok(metadata) = file.metadata() {
                                            if metadata.len() == 0 {
                                                let _ = writeln!(file, "experiment_id,batch_id,batch_size,relay_latency_ms,e2e_latency_ms,da_mode,gas_used");
                                            }
                                        }
                                        let _ = writeln!(file, "{},{},{},{},{},{},{}", experiment_id, fetched.batch_id, batch_size, relay_latency_ms, e2e_latency_ms, da_mode, gas_used);
                                    }
                                    Err(e) => {
                                        warn!("Failed to open CSV metrics file: {}", e);
                                    }
                                }
                            }
                        }
                    }

                    // Advance state to CONFIRMED_ON_L1
                    if let Err(e) = outbox.update_state(&fetched.batch_id, SagaState::ConfirmedOnL1) {
                         error!("Failed to update state to CONFIRMED_ON_L1: {}", e);
                    } else {
                         let timestamp_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
                         let experiment_id = std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default".to_string());
                         tracing::info!(
                             batch_id = %fetched.batch_id,
                             from_state = "SUBMITTED_TO_L1",
                             to_state = "CONFIRMED_ON_L1",
                             timestamp_ms = %timestamp_ms,
                             experiment_id = %experiment_id,
                             "State transition"
                         );
                    }

                    // Cleanup
                    let _ = tokio::fs::remove_file(data_file).await;
                    continue;
                }

                match da_strategy.submit(&batch, &proof_hex, verifier_id).await {
                    Ok(result) => {
                        // Fetch the transaction to get its initial gas price and nonce
                        let mut orig_gp = None;
                        let mut orig_nonce = None;
                        if let Ok(tx_hash) = result.tx_hash.parse::<H256>() {
                            if let Ok(Some(tx)) = client.get_transaction(tx_hash).await {
                                if let Some(gp) = tx.gas_price {
                                    orig_gp = Some(gp.to_string());
                                }
                                orig_nonce = Some(tx.nonce.low_u64() as i64);
                            }
                        }

                        // Update state to SUBMITTED_TO_L1
                        if let Err(e) = outbox.update_submission(&fetched.batch_id, &result.tx_hash, orig_nonce, orig_gp.as_deref()) {
                            error!("Failed to update state to SUBMITTED_TO_L1: {}", e);
                        } else {
                             let timestamp_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
                             let experiment_id = std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default".to_string());
                             tracing::info!(
                                 batch_id = %fetched.batch_id,
                                 from_state = "COMPRESSED",
                                 to_state = "SUBMITTED_TO_L1",
                                 timestamp_ms = %timestamp_ms,
                                 experiment_id = %experiment_id,
                                 "State transition"
                             );
                        }

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
                        
                        if let Ok(Some(record)) = outbox.get_record(&fetched.batch_id) {
                            if let Some(data_json) = record.batch_data {
                                if let Ok(batch_record) = serde_json::from_str::<Batch>(&data_json) {
                                    let experiment_id = std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default".to_string());
                                    let batch_size = batch_record.tx_count;
                                    let current_time_ms = chrono::Utc::now().timestamp_millis() as u64;
                                    let created_at_ms = batch_record.created_at.timestamp_millis() as u64;
                                    let e2e_latency_ms = current_time_ms.saturating_sub(created_at_ms);
                                    let relay_latency_ms = current_time_ms.saturating_sub(record.last_updated as u64);
                                    let gas_used = result.gas_used.map(|g| g.to_string()).unwrap_or_else(|| "N/A".to_string());
                                    let da_mode = format!("{:?}", cfg.da.mode);

                                    let csv_path = cfg.csv_output_path.as_deref().unwrap_or("results/metrics.csv");
                                    match std::fs::OpenOptions::new().append(true).create(true).open(csv_path) {
                                        Ok(mut file) => {
                                            if let Ok(metadata) = file.metadata() {
                                                if metadata.len() == 0 {
                                                    let _ = writeln!(file, "experiment_id,batch_id,batch_size,relay_latency_ms,e2e_latency_ms,da_mode,gas_used");
                                                }
                                            }
                                            let _ = writeln!(file, "{},{},{},{},{},{},{}", experiment_id, fetched.batch_id, batch_size, relay_latency_ms, e2e_latency_ms, da_mode, gas_used);
                                        }
                                        Err(e) => {
                                            warn!("Failed to open CSV metrics file: {}", e);
                                        }
                                    }
                                }
                            }
                        }

                        // Advance state to CONFIRMED_ON_L1
                        if let Err(e) = outbox.update_state(&fetched.batch_id, SagaState::ConfirmedOnL1) {
                             error!("Failed to update state to CONFIRMED_ON_L1: {}", e);
                        } else {
                             let timestamp_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
                             let experiment_id = std::env::var("EXPERIMENT_ID").unwrap_or_else(|_| "default".to_string());
                             tracing::info!(
                                 batch_id = %fetched.batch_id,
                                 from_state = "SUBMITTED_TO_L1",
                                 to_state = "CONFIRMED_ON_L1",
                                 timestamp_ms = %timestamp_ms,
                                 experiment_id = %experiment_id,
                                 "State transition"
                             );
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
