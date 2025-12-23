# ZK Rollup Batch Submitter

[![CI](https://github.com/your-org/submitter/actions/workflows/ci.yml/badge.svg)](https://github.com/your-org/submitter/actions/workflows/ci.yml)
[![Coverage](https://github.com/your-org/submitter/actions/workflows/coverage.yml/badge.svg)](https://github.com/your-org/submitter/actions/workflows/coverage.yml)
[![Security](https://github.com/your-org/submitter/actions/workflows/security.yml/badge.svg)](https://github.com/your-org/submitter/actions/workflows/security.yml)
[![Docker](https://github.com/your-org/submitter/actions/workflows/docker-publish.yml/badge.svg)](https://github.com/your-org/submitter/actions/workflows/docker-publish.yml)
[![Proof HTML](https://github.com/your-org/submitter/actions/workflows/proof-html.yml/badge.svg)](https://github.com/your-org/submitter/actions/workflows/proof-html.yml)

A production-grade, highly reliable Rust service for submitting ZK Rollup batches to Ethereum. It handles the complete lifecycle of batches from discovery to proof generation and on-chain submission, utilizing a robust Domain-Driven Design (DDD) architecture.

## Features

- **Robust Architecture:** Built using **Domain-Driven Design (DDD)** principles and Hexagonal Architecture (Ports and Adapters).
- **Reliability & Resilience:** Implements **Outbox Pattern**, **Saga Workflow**, **Circuit Breakers**, **Exponential Backoff**, and **Crash Recovery** to ensure fault tolerance.
- **Data Availability:** Supports both **Calldata** (Legacy) and **EIP-4844 Blobs** for cost-efficient data posting.
- **Idempotency:** Deterministic batch processing (UUID v5) prevents double-spending and ensures consistency.
- **Observability:** Built-in Prometheus metrics and structured JSON logging (Tracing).
- **Flexibility:** Configurable via YAML and Environment Variables.
- **Persistence:** Supports both **SQLite** (local/dev) and **PostgreSQL** (production).

## Documentation

For more detailed information, please refer to:

- [**API Documentation**](docs/API.md): Details on Metrics, Configuration, and Prover APIs.
- [**Architecture**](docs/ARCHITECTURE.md): Deep dive into the DDD implementation, layers, and system design.
- [**Integration Guide**](docs/INTEGRATION.md): Detailed deployment, database, and external service setup.
- [**Best Practices**](BEST_PRACTICES.md): Explanation of system design patterns.
- [**Agent Instructions**](AGENTS.md): Guidelines for contributors and AI agents.

## Getting Started

### Prerequisites

- **Rust**: Latest stable version (1.83+ recommended).
- **Docker**: For containerized deployment and Postgres integration tests.
- **Ethereum Node**: An RPC endpoint (e.g., Anvil, Geth).

### Configuration

The daemon is configured via `submitter.yaml`. You can copy the example in the repo.

```bash
# Run with SQLite (default)
cargo run --bin submitter -- --config submitter.yaml
```

**Environment Variables:**
- `SUBMITTER_PRIVATE_KEY`: **Required**. The private key (hex) of the wallet submitting transactions.
- `DATABASE_URL`: **Optional**. Connection string for the database (e.g., `postgres://user:pass@localhost:5432/db`).

```bash
# Run with Postgres
export DATABASE_URL="postgres://user:pass@localhost:5432/db"
export SUBMITTER_PRIVATE_KEY="0x..."
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
