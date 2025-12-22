use anyhow::{Context, Result};
use clap::Parser;
use dotenvy::dotenv;
use ethers::prelude::*;
use std::{fs, path::PathBuf, sync::Arc};
use tracing::info;

mod config;
mod contracts;
mod submitter;

use config::DaMode;
use contracts::ZKRollupBridge;
use submitter::Submitter;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let cfg = config::load_config(args.config)?;

    let pk = std::env::var("SUBMITTER_PRIVATE_KEY")
        .context("Missing env SUBMITTER_PRIVATE_KEY (DO NOT put private keys in yaml)")?;
    let wallet: LocalWallet = pk
        .parse::<LocalWallet>()?
        .with_chain_id(cfg.network.chain_id);

    let provider = Provider::<Http>::try_from(cfg.network.rpc_url.as_str())?;
    let client = Arc::new(SignerMiddleware::new(provider, wallet));

    let bridge_addr: Address = cfg.contracts.bridge.parse()?;
    let bridge = ZKRollupBridge::new(bridge_addr, client.clone());

    // Dummy Groth16 proof for mock verifier
    // We need to match the structure defined in abigen
    let proof = contracts::Groth16Proof {
        a: [U256::zero(), U256::zero()],
        b: [[U256::zero(), U256::zero()], [U256::zero(), U256::zero()]],
        c: [U256::zero(), U256::zero()],
    };

    let new_root: H256 = cfg.batch.new_root.parse()?;
    let submitter = Submitter::new(bridge);

    match cfg.da.mode {
        DaMode::Calldata => {
            let batch_bytes = fs::read(&cfg.batch.data_file)
                .with_context(|| format!("read batch file {}", cfg.batch.data_file))?;

            let tx_hash = submitter
                .submit_calldata(batch_bytes, new_root.into(), proof)
                .await?;

            info!("✅ calldata batch submitted. tx={:?}", tx_hash);
        }
        DaMode::Blob => {
            let vh = cfg
                .batch
                .blob_versioned_hash
                .clone()
                .context("blob mode needs batch.blob_versioned_hash in yaml")?;
            let expected: H256 = vh.parse()?;

            let blob_index = cfg.da.blob_index.unwrap_or(0);
            let use_opcode = cfg.da.blob_binding == config::BlobBinding::Opcode;

            let tx_hash = submitter
                .submit_blob(
                    expected.into(),
                    blob_index,
                    use_opcode,
                    new_root.into(),
                    proof,
                )
                .await?;

            info!(
                "✅ blob batch submitted ({:?} binding). tx={:?}",
                cfg.da.blob_binding, tx_hash
            );
        }
    }

    Ok(())
}
