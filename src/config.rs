use anyhow::{Context, Result};
use ethers::types::Address;
use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub network: Network,
    pub contracts: Contracts,
    pub da: DaConfig,
    pub batch: BatchConfig,
    // Optional prover URL
    pub prover: Option<ProverConfig>,
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

#[derive(Debug, Deserialize, PartialEq)]
pub struct DaConfig {
    pub mode: DaMode,
    pub blob_binding: BlobBinding,
    pub blob_index: Option<u8>,
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
    pub url: String,
}

pub fn load_config(path: PathBuf) -> Result<Config> {
    let raw = fs::read_to_string(&path).context("read config yaml")?;
    let cfg: Config = serde_yaml::from_str(&raw).context("parse yaml")?;
    validate_config(&cfg)?;
    Ok(cfg)
}

fn validate_config(cfg: &Config) -> Result<()> {
    // Validate addresses
    cfg.contracts.bridge.parse::<Address>().context("Invalid bridge address")?;
    
    // Validate specific requirements based on mode
    if cfg.da.mode == DaMode::Blob && cfg.batch.blob_versioned_hash.is_none() {
        anyhow::bail!("blob mode needs batch.blob_versioned_hash in yaml");
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
    fn test_valid_blob_config() {
        let yaml = r#"
network:
  rpc_url: "http://localhost:8545"
  chain_id: 123
contracts:
  bridge: "0x0000000000000000000000000000000000000001"
da:
  mode: "blob"
  blob_binding: "opcode"
batch:
  data_file: "data.txt"
  new_root: "0x00"
  blob_versioned_hash: "0x1234"
"#;
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.da.mode, DaMode::Blob);
        assert_eq!(cfg.da.blob_binding, BlobBinding::Opcode);
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn test_invalid_blob_config_missing_hash() {
        let yaml = r#"
network:
  rpc_url: "http://localhost:8545"
  chain_id: 123
contracts:
  bridge: "0x0000000000000000000000000000000000000001"
da:
  mode: "blob"
  blob_binding: "opcode"
batch:
  data_file: "data.txt"
  new_root: "0x00"
"#;
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(validate_config(&cfg).is_err());
    }

    #[test]
    fn test_invalid_address() {
        let yaml = r#"
network:
  rpc_url: "http://localhost:8545"
  chain_id: 123
contracts:
  bridge: "invalid_address"
da:
  mode: "calldata"
  blob_binding: "mock"
batch:
  data_file: "data.txt"
  new_root: "0x00"
"#;
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(validate_config(&cfg).is_err());
    }

    #[test]
    fn test_prover_config() {
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
prover:
  url: "http://prover:3000"
"#;
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.prover.is_some());
        assert_eq!(cfg.prover.unwrap().url, "http://prover:3000");
    }
}
