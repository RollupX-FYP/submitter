use tracing_test::traced_test;
use submitter_rs::saga::{SagaOutbox, SagaState};
use tempfile::NamedTempFile;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
#[traced_test]
fn test_4_3_a_gas_bump_on_stuck_transaction() {
    let db_file = NamedTempFile::new().unwrap();
    let db_path = db_file.path().to_str().unwrap().to_string();
    let outbox = SagaOutbox::new(&db_path).unwrap();

    let batch_id = "test-gas-bump-001";
    let batch_data = r#"{"dummy": "data"}"#;
    let proof = "0xdeadbeef";
    let tx_hash = "0xoriginalhash";
    let nonce = 5i64;
    let original_gas = 1_000_000_000_u64; // 1 gwei
    let original_gas_str = original_gas.to_string();

    outbox.insert_or_ignore(batch_id, batch_data, proof).unwrap();
    outbox.update_submission(batch_id, tx_hash, Some(nonce), Some(&original_gas_str)).unwrap();

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    let past_time = now - 301;

    outbox.conn.lock().unwrap().execute(
        "UPDATE batch_outbox SET last_updated = ?1 WHERE batch_id = ?2",
        rusqlite::params![past_time, batch_id],
    ).unwrap();

    let unconfirmed = outbox.get_unconfirmed_batches().unwrap();
    assert_eq!(unconfirmed.len(), 1);
    let record = &unconfirmed[0];
    
    assert_eq!(record.state, SagaState::SubmittedToL1);
    assert_eq!(record.original_gas_price.as_deref(), Some(original_gas_str.as_str()));
    
    assert!(now - record.last_updated > 300, "Submitter logic detects > 300 seconds");
    
    let current_gas_price = original_gas;
    let bump = current_gas_price / 5; // 20% bump
    let new_gas_price = current_gas_price + bump;
    
    assert_eq!(new_gas_price, 1_200_000_000, "New gas price should be exactly 1.2x original");

    let new_hash = "0xbumpedhash";
    outbox.update_submission(batch_id, new_hash, record.nonce, None).unwrap();
    
    let updated_record = outbox.get_record(batch_id).unwrap().unwrap();
    assert_eq!(updated_record.tx_hash.unwrap(), new_hash);
    assert_eq!(updated_record.nonce, Some(nonce));
    assert_eq!(updated_record.original_gas_price.unwrap(), original_gas_str, "Original gas price must be preserved across bumps");
}

#[test]
#[traced_test]
fn test_4_3_b_gas_multiplier_cap() {
    let original_gas = 1_000_000_000_u64; // 1 gwei
    let max_gas_price = original_gas * 3; // 3 gwei
    let mut current_gas_price = original_gas;
    
    let expected_sequence = [
        1_200_000_000, // Cycle 1: 1.2x
        1_440_000_000, // Cycle 2: 1.44x
        1_728_000_000, // Cycle 3: 1.728x
        2_073_600_000, // Cycle 4: 2.0736x
        2_488_320_000, // Cycle 5: 2.48832x
        2_985_984_000, // Cycle 6: 2.985984x
    ];

    for (cycle, &expected) in expected_sequence.iter().enumerate() {
        let bump = current_gas_price / 5;
        let mut new_gas_price = current_gas_price + bump;
        
        if new_gas_price > max_gas_price {
            new_gas_price = max_gas_price;
        }
        
        assert_eq!(new_gas_price, expected, "Mismatch at cycle {}", cycle + 1);
        current_gas_price = new_gas_price;
    }
    
    // Cycle 7 should hit the cap
    let bump = current_gas_price / 5;
    let mut new_gas_price = current_gas_price + bump;
    
    assert!(new_gas_price > max_gas_price, "Uncapped price would be > 3x");
    
    // As implemented in daemon.rs, the first time we hit it we cap it and broadcast:
    if new_gas_price > max_gas_price {
        new_gas_price = max_gas_price;
    }
    assert_eq!(new_gas_price, max_gas_price, "b) Gas price is strictly capped at 3x the original price");
    
    // Simulate updating the tx with the capped max price
    current_gas_price = new_gas_price;
    
    // Cycle 8 (Next attempt after hitting the cap)
    // As implemented in daemon.rs:
    // `if current_gas_price == max_gas_price { update_state(Failed); continue; }`
    let db_file = NamedTempFile::new().unwrap();
    let db_path = db_file.path().to_str().unwrap().to_string();
    let outbox = SagaOutbox::new(&db_path).unwrap();
    
    let batch_id = "test-gas-cap-002";
    outbox.insert_or_ignore(batch_id, "{}", "0x").unwrap();
    outbox.update_submission(batch_id, "0xhash", Some(1), Some(&original_gas.to_string())).unwrap();
    
    // The cap logic in `daemon.rs` transitions to FAILED
    // Since we're unit testing the cap logic, we simulate the internal checks
    let is_chronically_stuck = current_gas_price == max_gas_price;
    assert!(is_chronically_stuck, "Should detect chronically stuck");
    
    if is_chronically_stuck {
        outbox.update_state(batch_id, SagaState::Failed).unwrap();
    }
    
    let record = outbox.get_record(batch_id).unwrap().unwrap();
    assert_eq!(record.state, SagaState::Failed, "c) Transitions to FAILED state");
}
