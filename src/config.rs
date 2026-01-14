use anyhow::{Context, Result};
use ethers::types::Address;
use serde::Deserialize;
use std::{fs, path::PathBuf};
use tracing::warn;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub network: Network,
    pub contracts: Contracts,
    pub da: DaConfig,
    pub batch: BatchConfig,
    // Optional prover URL
    #[allow(dead_code)]
    pub prover: Option<ProverConfig>,
    // Optional resilience config
    #[allow(dead_code)]
    pub resilience: Option<ResilienceConfig>,
    // Optional fees config
    #[allow(dead_code)]
    pub fees: Option<FeeConfig>,
    // Optional flow config
    #[allow(dead_code)]
    pub flow: Option<FlowConfig>,
    // Optional sequencer config (for future/mock implementation)
    #[allow(dead_code)]
    pub sequencer: Option<SequencerConfig>,
    // Optional aggregator config (for future/mock implementation)
    #[allow(dead_code)]
    pub aggregator: Option<AggregatorConfig>,
    // Optional simulation config (for local testing/mocking)
    #[allow(dead_code)]
    pub simulation: Option<SimulationConfig>,
}

#[derive(Debug, Deserialize)]
pub struct ResilienceConfig {
    #[allow(dead_code)]
    pub max_retries: Option<u32>,
    #[allow(dead_code)]
    pub circuit_breaker_threshold: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct Network {
    pub rpc_url: String,
    pub chain_id: u64,
}

#[derive(Debug, Deserialize)]
pub struct Contracts {
    pub bridge: String,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct DaConfig {
    pub mode: DaMode,
    pub blob_binding: BlobBinding,
    pub blob_index: Option<u8>,
    pub archiver_url: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum DaMode {
    Calldata,
    Blob,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum BlobBinding {
    Mock,
    Opcode,
}

#[derive(Debug, Deserialize)]
pub struct BatchConfig {
    pub data_file: String,
    pub new_root: String,
    pub blob_versioned_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProverConfig {
    #[allow(dead_code)]
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct FeeConfig {
    #[allow(dead_code)]
    pub policy: FeePolicy,
    #[allow(dead_code)]
    pub max_blob_fee_gwei: Option<u64>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FeePolicy {
    Standard,
    Aggressive,
    Fixed,
}

#[derive(Debug, Deserialize)]
pub struct FlowConfig {
    #[allow(dead_code)]
    pub enable_forced_inclusion: bool,
}

// --- New Configs for V2 Report (Simulated/Future Components) ---

#[derive(Debug, Deserialize)]
pub struct SequencerConfig {
    #[allow(dead_code)]
    pub batch_size: Option<u32>,
    #[allow(dead_code)]
    pub batch_timeout_ms: Option<u64>,
    #[allow(dead_code)]
    pub ordering_policy: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AggregatorConfig {
    #[allow(dead_code)]
    pub compression: Option<CompressionMode>,
}

#[derive(Debug, Deserialize, PartialEq, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum CompressionMode {
    FullTxData,
    StateDiff,
}

#[derive(Debug, Deserialize)]
pub struct SimulationConfig {
    #[allow(dead_code)]
    pub mock_proving_time_ms: Option<u64>,
    #[allow(dead_code)]
    pub gas_price_fluctuation: Option<f64>,
}

pub fn load_config(path: PathBuf) -> Result<Config> {
    let raw = fs::read_to_string(&path).context("read config yaml")?;
    let cfg: Config = serde_yaml::from_str(&raw).context("parse yaml")?;
    validate_config(&cfg)?;
    Ok(cfg)
}

fn validate_config(cfg: &Config) -> Result<()> {
    // Validate addresses
    cfg.contracts
        .bridge
        .parse::<Address>()
        .context("Invalid bridge address")?;

    // Validate specific requirements based on mode
    if cfg.da.mode == DaMode::Blob {
        if cfg.batch.blob_versioned_hash.is_none() {
            anyhow::bail!("blob mode needs batch.blob_versioned_hash in yaml");
        }
        if cfg.da.archiver_url.is_none() {
            warn!("Blob mode selected but no 'archiver_url' provided. Blobs will not be archived (Data availability risk).");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_calldata_config() {
        let yaml = r#"
network:
  rpc_url: "http://localhost:8545"
  chain_id: 123
contracts:
  bridge: "0x0000000000000000000000000000000000000001"
da:
  mode: "calldata"
  blob_binding: "mock"
batch:
  data_file: "data.txt"
  new_root: "0x00"
"#;
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.da.mode, DaMode::Calldata);
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn test_full_config_v2() {
        let yaml = r#"
network:
  rpc_url: "http://localhost:8545"
  chain_id: 123
contracts:
  bridge: "0x0000000000000000000000000000000000000001"
da:
  mode: "blob"
  blob_binding: "opcode"
  archiver_url: "http://archive"
batch:
  data_file: "data.txt"
  new_root: "0x00"
  blob_versioned_hash: "0x1234"
fees:
  policy: "aggressive"
  max_blob_fee_gwei: 100
flow:
  enable_forced_inclusion: true
sequencer:
  batch_size: 50
  batch_timeout_ms: 5000
  ordering_policy: "fifo"
aggregator:
  compression: "state_diff"
simulation:
  mock_proving_time_ms: 200
  gas_price_fluctuation: 1.2
"#;
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.sequencer.is_some());
        assert_eq!(cfg.sequencer.unwrap().batch_size, Some(50));
        assert!(cfg.aggregator.is_some());
        assert_eq!(cfg.aggregator.unwrap().compression, Some(CompressionMode::StateDiff));
        assert!(cfg.simulation.is_some());
        assert_eq!(cfg.simulation.unwrap().mock_proving_time_ms, Some(200));
    }
}
