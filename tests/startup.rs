use submitter_rs::startup;
use std::io::Write;
use tempfile::NamedTempFile;
use wiremock::matchers::{method};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_full_startup_flow_logic() {
    let mock_server = MockServer::start().await;
    let rpc_url = mock_server.uri();
    
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "0x539"
        })))
        .mount(&mock_server)
        .await;

    let mut config_file = NamedTempFile::new().unwrap();
    let config_content = format!(r#"
network:
  rpc_url: "{}"
  chain_id: 1337
contracts:
  bridge: '0x0000000000000000000000000000000000000000'
batch:
  data_file: 'data_full_logic.txt'
  new_root: '0x0000000000000000000000000000000000000000000000000000000000000000'
  blob_versioned_hash: '0x0000000000000000000000000000000000000000000000000000000000000000'
da:
  mode: calldata
  blob_binding: opcode
prover:
  url: "{}"
"#, rpc_url, rpc_url);

    write!(config_file, "{}", config_content).unwrap();
    let config_path = config_file.path().to_path_buf();

    std::env::set_var("SUBMITTER_PRIVATE_KEY", "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20");
    std::env::set_var("DATABASE_URL", "sqlite::memory:");
    
    std::fs::write("data_full_logic.txt", "dummy").unwrap();

    let (storage, orchestrator) = startup::build(config_path).await.expect("Failed to build app");

    let pending = storage.get_pending_batches().await.expect("Failed to get pending");
    assert_eq!(pending.len(), 1);
    
    let handle = tokio::spawn(async move {
        orchestrator.run().await
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    handle.abort();
    let _ = std::fs::remove_file("data_full_logic.txt");
}

#[tokio::test]
async fn test_full_startup_run_shutdown() {
    let mock_server = MockServer::start().await;
    let rpc_url = mock_server.uri();
    
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "0x539"
        })))
        .mount(&mock_server)
        .await;

    let mut config_file = NamedTempFile::new().unwrap();
    let config_content = format!(r#"
network:
  rpc_url: "{}"
  chain_id: 1337
contracts:
  bridge: '0x0000000000000000000000000000000000000000'
batch:
  data_file: 'data_full_run.txt'
  new_root: '0x0000000000000000000000000000000000000000000000000000000000000000'
  blob_versioned_hash: '0x0000000000000000000000000000000000000000000000000000000000000000'
da:
  mode: calldata
  blob_binding: opcode
prover:
  url: "{}"
"#, rpc_url, rpc_url);

    write!(config_file, "{}", config_content).unwrap();
    let config_path = config_file.path().to_path_buf();

    std::env::set_var("SUBMITTER_PRIVATE_KEY", "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20");
    std::env::set_var("DATABASE_URL", "sqlite::memory:");
    
    std::fs::write("data_full_run.txt", "dummy").unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move { let _ = rx.await; };

    let handle = tokio::spawn(async move {
        startup::run(config_path, shutdown).await
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let _ = tx.send(());
    let res = handle.await.unwrap();
    assert!(res.is_ok());
    
    let _ = std::fs::remove_file("data_full_run.txt");
}
