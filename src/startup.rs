use crate::{
    application::{
        orchestrator::Orchestrator,
        ports::{DaStrategy, ProofProvider, Storage},
    },
    config::{self, DaMode},
    contracts::ZKRollupBridge,
    domain::batch::Batch,
    infrastructure::{
        da_blob::BlobStrategy, da_calldata::CalldataStrategy,
        prover_http::HttpProofProvider, prover_mock::MockProofProvider,
        storage_postgres::PostgresStorage, storage_sqlite::SqliteStorage,
    },
};
use anyhow::{Context, Result};
use ethers::prelude::*;
use sha1_smol::Sha1;
use std::{fs, path::PathBuf, sync::Arc};
use tracing::info;

pub type AppStorage = Arc<dyn Storage>;
pub type AppOrchestrator = Orchestrator;

pub async fn build(config_path: PathBuf) -> Result<(AppStorage, AppOrchestrator)> {
    let cfg = config::load_config(config_path)?;

    let pk = std::env::var("SUBMITTER_PRIVATE_KEY")
        .context("Missing env SUBMITTER_PRIVATE_KEY (DO NOT put private keys in yaml)")?;
    let wallet: LocalWallet = pk
        .parse::<LocalWallet>()?
        .with_chain_id(cfg.network.chain_id);
    let provider = Provider::<Http>::try_from(cfg.network.rpc_url.as_str())?;
    let client = Arc::new(SignerMiddleware::new(provider, wallet));
    let bridge_addr: Address = cfg.contracts.bridge.parse()?;
    let bridge = ZKRollupBridge::new(bridge_addr, client.clone());

    let storage: Arc<dyn Storage> = if let Ok(pg_url) = std::env::var("DATABASE_URL") {
        if pg_url.starts_with("postgres") {
            Arc::new(PostgresStorage::new(&pg_url).await?)
        } else {
            Arc::new(SqliteStorage::new(&pg_url).await?)
        }
    } else {
        Arc::new(SqliteStorage::new("sqlite:submitter.db").await?)
    };

    let prover: Arc<dyn ProofProvider> = if let Some(prover_cfg) = &cfg.prover {
        info!("Using HTTP Prover at {}", prover_cfg.url);
        Arc::new(HttpProofProvider::new(prover_cfg.url.clone()))
    } else {
        info!("Using Mock Prover");
        Arc::new(MockProofProvider)
    };

    let da_strategy: Arc<dyn DaStrategy> = match cfg.da.mode {
        DaMode::Calldata => Arc::new(CalldataStrategy::new(bridge)),
        DaMode::Blob => {
            let vh = cfg
                .batch
                .blob_versioned_hash
                .clone()
                .context("blob mode needs batch.blob_versioned_hash")?;
            let expected: H256 = vh.parse()?;
            let blob_index = cfg.da.blob_index.unwrap_or(0);
            let use_opcode = cfg.da.blob_binding == config::BlobBinding::Opcode;

            Arc::new(BlobStrategy::new(bridge, expected, blob_index, use_opcode))
        }
    };

    let pending = storage.get_pending_batches().await?;
    if pending.is_empty() {
        info!("Seeding initial batch from config");

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

    let orchestrator = Orchestrator::new(storage.clone(), prover, da_strategy);
    Ok((storage, orchestrator))
}

use std::future::Future;

pub async fn run(config_path: PathBuf, shutdown: impl Future<Output = ()> + Send + 'static) -> Result<()> {
    let (_, orchestrator) = build(config_path).await?;

    tokio::select! {
        _ = orchestrator.run() => {},
        _ = shutdown => { info!("Shutdown signal received"); },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_startup_missing_env() {
        let mut config_file = NamedTempFile::new().unwrap();
        write!(config_file, "
network:
  rpc_url: http://localhost:8545
  chain_id: 1337
contracts:
  bridge: '0x0000000000000000000000000000000000000000'
batch:
  data_file: 'data.txt'
  new_root: '0x0000000000000000000000000000000000000000000000000000000000000000'
  blob_versioned_hash: '0x0000000000000000000000000000000000000000000000000000000000000000'
da:
  mode: calldata
  blob_binding: opcode
        ").unwrap();

        std::env::remove_var("SUBMITTER_PRIVATE_KEY");

        let shutdown = std::future::pending::<()>();
        let res = run(config_file.path().to_path_buf(), shutdown).await;
        if let Err(e) = &res {
            println!("Error message: {}", e);
        }
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("Missing env SUBMITTER_PRIVATE_KEY"), "Unexpected error: {}", err_msg);
    }

    #[tokio::test]
    async fn test_build_blob_config() {
        let mut config_file = NamedTempFile::new().unwrap();
        write!(config_file, "
network:
  rpc_url: http://localhost:8545
  chain_id: 1337
contracts:
  bridge: '0x0000000000000000000000000000000000000000'
batch:
  data_file: 'data_blob.txt'
  new_root: '0x0000000000000000000000000000000000000000000000000000000000000000'
  blob_versioned_hash: '0x0000000000000000000000000000000000000000000000000000000000000000'
da:
  mode: blob
  blob_binding: opcode
        ").unwrap();

        std::env::set_var("SUBMITTER_PRIVATE_KEY", "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20");
        std::env::set_var("DATABASE_URL", "sqlite::memory:");
        
        std::fs::write("data_blob.txt", "dummy").unwrap();

        let res = build(config_file.path().to_path_buf()).await;
        assert!(res.is_ok());
        
        let _ = std::fs::remove_file("data_blob.txt");
    }
}
