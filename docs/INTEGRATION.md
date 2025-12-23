# Integration Guide

This guide details how to integrate, deploy, and run the Submitter Daemon.

## Prerequisites

*   **Rust**: Stable toolchain (1.83+ recommended).
*   **Docker**: For containerized deployment.
*   **PostgreSQL**: For production persistence (SQLite used for dev).
*   **Ethereum Node**: An RPC endpoint (e.g., Anvil, Geth, Alchemy, Infura).

## Local Development Setup

1.  **Clone the repository**:
    ```bash
    git clone https://github.com/your-org/submitter.git
    cd submitter
    ```

2.  **Configuration**:
    Copy `submitter.yaml` and modify it if needed.
    ```bash
    # Ensure submitter.yaml exists
    cat submitter.yaml
    ```

3.  **Environment Variables**:
    Create a `.env` file or export variables:
    ```bash
    export SUBMITTER_PRIVATE_KEY="0x..." # Your wallet private key
    # Optional: export DATABASE_URL="sqlite:submitter.db"
    ```

4.  **Run the Daemon**:
    ```bash
    cargo run --bin submitter -- --config submitter.yaml
    ```

## Docker Deployment

The repository includes a Dockerfile for building a production-ready image.

### Build Image
```bash
docker build -t submitter:latest .
```

### Run Container
```bash
docker run -d \
  --name submitter \
  -p 9000:9000 \
  -e SUBMITTER_PRIVATE_KEY="0x..." \
  -v $(pwd)/submitter.yaml:/app/submitter.yaml \
  submitter:latest \
  /app/submitter --config /app/submitter.yaml
```

## External Services Integration

### 1. Database (Postgres)
For production, use PostgreSQL.
*   Set `DATABASE_URL=postgres://user:password@host:5432/dbname`.
*   The application uses `sqlx` and should handle schema creation if migrations are embedded (check source). *Note: Ensure tables `batches` exists.*

### 2. Prover Service
The daemon sends HTTP POST requests to the configured `prover.url`.
*   Ensure your Prover Service complies with the API described in `docs/API.md`.
*   If testing, you can omit `prover.url` in config to use the internal **Mock Prover**.

### 3. Smart Contract (Bridge)
The daemon interacts with the `ZKRollupBridge` contract.
*   Ensure the `contracts.bridge` address in `submitter.yaml` is correct for the connected chain (`network.chain_id`).
*   The wallet (`SUBMITTER_PRIVATE_KEY`) must have ETH to pay for gas.

## Troubleshooting

*   **"Missing env SUBMITTER_PRIVATE_KEY"**: Ensure the environment variable is set.
*   **"Provider error"**: Check your `rpc_url` and ensure the node is reachable.
*   **Circuit Breaker Open**: The Prover Service is down or returning 500s. Check the Prover logs.
