use axum::{routing::get, Router};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;

pub fn init_tracing() {
    // Check for JSON log format request
    let use_json = std::env::var("LOG_JSON").unwrap_or_else(|_| "true".to_string()) == "true";

    if use_json {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }
}

pub fn init_metrics() -> PrometheusHandle {
    let builder = PrometheusBuilder::new();
    builder
        .install_recorder()
        .expect("failed to install Prometheus recorder")
}

pub async fn start_metrics_server(handle: PrometheusHandle) {
    let app = Router::new().route("/metrics", get(move || std::future::ready(handle.render())));

    let addr = SocketAddr::from(([0, 0, 0, 0], 9000));
    info!("Metrics server listening on {}", addr);

    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind metrics port");
    axum::serve(listener, app)
        .await
        .expect("failed to start metrics server");
}
