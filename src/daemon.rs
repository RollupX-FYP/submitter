use crate::application::orchestrator::Orchestrator;
use crate::application::ports::DaStrategy;
use crate::config::{self, DaMode};
use crate::contracts::ZKRollupBridge;
use crate::infrastructure::da_blob::BlobStrategy;
use crate::infrastructure::da_calldata::CalldataStrategy;
use crate::infrastructure::ethereum_adapter::RealBridgeClient;
use crate::infrastructure::prover_mock::MockProofProvider;
use crate::infrastructure::storage_postgres::PostgresStorage;
use anyhow::{Context, Result};
use ethers::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;

pub async fn run(config_path: PathBuf) -> Result<()> {
    let cfg = config::load_config(config_path)?;

    // 1. Setup Storage
    let db_url = std::env::var("DATABASE_URL")
        .context("Missing env DATABASE_URL for daemon mode")?;
    
    let batch_size = cfg.sequencer.as_ref().and_then(|s| s.batch_size);
    let ordering = cfg.sequencer.as_ref().and_then(|s| s.ordering_policy.clone());

    let storage = Arc::new(PostgresStorage::new(&db_url, batch_size, ordering).await?);

    // 2. Setup Ethereum Client
    let pk = std::env::var("SUBMITTER_PRIVATE_KEY")
        .context("Missing env SUBMITTER_PRIVATE_KEY")?;
    let wallet: LocalWallet = pk
        .parse::<LocalWallet>()?
        .with_chain_id(cfg.network.chain_id);

    let provider = Provider::<Http>::try_from(cfg.network.rpc_url.as_str())?;
    let client = Arc::new(SignerMiddleware::new(provider, wallet));

    let bridge_addr: Address = cfg.contracts.bridge.parse()?;
    let bridge = ZKRollupBridge::new(bridge_addr, client.clone());

    // 3. Setup Bridge Client (Reader + Submitter)
    let bridge_client = Arc::new(RealBridgeClient::new(bridge.clone()));

    // 4. Setup Prover
    let delay = cfg.simulation.as_ref().and_then(|s| s.mock_proving_time_ms).unwrap_or(0);
    let prover = Arc::new(MockProofProvider::new(delay));

    // 5. Setup DA Strategy
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
        }
    };

    // 6. Create Orchestrator
    let max_attempts = cfg.resilience.as_ref().and_then(|r| r.max_retries).unwrap_or(5);
    
    let orchestrator = Orchestrator::new(
        storage,
        prover,
        da_strategy,
        bridge_client, // as BridgeReader
        max_attempts,
    );

    // 7. Run
    orchestrator.run().await.map_err(|e| anyhow::anyhow!(e))
}
