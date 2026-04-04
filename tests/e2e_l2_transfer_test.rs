use ethers::prelude::*;
use std::time::Duration;
use reqwest;

// Note: This test requires a full stack running locally (Anvil, Hardhat, Executor, Sequencer, Submitter).
// Currently, `scripts/run_e2e_debug.sh` provides this environment and relies on this ignored test
// for the structure.
#[tokio::test]
#[ignore]
async fn test_full_l2_transfer_flow() {
    // 1. Submit L2 transfer via reqwest
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "sendTransaction",
        "params": {
            "from": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
            "to": "0x0202020202020202020202020202020202020202",
            "value": "0x3e8",
            "nonce": 0,
            "gas_price": "0x1",
            "gas_limit": 21000,
            "signature": "0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001b",
            "timestamp": 1672531200
        },
        "id": 1
    });

    let res = client.post("http://127.0.0.1:3000/")
        .json(&payload)
        .send()
        .await
        .expect("Sequencer did not accept tx");

    assert!(res.status().is_success());

    // In a real environment, we would also verify Outbox states here via direct SQLite connection.
    // The bash script does this via log parsing.
}
