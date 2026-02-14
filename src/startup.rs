use crate::daemon;
use anyhow::Result;
use std::future::Future;
use std::path::PathBuf;
use tracing::info;

pub async fn run(
    config_path: PathBuf,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    tokio::select! {
        res = daemon::run(config_path) => {
            res?;
        },
        _ = shutdown => {
            info!("Shutdown signal received");
        },
    }

    Ok(())
}
