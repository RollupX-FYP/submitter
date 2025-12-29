# API Documentation

This document provides a comprehensive reference for the Submitter Service's Configuration and Metrics APIs.

## 1. Configuration (`submitter.yaml`)

The configuration file is deserialized into the `Config` struct defined in `src/config.rs`.

### `network`
Defines the connection to the L1 chain.
*   `rpc_url` (String): HTTP endpoint for the JSON-RPC node.
*   `chain_id` (Integer): Chain ID (e.g., 1 for Mainnet, 31337 for Hardhat).

### `contracts`
Addresses of deployed smart contracts.
*   `bridge` (Address): The `ZKRollupBridge` contract address (0x...).

### `da` (Data Availability)
Controls how batch data is posted to Ethereum.
*   `mode` (Enum):
    *   `calldata`: Uses `calldata` in standard transactions.
    *   `blob`: Uses EIP-4844 blobs.
*   `blob_binding` (Enum):
    *   `opcode`: Expects a real network supporting `BLOBHASH`.
    *   `mock`: For local testing where blob sidecars might not be fully supported by the node.
*   `blob_index` (Integer): The index of the blob in the transaction (usually 0).
*   `archiver_url` (String): URL of the external Archiver service to store blob data before expiry.

### `fees` (Experimental)
Research controls for fee market behavior (RQ2).
*   `policy` (Enum):
    *   `standard`: Uses standard gas estimation.
    *   `aggressive`: Bids higher priority fees to reduce latency.
    *   `fixed`: Uses a hardcoded gas price (for baseline benchmarks).
*   `max_blob_fee_gwei` (Integer): Cap on the blob base fee.

### `flow` (Experimental)
Controls for transaction inclusion logic.
*   `enable_forced_inclusion` (Boolean): If true, the Submitter checks the L1 Forced Queue and includes those transactions (simulated).

### `resilience`
Reliability settings.
*   `max_retries` (Integer): Number of times to retry a failed batch before marking it `Failed`.
*   `circuit_breaker_threshold` (Integer): Consecutive failures allowed for external services (Prover) before pausing.

### `simulation`
Parameters for the Simulation Layer (Mock Prover).
*   `mock_proving_time_ms` (Integer): Milliseconds to sleep during proof generation to simulate ZK computation time.
*   `gas_price_fluctuation` (Float): Multiplier to simulate volatile L1 fees.

### `sequencer` (Mock/Future)
Parameters that define the behavior of the simulated Sequencer.
*   `batch_size` (Integer): Target number of transactions per batch.
*   `batch_timeout_ms` (Integer): Max time to wait before sealing a batch.
*   `ordering_policy` (String): Strategy for ordering txs (`fifo`, `fee_priority`).

### `aggregator` (Mock/Future)
Parameters for data compression strategies (RQ3).
*   `compression` (Enum): `full_tx_data` vs `state_diff`.

---

## 2. Environment Variables

Secrets and infrastructure bindings are strictly handled via Environment Variables.

| Variable | Required | Description |
| :--- | :--- | :--- |
| `SUBMITTER_PRIVATE_KEY` | **Yes** | Hex-encoded private key of the account sending transactions. |
| `DATABASE_URL` | **Yes** | Connection string. `sqlite://file.db` or `postgres://user:pass@host:5432/db`. |
| `RUST_LOG` | No | Logging level (default `info`). Example: `debug,submitter_rs=trace`. |

---

## 3. Metrics API (Prometheus)

The service runs a dedicated HTTP server on port `9000` exposing `/metrics`.

### Counters
*   `batch_transitions_total`: Logs state changes (e.g., `Discovered` -> `Proving`). Labels: `from`, `to`.
*   `batches_completed_total`: Total successful batches confirmed on L1.
*   `batch_failures_total`: Total error events. Label: `batch_id`.
*   `batches_failed_permanent_total`: Batches that exceeded retry limits.

### Histograms
*   `prove_duration_seconds`: Time taken by the ProofProvider.
*   `submit_tx_duration_seconds`: Time taken to construct and broadcast the transaction.
*   `batch_e2e_duration_seconds`: Total time from `Discovered` to `Confirmed`.
