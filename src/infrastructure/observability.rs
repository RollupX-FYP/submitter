use anyhow::Result;
use axum::{routing::get, Router};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;

pub fn init_tracing() {
    // Check for JSON log format request
    let use_json = std::env::var("LOG_JSON").unwrap_or_else(|_| "true".to_string()) == "true";
    let filter = tracing_subscriber::EnvFilter::from_default_env();

    if use_json {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .try_init();
    }
}

pub fn init_metrics() -> Result<PrometheusHandle> {
    let builder = PrometheusBuilder::new();
    builder
        .install_recorder()
        .map_err(|e| anyhow::anyhow!("Failed to install recorder: {:?}", e))
}

pub async fn start_metrics_server(handle: PrometheusHandle, port: u16) {
    let app = Router::new().route("/metrics", get(move || std::future::ready(handle.render())));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Metrics server listening on {}", addr);

    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind metrics port");
    axum::serve(listener, app)
        .await
        .expect("failed to start metrics server");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tracing_safe() {
        init_tracing();
        init_tracing();
    }

    #[test]
    fn test_init_metrics_safe() {
        let _ = init_metrics();
    }
}
