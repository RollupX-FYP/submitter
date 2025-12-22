use anyhow::{Context, Result};
use clap::Parser;
use dotenvy::dotenv;
use ethers::prelude::*;
use std::{path::PathBuf, sync::Arc};
use tracing::info;
use sha1_smol::Sha1;
use std::fs;

// Import from the library crate
use submitter_rs::{
    config::{self, DaMode},
    contracts::ZKRollupBridge,
    domain::batch::Batch,
    application::{
        orchestrator::Orchestrator,
        ports::{DaStrategy, ProofProvider, Storage},
    },
    infrastructure::{
        da_blob::BlobStrategy,
        da_calldata::CalldataStrategy,
        observability,
        prover_mock::MockProofProvider,
        prover_http::HttpProofProvider,
        storage_sqlite::SqliteStorage,
        storage_postgres::PostgresStorage,
    },
};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    
    // 1. Observability
    observability::init_tracing();
    let metrics_handle = observability::init_metrics();
    tokio::spawn(observability::start_metrics_server(metrics_handle));

    let args = Args::parse();
    let cfg = config::load_config(args.config)?;

    // 2. Setup Ethereum Client
    let pk = std::env::var("SUBMITTER_PRIVATE_KEY")
        .context("Missing env SUBMITTER_PRIVATE_KEY (DO NOT put private keys in yaml)")?;
    let wallet: LocalWallet = pk.parse::<LocalWallet>()?.with_chain_id(cfg.network.chain_id);
    let provider = Provider::<Http>::try_from(cfg.network.rpc_url.as_str())?;
    let client = Arc::new(SignerMiddleware::new(provider, wallet));
    let bridge_addr: Address = cfg.contracts.bridge.parse()?;
    let bridge = ZKRollupBridge::new(bridge_addr, client.clone());

    // 3. Setup Dependencies
    // Select storage based on env var, default to sqlite
    let storage: Arc<dyn Storage> = if let Ok(pg_url) = std::env::var("DATABASE_URL") {
        if pg_url.starts_with("postgres") {
             Arc::new(PostgresStorage::new(&pg_url).await?)
        } else {
             Arc::new(SqliteStorage::new("sqlite:submitter.db").await?)
        }
    } else {
        Arc::new(SqliteStorage::new("sqlite:submitter.db").await?)
    };

    // Select prover based on config
    let prover: Arc<dyn ProofProvider> = if let Some(prover_cfg) = &cfg.prover {
        info!("Using HTTP Prover at {}", prover_cfg.url);
        Arc::new(HttpProofProvider::new(prover_cfg.url.clone()))
    } else {
        info!("Using Mock Prover");
        Arc::new(MockProofProvider)
    };

    // 4. DA Strategy selection
    let da_strategy: Arc<dyn DaStrategy> = match cfg.da.mode {
        DaMode::Calldata => Arc::new(CalldataStrategy::new(bridge)),
        DaMode::Blob => {
             let vh = cfg.batch.blob_versioned_hash.context("blob mode needs batch.blob_versioned_hash")?;
             let expected: H256 = vh.parse()?;
             let blob_index = cfg.da.blob_index.unwrap_or(0);
             let use_opcode = cfg.da.blob_binding == config::BlobBinding::Opcode;
             
             Arc::new(BlobStrategy::new(bridge, expected, blob_index, use_opcode))
        }
    };

    // 5. Initial Seed (For demo/testing purposes)
    let pending = storage.get_pending_batches().await?;
    if pending.is_empty() {
        info!("Seeding initial batch from config");
        
        // Calculate hash of data file for idempotency
        let data_bytes = fs::read(&cfg.batch.data_file)
            .context(format!("Failed to read data file {}", cfg.batch.data_file))?;
        let data_hash = Sha1::from(data_bytes).digest().to_string();

        let batch = Batch::new(
            cfg.network.chain_id,
            &cfg.contracts.bridge,
            cfg.batch.data_file.clone(),
            data_hash,
            cfg.batch.new_root.clone(),
            format!("{:?}", cfg.da.mode),
        );
        storage.save_batch(&batch).await?;
    }

    // 6. Start Orchestrator
    let orchestrator = Orchestrator::new(storage, prover, da_strategy);
    
    // Handle graceful shutdown
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    
    tokio::select! {
        _ = orchestrator.run() => {},
        _ = tokio::signal::ctrl_c() => { info!("Ctrl-C received, shutting down"); },
        _ = sigterm.recv() => { info!("SIGTERM received, shutting down"); },
    }

    Ok(())
}
