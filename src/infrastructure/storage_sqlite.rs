use crate::application::ports::Storage;
use crate::domain::{
    batch::{Batch, BatchId, BatchStatus},
    errors::DomainError,
};
use async_trait::async_trait;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use tracing::info;
use uuid::Uuid;

pub struct SqliteStorage {
    pool: Pool<Sqlite>,
}

impl SqliteStorage {
    pub async fn new(db_url: &str) -> Result<Self, DomainError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(db_url)
            .await
            .map_err(|e| DomainError::Storage(e.to_string()))?;

        info!("Connected to SQLite");

        let storage = Self { pool };
        storage.migrate().await?;

        Ok(storage)
    }

    async fn migrate(&self) -> Result<(), DomainError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS batches (
                id TEXT PRIMARY KEY,
                data_file TEXT NOT NULL,
                new_root TEXT NOT NULL,
                status TEXT NOT NULL,
                da_mode TEXT NOT NULL,
                proof TEXT,
                tx_hash TEXT,
                attempts INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Storage(format!("Migration failed: {}", e)))?;

        let _ = sqlx::query("ALTER TABLE batches ADD COLUMN attempts INTEGER DEFAULT 0")
            .execute(&self.pool)
            .await;

        Ok(())
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn save_batch(&self, batch: &Batch) -> Result<(), DomainError> {
        let id_str = batch.id.to_string();
        let status_str = batch.status.to_string();

        sqlx::query(
            r#"
            INSERT INTO batches (id, data_file, new_root, status, da_mode, proof, tx_hash, attempts, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                proof = excluded.proof,
                tx_hash = excluded.tx_hash,
                attempts = excluded.attempts,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(id_str)
        .bind(&batch.data_file)
        .bind(&batch.new_root)
        .bind(status_str)
        .bind(&batch.da_mode)
        .bind(&batch.proof)
        .bind(&batch.tx_hash)
        .bind(batch.attempts)
        .bind(batch.created_at.to_rfc3339())
        .bind(batch.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn get_batch(&self, id: BatchId) -> Result<Option<Batch>, DomainError> {
        let row = sqlx::query("SELECT * FROM batches WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DomainError::Storage(e.to_string()))?;

        if let Some(row) = row {
            let id_str: String = row
                .try_get("id")
                .map_err(|e| DomainError::Storage(e.to_string()))?;
            let status_str: String = row
                .try_get("status")
                .map_err(|e| DomainError::Storage(e.to_string()))?;

            let status = match status_str.as_str() {
                "Discovered" => BatchStatus::Discovered,
                "Proving" => BatchStatus::Proving,
                "Proved" => BatchStatus::Proved,
                "Submitting" => BatchStatus::Submitting,
                "Submitted" => BatchStatus::Submitted,
                "Confirmed" => BatchStatus::Confirmed,
                "Failed" => BatchStatus::Failed,
                _ => {
                    return Err(DomainError::Storage(format!(
                        "Unknown status: {}",
                        status_str
                    )))
                }
            };

            let uuid = Uuid::parse_str(&id_str).map_err(|e| DomainError::Storage(e.to_string()))?;

            let created_at_str: String = row.try_get("created_at").unwrap_or_default();
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| DomainError::Storage(format!("Invalid created_at: {}", e)))?
                .with_timezone(&chrono::Utc);

            let updated_at_str: String = row.try_get("updated_at").unwrap_or_default();
            let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                .map_err(|e| DomainError::Storage(format!("Invalid updated_at: {}", e)))?
                .with_timezone(&chrono::Utc);

            Ok(Some(Batch {
                id: BatchId(uuid),
                data_file: row.try_get("data_file").unwrap_or_default(),
                new_root: row.try_get("new_root").unwrap_or_default(),
                status,
                da_mode: row.try_get("da_mode").unwrap_or_default(),
                proof: row.try_get("proof").ok(),
                tx_hash: row.try_get("tx_hash").ok(),
                attempts: row.try_get("attempts").unwrap_or(0),
                created_at,
                updated_at,
                blob_versioned_hash: None, // TODO: Add DB columns
                blob_index: None,
                fee: 0,
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_pending_batches(&self) -> Result<Vec<Batch>, DomainError> {
        let rows =
            sqlx::query("SELECT * FROM batches WHERE status != 'Confirmed' AND status != 'Failed'")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| DomainError::Storage(e.to_string()))?;

        let mut batches = Vec::new();
        for row in rows {
            let id_str: String = match row.try_get("id") {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("Skipping row with missing id: {}", e);
                    continue;
                }
            };
            let status_str: String = match row.try_get("status") {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("Skipping row with missing status: {}", e);
                    continue;
                }
            };
            let status = match status_str.as_str() {
                "Discovered" => BatchStatus::Discovered,
                "Proving" => BatchStatus::Proving,
                "Proved" => BatchStatus::Proved,
                "Submitting" => BatchStatus::Submitting,
                "Submitted" => BatchStatus::Submitted,
                "Confirmed" => BatchStatus::Confirmed,
                "Failed" => BatchStatus::Failed,
                other => {
                    tracing::warn!("Skipping row with unknown status: {}", other);
                    continue;
                }
            };

            let uuid = match Uuid::parse_str(&id_str) {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!("Skipping row with invalid uuid {}: {}", id_str, e);
                    continue;
                }
            };

            let created_at_str: String = row.try_get("created_at").unwrap_or_default();
            let created_at = match chrono::DateTime::parse_from_rfc3339(&created_at_str) {
                Ok(t) => t.with_timezone(&chrono::Utc),
                Err(e) => {
                    tracing::warn!("Skipping row with invalid created_at: {}", e);
                    continue;
                }
            };

            let updated_at_str: String = row.try_get("updated_at").unwrap_or_default();
            let updated_at = match chrono::DateTime::parse_from_rfc3339(&updated_at_str) {
                Ok(t) => t.with_timezone(&chrono::Utc),
                Err(e) => {
                    tracing::warn!("Skipping row with invalid updated_at: {}", e);
                    continue;
                }
            };

            batches.push(Batch {
                id: BatchId(uuid),
                data_file: row.try_get("data_file").unwrap_or_default(),
                new_root: row.try_get("new_root").unwrap_or_default(),
                status,
                da_mode: row.try_get("da_mode").unwrap_or_default(),
                proof: row.try_get("proof").ok(),
                tx_hash: row.try_get("tx_hash").ok(),
                attempts: row.try_get("attempts").unwrap_or(0),
                created_at,
                updated_at,
                blob_versioned_hash: None,
                blob_index: None,
                fee: 0,
            });
        }

        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_sqlite_storage_lifecycle() {
        let storage = SqliteStorage::new("sqlite::memory:").await.unwrap();

        let batch_id = BatchId(Uuid::new_v4());
        let batch = Batch {
            id: batch_id,
            data_file: "test.dat".to_string(),
            new_root: "0xroot".to_string(),
            status: BatchStatus::Discovered,
            da_mode: "calldata".to_string(),
            proof: None,
            tx_hash: None,
            attempts: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            blob_versioned_hash: None,
            blob_index: None,
            fee: 0,
        };

        // Save
        storage.save_batch(&batch).await.expect("save failed");

        // Get
        let retrieved = storage.get_batch(batch_id).await.expect("get failed").unwrap();
        assert_eq!(retrieved.id, batch.id);
        assert_eq!(retrieved.status, BatchStatus::Discovered);

        // Update
        let mut updated_batch = batch.clone();
        updated_batch.status = BatchStatus::Proving;
        storage.save_batch(&updated_batch).await.expect("update failed");

        let retrieved_2 = storage.get_batch(batch_id).await.expect("get failed").unwrap();
        assert_eq!(retrieved_2.status, BatchStatus::Proving);

        // Get Pending
        let pending = storage.get_pending_batches().await.expect("pending failed");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, batch.id);
    }

    #[tokio::test]
    async fn test_sqlite_malformed_data() {
        let storage = SqliteStorage::new("sqlite::memory:").await.unwrap();
        
        // Insert a bad row manually (invalid UUID)
        sqlx::query(
            "INSERT INTO batches (id, data_file, new_root, status, da_mode, created_at, updated_at) \
             VALUES ('bad-uuid', 'f', 'r', 'Discovered', 'm', '2023-01-01T00:00:00Z', '2023-01-01T00:00:00Z')"
        )
        .execute(&storage.pool)
        .await
        .unwrap();

        // Should be skipped
        let pending = storage.get_pending_batches().await.unwrap();
        assert!(pending.is_empty());
    }
}
