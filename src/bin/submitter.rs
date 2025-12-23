use anyhow::Result;
use clap::Parser;
use dotenvy::dotenv;
use std::path::PathBuf;
use submitter_rs::{infrastructure::observability, startup};
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // 1. Observability
    observability::init_tracing();
    let metrics_handle = observability::init_metrics().expect("failed to install Prometheus recorder");
    tokio::spawn(observability::start_metrics_server(metrics_handle, 9000));

    let args = Args::parse();
    
    let shutdown = async {
        #[cfg(unix)]
        {
            let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => { info!("Ctrl-C received"); },
                _ = sigterm.recv() => { info!("SIGTERM received"); },
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await.expect("failed to install CTRL+C handler");
            info!("Ctrl-C received");
        }
    };

    startup::run(args.config, shutdown).await
}
