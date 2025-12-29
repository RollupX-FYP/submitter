# ZK Rollup Batch Submitter

[![CI](https://github.com/RollupX-FYP/submitter/actions/workflows/ci.yml/badge.svg)](https://github.com/RollupX-FYP/submitter/actions/workflows/ci.yml)
[![Coverage](https://github.com/RollupX-FYP/submitter/actions/workflows/coverage.yml/badge.svg)](https://github.com/RollupX-FYP/submitter/actions/workflows/coverage.yml)
[![Security](https://github.com/RollupX-FYP/submitter/actions/workflows/security.yml/badge.svg)](https://github.com/RollupX-FYP/submitter/actions/workflows/security.yml)
[![Docker](https://github.com/RollupX-FYP/submitter/actions/workflows/docker-publish.yml/badge.svg)](https://github.com/RollupX-FYP/submitter/actions/workflows/docker-publish.yml)

A production-grade, highly reliable Rust service for submitting ZK Rollup batches to Ethereum. It handles the complete lifecycle of batches from discovery to proof generation and on-chain submission, utilizing a robust Domain-Driven Design (DDD) architecture.

## Features

- **Robust Architecture:** Built using **Domain-Driven Design (DDD)** principles and Hexagonal Architecture (Ports and Adapters).
- **Reliability & Resilience:** Implements **Outbox Pattern**, **Saga Workflow**, **Circuit Breakers**, **Exponential Backoff**, and **Crash Recovery** to ensure fault tolerance.
- **Data Availability:** Supports **Calldata** (Legacy) and **EIP-4844 Blobs** with **Archiver** integration for long-term persistence.
- **Experimental Features:**
    - **Priority Scheduling:** Reorders batches based on fees when configured.
    - **Payload Compression:** Compresses calldata using Zlib (flate2) to reduce L1 costs.
- **Idempotency:** Deterministic batch processing (UUID v5) prevents double-spending and ensures consistency.
- **Observability:** Built-in Prometheus metrics and structured JSON logging (Tracing).
- **Persistence:** Supports both **SQLite** (local/dev) and **PostgreSQL** (production).

## Documentation

For more detailed information, please refer to:

- [**API Documentation**](docs/API.md): Details on Metrics, Configuration, and Prover APIs.
- [**Architecture**](docs/ARCHITECTURE.md): Deep dive into the DDD implementation, layers, and system design.
- [**Integration Guide**](docs/INTEGRATION.md): Detailed deployment, database, and external service setup.
- [**Best Practices**](BEST_PRACTICES.md): Explanation of system design patterns.
- [**Agent Instructions**](AGENTS.md): Guidelines for contributors and AI agents.
- [**Local Testing**](../LOCAL_TESTING.md): End-to-end local testing guide.

## Getting Started

### Prerequisites

- **Rust**: Latest stable version (1.83+ recommended).
- **Docker**: For containerized deployment and Postgres integration tests.
- **Ethereum Node**: An RPC endpoint (e.g., Anvil, Hardhat, Geth).

### Configuration

The daemon is configured via a YAML file (e.g., `submitter.yaml`) and Environment Variables.

**Environment Variables:**
- `SUBMITTER_PRIVATE_KEY`: **Required**. The private key (hex) of the wallet submitting transactions.
- `DATABASE_URL`: **Required**. Connection string for the database.
    - SQLite: `sqlite://data/submitter.db`
    - Postgres: `postgres://user:pass@localhost:5432/db`
- `RUST_LOG`: **Optional**. Log level (e.g., `info`, `debug`).

### Running Locally

To run with a local SQLite database:

```bash
# 1. Create data directory and db file
mkdir -p data
touch data/submitter.db

# 2. Export Env Vars
export DATABASE_URL=sqlite://data/submitter.db
export SUBMITTER_PRIVATE_KEY="0x..." # Replace with your key

# 3. Run
cargo run --bin submitter -- --config submitter.yaml
```

### Docker Usage

Build the production image:
```bash
docker build -t submitter .
```

Run the container:
```bash
docker run -d \
  --name submitter \
  -p 9000:9000 \
  -e SUBMITTER_PRIVATE_KEY="0x..." \
  -v $(pwd)/submitter.yaml:/app/submitter.yaml \
  submitter \
  /app/submitter --config /app/submitter.yaml
```

### Metrics

The service exposes Prometheus metrics on port `9000` by default.

- Endpoint: `http://localhost:9000/metrics`

## Testing

The project requires **100% code coverage**.

```bash
# Run all tests (Unit + Integration)
cargo test --all-features

# Run integration tests specifically
cargo test --test lifecycle
cargo test --test startup
cargo test --test cli

# Run with Postgres (Optional, CI does this automatically)
# Requires Docker running on localhost:5432
export DATABASE_URL="postgres://postgres:password@localhost:5432/submitter"
cargo test --lib infrastructure::storage_postgres
```

## License

This project is licensed under the Apache License 2.0.
