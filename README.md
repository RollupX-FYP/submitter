# ZK Rollup Batch Submitter

[![CI](https://github.com/RollupX-FYP/submitter/actions/workflows/ci.yml/badge.svg)](https://github.com/RollupX-FYP/submitter/actions/workflows/ci.yml)
[![Coverage](https://github.com/RollupX-FYP/submitter/actions/workflows/coverage.yml/badge.svg)](https://github.com/RollupX-FYP/submitter/actions/workflows/coverage.yml)
[![Security](https://github.com/RollupX-FYP/submitter/actions/workflows/security.yml/badge.svg)](https://github.com/RollupX-FYP/submitter/actions/workflows/security.yml)
[![Docker](https://github.com/RollupX-FYP/submitter/actions/workflows/docker-publish.yml/badge.svg)](https://github.com/RollupX-FYP/submitter/actions/workflows/docker-publish.yml)
[![Proof HTML](https://github.com/RollupX-FYP/submitter/actions/workflows/proof-html.yml/badge.svg)](https://github.com/RollupX-FYP/submitter/actions/workflows/proof-html.yml)

A production-grade, highly reliable Rust service for submitting ZK Rollup batches to Ethereum.

## Features

- **Robust Architecture:** Follows **Domain-Driven Design (DDD)** principles.
- **Reliability:** Implements **Outbox Pattern**, **Saga Workflow**, **Circuit Breakers**, and **Crash Recovery**.
- **Idempotency:** Deterministic batch processing prevents double-spending.
- **Observability:** Built-in Prometheus metrics and structured JSON logging.
- **Flexibility:** Supports multiple Data Availability (DA) strategies (`Calldata` and `Blob` EIP-4844).
- **Persistence:** Supports both **SQLite** (local/dev) and **PostgreSQL** (production).

## Documentation

- [Best Practices & Architecture](BEST_PRACTICES.md): Detailed explanation of the system design and patterns.
- [Agent Instructions](AGENTS.md): Guidelines for contributors and AI agents.

## Getting Started

### Prerequisites

- Rust (latest stable)
- Docker (for Postgres integration tests)

### Configuration

Create a `submitter.yaml` file (see `submitter.yaml` example in repo) or pass the path via CLI.

```bash
# Run with SQLite (default)
cargo run -- --config submitter.yaml

# Run with Postgres
export DATABASE_URL="postgres://user:pass@localhost:5432/db"
cargo run -- --config submitter.yaml
```

### Metrics

The service exposes Prometheus metrics on port `9000` by default.

- Endpoint: `http://localhost:9000/metrics`

## Testing

```bash
# Run unit and integration tests (SQLite)
cargo test

# Run integration tests with Postgres
docker compose up -d postgres
export DATABASE_URL="postgres://postgres:postgres@localhost:5432/submitter"
cargo test --test integration_test
```
