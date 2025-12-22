use crate::application::ports::Storage;
use crate::domain::{
    batch::{Batch, BatchId, BatchStatus},
    errors::DomainError,
};
use async_trait::async_trait;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite, Row};
use std::str::FromStr;
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
        
        // Simple migration for existing tables if needed (idempotent-ish)
        // In a real app we'd use proper migrations, here we just try adding the column and ignore error
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
            let id_str: String = row.try_get("id").map_err(|e| DomainError::Storage(e.to_string()))?;
            let status_str: String = row.try_get("status").map_err(|e| DomainError::Storage(e.to_string()))?;
            
            let status = match status_str.as_str() {
                "Discovered" => BatchStatus::Discovered,
                "Proving" => BatchStatus::Proving,
                "Proved" => BatchStatus::Proved,
                "Submitting" => BatchStatus::Submitting,
                "Submitted" => BatchStatus::Submitted,
                "Confirmed" => BatchStatus::Confirmed,
                "Failed" => BatchStatus::Failed,
                _ => return Err(DomainError::Storage(format!("Unknown status: {}", status_str))),
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
                attempts: row.try_get("attempts").unwrap_or(0),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.try_get::<String, _>("created_at").unwrap_or_default()).unwrap().with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.try_get::<String, _>("updated_at").unwrap_or_default()).unwrap().with_timezone(&chrono::Utc),
            }))
        } else {
            Ok(None)
        }
    }
    
    async fn get_pending_batches(&self) -> Result<Vec<Batch>, DomainError> {
         let rows = sqlx::query("SELECT * FROM batches WHERE status != 'Confirmed' AND status != 'Failed'")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DomainError::Storage(e.to_string()))?;
            
        let mut batches = Vec::new();
        for row in rows {
            let id_str: String = row.try_get("id").unwrap();
             let status_str: String = row.try_get("status").unwrap();
             let status = match status_str.as_str() {
                "Discovered" => BatchStatus::Discovered,
                "Proving" => BatchStatus::Proving,
                "Proved" => BatchStatus::Proved,
                "Submitting" => BatchStatus::Submitting,
                "Submitted" => BatchStatus::Submitted,
                "Confirmed" => BatchStatus::Confirmed,
                 "Failed" => BatchStatus::Failed,
                _ => continue,
            };
            
             let uuid = Uuid::parse_str(&id_str).unwrap();
             
             batches.push(Batch {
                id: BatchId(uuid),
                data_file: row.try_get("data_file").unwrap_or_default(),
                new_root: row.try_get("new_root").unwrap_or_default(),
                status,
                da_mode: row.try_get("da_mode").unwrap_or_default(),
                proof: row.try_get("proof").ok(),
                tx_hash: row.try_get("tx_hash").ok(),
                attempts: row.try_get("attempts").unwrap_or(0),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.try_get::<String, _>("created_at").unwrap_or_default()).unwrap().with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.try_get::<String, _>("updated_at").unwrap_or_default()).unwrap().with_timezone(&chrono::Utc),
             });
        }
        
        Ok(batches)
    }
}
