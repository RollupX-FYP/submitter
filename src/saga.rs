use anyhow::Result;
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SagaState {
    ReceivedFromExecutor,
    Compressed,
    SubmittedToL1,
    ConfirmedOnL1,
    Failed,
}

impl SagaState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SagaState::ReceivedFromExecutor => "RECEIVED_FROM_EXECUTOR",
            SagaState::Compressed => "COMPRESSED",
            SagaState::SubmittedToL1 => "SUBMITTED_TO_L1",
            SagaState::ConfirmedOnL1 => "CONFIRMED_ON_L1",
            SagaState::Failed => "FAILED",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "RECEIVED_FROM_EXECUTOR" => Some(SagaState::ReceivedFromExecutor),
            "COMPRESSED" => Some(SagaState::Compressed),
            "SUBMITTED_TO_L1" => Some(SagaState::SubmittedToL1),
            "CONFIRMED_ON_L1" => Some(SagaState::ConfirmedOnL1),
            "FAILED" => Some(SagaState::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BatchSagaRecord {
    pub batch_id: String,
    pub state: SagaState,
    pub tx_hash: Option<String>,
    pub nonce: Option<i64>,
    pub last_updated: i64,
    pub batch_data: Option<String>, // Serialized JSON of domain::batch::Batch
    pub proof_hex: Option<String>, // The proof needed to resume
    pub original_gas_price: Option<String>,
}

#[derive(Clone)]
pub struct SagaOutbox {
    pub conn: Arc<Mutex<Connection>>,
}

impl SagaOutbox {
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS batch_outbox (
                batch_id TEXT PRIMARY KEY,
                state TEXT NOT NULL,
                tx_hash TEXT,
                nonce INTEGER,
                last_updated INTEGER NOT NULL,
                batch_data TEXT,
                proof_hex TEXT,
                original_gas_price TEXT
            )",
            [],
        )?;

        info!("Saga outbox initialized at {}", db_path);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn insert_or_ignore(&self, batch_id: &str, batch_data: &str, proof_hex: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let rows_affected = conn.execute(
            "INSERT OR IGNORE INTO batch_outbox (batch_id, state, last_updated, batch_data, proof_hex) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![batch_id, SagaState::ReceivedFromExecutor.as_str(), now, batch_data, proof_hex],
        )?;
        Ok(rows_affected > 0)
    }

    pub fn update_state(&self, batch_id: &str, state: SagaState) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE batch_outbox SET state = ?1, last_updated = ?2 WHERE batch_id = ?3",
            params![state.as_str(), now, batch_id],
        )?;
        Ok(())
    }

    pub fn update_submission(&self, batch_id: &str, tx_hash: &str, nonce: Option<i64>, gas_price: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        
        // If gas_price is provided (first submission), save it. Otherwise keep existing (bumps)
        if let Some(gp) = gas_price {
            conn.execute(
                "UPDATE batch_outbox SET state = ?1, tx_hash = ?2, nonce = ?3, last_updated = ?4, original_gas_price = ?5 WHERE batch_id = ?6",
                params![SagaState::SubmittedToL1.as_str(), tx_hash, nonce, now, gp, batch_id],
            )?;
        } else {
            conn.execute(
                "UPDATE batch_outbox SET state = ?1, tx_hash = ?2, nonce = ?3, last_updated = ?4 WHERE batch_id = ?5",
                params![SagaState::SubmittedToL1.as_str(), tx_hash, nonce, now, batch_id],
            )?;
        }
        
        Ok(())
    }

    pub fn get_unconfirmed_batches(&self) -> Result<Vec<BatchSagaRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT batch_id, state, tx_hash, nonce, last_updated, batch_data, proof_hex, original_gas_price FROM batch_outbox WHERE state != ?1"
        )?;
        
        let batch_iter = stmt.query_map(params![SagaState::ConfirmedOnL1.as_str()], |row| {
            let state_str: String = row.get(1)?;
            Ok(BatchSagaRecord {
                batch_id: row.get(0)?,
                state: SagaState::from_str(&state_str).unwrap_or(SagaState::ReceivedFromExecutor),
                tx_hash: row.get(2)?,
                nonce: row.get(3)?,
                last_updated: row.get(4)?,
                batch_data: row.get(5)?,
                proof_hex: row.get(6)?,
                original_gas_price: row.get(7)?,
            })
        })?;

        let mut batches = Vec::new();
        for batch in batch_iter {
            batches.push(batch?);
        }
        
        Ok(batches)
    }

    pub fn get_record(&self, batch_id: &str) -> Result<Option<BatchSagaRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT batch_id, state, tx_hash, nonce, last_updated, batch_data, proof_hex, original_gas_price FROM batch_outbox WHERE batch_id = ?1"
        )?;
        
        let mut batch_iter = stmt.query_map(params![batch_id], |row| {
            let state_str: String = row.get(1)?;
            Ok(BatchSagaRecord {
                batch_id: row.get(0)?,
                state: SagaState::from_str(&state_str).unwrap_or(SagaState::ReceivedFromExecutor),
                tx_hash: row.get(2)?,
                nonce: row.get(3)?,
                last_updated: row.get(4)?,
                batch_data: row.get(5)?,
                proof_hex: row.get(6)?,
                original_gas_price: row.get(7)?,
            })
        })?;

        if let Some(record) = batch_iter.next() {
            Ok(Some(record?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_startup_recovery_from_submitted_state() {
        let db_file = NamedTempFile::new().unwrap();
        let db_path = db_file.path().to_str().unwrap();
        let outbox = SagaOutbox::new(db_path).unwrap();

        let batch_id = "test_batch_1";
        let batch_data = "{\"dummy\": \"data\"}";
        let proof_hex = "0x123";
        outbox.insert_or_ignore(batch_id, batch_data, proof_hex).unwrap();
        
        // Advance state to SUBMITTED_TO_L1 but deliberately push last_updated to past (e.g. 6 mins ago)
        outbox.update_submission(batch_id, "0xabc123", Some(42), Some("10000")).unwrap();
        
        // Manually adjust the timestamp in sqlite to simulate a stuck tx (e.g., 301 seconds ago)
        {
            let conn = outbox.conn.lock().unwrap();
            let past = chrono::Utc::now().timestamp_millis() - 301000;
            conn.execute("UPDATE batch_outbox SET last_updated = ?1 WHERE batch_id = ?2", params![past, batch_id]).unwrap();
        }

        // Now run recovery check
        let unconfirmed = outbox.get_unconfirmed_batches().unwrap();
        assert_eq!(unconfirmed.len(), 1);
        let record = &unconfirmed[0];
        
        assert_eq!(record.batch_id, batch_id);
        assert_eq!(record.state, SagaState::SubmittedToL1);
        assert_eq!(record.tx_hash.as_deref(), Some("0xabc123"));
        assert_eq!(record.nonce, Some(42));
    }

    #[test]
    fn test_deduplication() {
        let db_file = NamedTempFile::new().unwrap();
        let db_path = db_file.path().to_str().unwrap();
        let outbox = SagaOutbox::new(db_path).unwrap();

        let batch_id = "duplicate_batch_id";
        let batch_data = "{}";
        let proof_hex = "0x";
        
        // First insert
        let inserted1 = outbox.insert_or_ignore(batch_id, batch_data, proof_hex).unwrap();
        assert!(inserted1, "First insert should succeed");
        
        // Second insert
        let inserted2 = outbox.insert_or_ignore(batch_id, batch_data, proof_hex).unwrap();
        assert!(!inserted2, "Second insert should return false / be ignored");

        // Verify get_record returns the item
        let record = outbox.get_record(batch_id).unwrap();
        assert!(record.is_some());
    }

    #[test]
    fn test_gas_bump_simulation() {
        let db_file = NamedTempFile::new().unwrap();
        let db_path = db_file.path().to_str().unwrap();
        let outbox = SagaOutbox::new(db_path).unwrap();

        let batch_id = "stuck_batch";
        outbox.insert_or_ignore(batch_id, "{}", "0x").unwrap();
        outbox.update_submission(batch_id, "0x_old_hash", Some(10), Some("5000")).unwrap();

        // Simulate the gas bump updating the outbox (subsequent bumps do not rewrite the original gas price if not passed)
        outbox.update_submission(batch_id, "0x_new_bumped_hash", Some(10), None).unwrap();

        let record = outbox.get_record(batch_id).unwrap().unwrap();
        assert_eq!(record.state, SagaState::SubmittedToL1);
        assert_eq!(record.tx_hash.as_deref(), Some("0x_new_bumped_hash"));
        assert_eq!(record.nonce, Some(10));
    }
}
