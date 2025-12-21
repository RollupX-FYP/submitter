use anyhow::{Context, Result};
use clap::Parser;
use dotenvy::dotenv;
use ethers::prelude::*;
use serde::Deserialize;
use std::{fs, path::PathBuf, sync::Arc};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    config: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Config {
    network: Network,
    contracts: Contracts,
    da: DaConfig,
    batch: BatchConfig,
}

#[derive(Debug, Deserialize)]
struct Network {
    rpc_url: String,
    chain_id: u64,
}

#[derive(Debug, Deserialize)]
struct Contracts {
    bridge: String,
}

#[derive(Debug, Deserialize)]
struct DaConfig {
    mode: String,          // "calldata" | "blob"
    blob_binding: String,  // "mock" | "opcode"
    blob_index: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct BatchConfig {
    data_file: String, // path to batch data file
    new_root: String,  // 0x...
    // commitment only needed if you want to precompute; contract computes for calldata path.
    blob_versioned_hash: Option<String>, // 0x... (for blob mode)
}

abigen!(
    ZKRollupBridge,
    r#"[
        function commitBatchCalldata(bytes batchData, bytes32 newRoot, (uint256[2],uint256[2][2],uint256[2]) proof)
        function commitBatchBlob(bytes32 expectedVersionedHash, uint8 blobIndex, bool useOpcodeBlobhash, bytes32 newRoot, (uint256[2],uint256[2][2],uint256[2]) proof)
    ]"#,
);

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let args = Args::parse();

    let raw = fs::read_to_string(&args.config).context("read config yaml")?;
    let cfg: Config = serde_yaml::from_str(&raw).context("parse yaml")?;

    let pk = std::env::var("SUBMITTER_PRIVATE_KEY")
        .context("Missing env SUBMITTER_PRIVATE_KEY (DO NOT put private keys in yaml)")?;
    let wallet: LocalWallet = pk.parse::<LocalWallet>()?.with_chain_id(cfg.network.chain_id);

    let provider = Provider::<Http>::try_from(cfg.network.rpc_url.as_str())?;
    let client = Arc::new(SignerMiddleware::new(provider, wallet));

    let bridge_addr: Address = cfg.contracts.bridge.parse()?;
    let bridge = ZKRollupBridge::new(bridge_addr, client.clone());

    // Dummy Groth16 proof for mock verifier
    let proof = (
        [U256::zero(), U256::zero()],
        [[U256::zero(), U256::zero()], [U256::zero(), U256::zero()]],
        [U256::zero(), U256::zero()],
    );

    let new_root: H256 = cfg.batch.new_root.parse()?;

    match cfg.da.mode.as_str() {
        "calldata" => {
            let batch_bytes = fs::read(&cfg.batch.data_file)
                .with_context(|| format!("read batch file {}", cfg.batch.data_file))?;

            let pending = bridge
                .commit_batch_calldata(batch_bytes.into(), new_root, proof)
                .send()
                .await?;

            let receipt = pending.await?.context("tx dropped")?;
            println!("✅ calldata batch submitted. tx={:?}", receipt.transaction_hash);
        }
        "blob" => {
            let vh = cfg
                .batch
                .blob_versioned_hash
                .clone()
                .context("blob mode needs batch.blob_versioned_hash in yaml")?;
            let expected: H256 = vh.parse()?;

            let blob_index = cfg.da.blob_index.unwrap_or(0);
            let use_opcode = cfg.da.blob_binding == "opcode";

            let pending = bridge
                .commit_batch_blob(expected, blob_index, use_opcode, new_root, proof)
                .send()
                .await?;

            let receipt = pending.await?.context("tx dropped")?;
            println!(
                "✅ blob batch submitted ({} binding). tx={:?}",
                cfg.da.blob_binding,
                receipt.transaction_hash
            );
        }
        other => anyhow::bail!("Unknown da.mode: {other} (use 'calldata' or 'blob')"),
    }

    Ok(())
}
