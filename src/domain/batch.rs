use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::fmt;
use sha1_smol::Sha1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BatchId(pub Uuid);

impl BatchId {
    pub fn new() -> Self {
        BatchId(Uuid::new_v4())
    }
    
    pub fn deterministic(
        chain_id: u64,
        bridge_addr: &str,
        data_hash: &str, 
        new_root: &str, 
        da_mode: &str
    ) -> Self {
        // Idempotency key construction:
        // chain_id | bridge_addr | data_hash | new_root | da_mode
        let input = format!("{}|{}|{}|{}|{}", chain_id, bridge_addr, data_hash, new_root, da_mode);
        let namespace = Uuid::NAMESPACE_OID;
        BatchId(Uuid::new_v5(&namespace, input.as_bytes()))
    }
}

impl fmt::Display for BatchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchStatus {
    Discovered,
    Proving,
    Proved,
    Submitting,
    Submitted,
    Confirmed,
    Failed,
}

impl fmt::Display for BatchStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub id: BatchId,
    pub data_file: String,
    pub new_root: String, // Hex string
    pub status: BatchStatus,
    pub da_mode: String,
    pub proof: Option<String>, // Serialized proof
    pub tx_hash: Option<String>,
    pub attempts: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Batch {
    pub fn new(
        chain_id: u64,
        bridge_addr: &str,
        data_file: String, 
        data_hash: String,
        new_root: String, 
        da_mode: String
    ) -> Self {
        let now = Utc::now();
        Self {
            id: BatchId::deterministic(chain_id, bridge_addr, &data_hash, &new_root, &da_mode),
            data_file,
            new_root,
            status: BatchStatus::Discovered,
            da_mode,
            proof: None,
            tx_hash: None,
            attempts: 0,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn transition_to(&mut self, status: BatchStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_id_deterministic() {
        let id1 = BatchId::deterministic(1, "0xBridge", "hash1", "root1", "calldata");
        let id2 = BatchId::deterministic(1, "0xBridge", "hash1", "root1", "calldata");
        let id3 = BatchId::deterministic(2, "0xBridge", "hash1", "root1", "calldata");

        assert_eq!(id1, id2, "Same inputs should produce same ID");
        assert_ne!(id1, id3, "Different inputs should produce different ID");
    }

    #[test]
    fn test_batch_creation() {
        let batch = Batch::new(
            1, "0xBridge", "file.txt".into(), "hash".into(), "root".into(), "blob".into()
        );
        assert_eq!(batch.status, BatchStatus::Discovered);
        assert_eq!(batch.attempts, 0);
    }

    #[test]
    fn test_batch_transition() {
        let mut batch = Batch::new(
            1, "0xBridge", "file.txt".into(), "hash".into(), "root".into(), "blob".into()
        );
        batch.transition_to(BatchStatus::Proving);
        assert_eq!(batch.status, BatchStatus::Proving);
    }
}
