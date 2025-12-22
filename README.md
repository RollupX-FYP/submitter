# Submitter RS

A Rust-based ZK Rollup Batch Submitter. This service watches for batch files and submits them to the configured bridge contract on an Ethereum-compatible chain (e.g., Anvil, Sepolia).

## Features

- **Multi-mode Submission**: Supports both standard `calldata` and `blob` (EIP-4844) transaction types.
- **Configurable**: Fully configurable via YAML configuration files.
- **Type-safe**: Uses strict typing for configuration and ABI interactions.
- **Dockerized**: Ready for deployment using Docker.

## Configuration

Configuration is managed via a YAML file. An example `submitter.yaml` is provided:

```yaml
network:
  rpc_url: "http://127.0.0.1:8545"
  chain_id: 31337

contracts:
  bridge: "0xYOUR_BRIDGE_ADDRESS"

da:
  mode: "calldata" # Options: "calldata" | "blob"
  blob_binding: "mock" # Options: "mock" | "opcode"
  blob_index: 0 # Optional

batch:
  data_file: "path/to/batch.txt"
  new_root: "0x..."
  blob_versioned_hash: "0x..." # Required for blob mode
```

## Running Locally

### Prerequisites

- Rust (latest stable)
- OpenSSL (`libssl-dev` on Ubuntu/Debian)

### Build and Run

```bash
# Build
cargo build --release

# Run
export SUBMITTER_PRIVATE_KEY="your_private_key_hex"
./target/release/submitter-rs --config submitter.yaml
```

**Note**: You must provide the private key via the `SUBMITTER_PRIVATE_KEY` environment variable. Do not commit private keys to files.

## Running with Docker

### Build Image

```bash
docker build -t submitter-rs .
```

### Run Container

```bash
docker run -v $(pwd)/submitter.yaml:/app/submitter.yaml \
           -v $(pwd)/data:/app/data \
           -e SUBMITTER_PRIVATE_KEY="your_private_key_hex" \
           submitter-rs --config /app/submitter.yaml
```

## Testing

Run unit tests:

```bash
cargo test
```
