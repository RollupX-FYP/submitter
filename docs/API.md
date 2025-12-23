# API Documentation

This document describes the APIs exposed by the Submitter Daemon and the APIs it consumes.

## Metrics API

The service exposes Prometheus metrics on port `9000` by default.

**Endpoint**: `GET /metrics`

### Key Metrics
- `batch_transitions_total`: Counter of state transitions (e.g., Discovered -> Proving).
- `prove_duration_seconds`: Histogram of time taken to generate proofs.
- `submit_tx_duration_seconds`: Histogram of time taken to submit transactions to Ethereum.
- `batches_completed_total`: Total number of batches successfully confirmed.
- `batch_failures_total`: Total failures encountered.

## Configuration Interface

The primary "API" for the user is the configuration. The daemon is configured via `submitter.yaml` and Environment Variables.

### `submitter.yaml`
```yaml
network:
  rpc_url: "http://127.0.0.1:8545" # Ethereum RPC URL
  chain_id: 31337

contracts:
  bridge: "0x..." # Address of the ZKRollupBridge contract

da:
  mode: "calldata" # "calldata" or "blob" (EIP-4844)
  blob_binding: "mock" # "mock" or "opcode"
  blob_index: 0 # Index of the blob in the transaction

prover:
  url: "http://localhost:3000" # URL of the Prover Service (optional, defaults to Mock if omitted)

resilience:
  max_retries: 5
  circuit_breaker_threshold: 5
```

### Environment Variables
- `SUBMITTER_PRIVATE_KEY`: **Required**. The private key (hex) of the wallet submitting transactions.
- `DATABASE_URL`: **Optional**. Connection string for the database (e.g., `postgres://user:pass@localhost:5432/db` or `sqlite:submitter.db`).

## Internal/External APIs

### Prover API
If using the `HttpProofProvider` (production), the daemon expects an external Prover Service listening at the configured `prover.url`.

**Request**: `POST /prove`
```json
{
  "batch_id": "uuid-string",
  "public_inputs": [ ... byte array ... ]
}
```

**Response**: `200 OK`
```json
{
  "proof": "hex-encoded-proof-string"
}
```
**Error**: Any non-200 status code triggers a retry (with exponential backoff) or circuit breaker trip.

### Ethereum RPC
The daemon uses standard JSON-RPC methods (`eth_sendRawTransaction`, `eth_getTransactionReceipt`, etc.) to interact with the Ethereum chain.
