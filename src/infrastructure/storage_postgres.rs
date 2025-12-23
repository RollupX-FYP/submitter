use crate::application::ports::Storage;
use crate::domain::{
    batch::{Batch, BatchId, BatchStatus},
    errors::DomainError,
};
use async_trait::async_trait;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres, Row};
use tracing::info;
use uuid::Uuid;

pub struct PostgresStorage {
    pool: Pool<Postgres>,
}

impl PostgresStorage {
    pub async fn new(db_url: &str) -> Result<Self, DomainError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(db_url)
            .await
            .map_err(|e| DomainError::Storage(e.to_string()))?;

        info!("Connected to Postgres");

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
                created_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Storage(format!("Migration failed: {}", e)))?;

        // Simple migration for existing tables if needed
        let _ =
            sqlx::query("ALTER TABLE batches ADD COLUMN IF NOT EXISTS attempts INTEGER DEFAULT 0")
                .execute(&self.pool)
                .await;

        Ok(())
    }
}

#[async_trait]
impl Storage for PostgresStorage {
    async fn save_batch(&self, batch: &Batch) -> Result<(), DomainError> {
        let id_str = batch.id.to_string();
        let status_str = batch.status.to_string();

        sqlx::query(
            r#"
            INSERT INTO batches (id, data_file, new_root, status, da_mode, proof, tx_hash, attempts, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
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
        .bind(batch.attempts as i32)
        .bind(batch.created_at)
        .bind(batch.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn get_batch(&self, id: BatchId) -> Result<Option<Batch>, DomainError> {
        let row = sqlx::query("SELECT * FROM batches WHERE id = $1")
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

            Ok(Some(Batch {
                id: BatchId(uuid),
                data_file: row.try_get("data_file").unwrap_or_default(),
                new_root: row.try_get("new_root").unwrap_or_default(),
                status,
                da_mode: row.try_get("da_mode").unwrap_or_default(),
                proof: row.try_get("proof").ok(),
                tx_hash: row.try_get("tx_hash").ok(),
                attempts: row.try_get::<i32, _>("attempts").unwrap_or(0) as u32,
                created_at: row
                    .try_get("created_at")
                    .map_err(|e| DomainError::Storage(e.to_string()))?,
                updated_at: row
                    .try_get("updated_at")
                    .map_err(|e| DomainError::Storage(e.to_string()))?,
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

            let created_at = match row.try_get("created_at") {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("Skipping row with invalid created_at: {}", e);
                    continue;
                }
            };

            let updated_at = match row.try_get("updated_at") {
                Ok(t) => t,
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
                attempts: row.try_get::<i32, _>("attempts").unwrap_or(0) as u32,
                created_at,
                updated_at,
            });
        }

        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::env;

    fn get_db_url() -> String {
        env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string())
    }

    #[tokio::test]
    async fn test_postgres_storage_lifecycle() {
        // Skip if no DB available (rudimentary check)
        let db_url = get_db_url();
        if std::net::TcpStream::connect("localhost:5432").is_err() && env::var("CI").is_err() {
            println!("Skipping postgres test: no db");
            return;
        }

        let storage = match PostgresStorage::new(&db_url).await {
            Ok(s) => s,
            Err(_) => {
                println!("Skipping postgres test: connection failed");
                return;
            }
        };

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
        };

        // Save
        storage.save_batch(&batch).await.expect("save failed");

        // Get
        let retrieved = storage.get_batch(batch_id).await.expect("get failed").unwrap();
        assert_eq!(retrieved.id, batch.id);

        // Update
        let mut updated_batch = batch.clone();
        updated_batch.status = BatchStatus::Proving;
        storage.save_batch(&updated_batch).await.expect("update failed");

        let retrieved_2 = storage.get_batch(batch_id).await.expect("get failed").unwrap();
        assert_eq!(retrieved_2.status, BatchStatus::Proving);
    }
}
