# Submitter Best Practices & Architecture

This repository follows a strict **Domain-Driven Design (DDD)** approach combined with robust distributed system reliability patterns. All contributions must adhere to these standards.

## 1. Architecture (DDD)

The codebase is organized into four distinct layers:

### `src/domain/` (Pure Logic)
*   **Entities:** Core business objects (e.g., `Batch`).
*   **Logic:** State transitions, validation rules, and business invariants.
*   **No Dependencies:** This layer **must not** depend on external libraries (HTTP, DB, Blockchain) or the `infrastructure` layer. It is pure Rust logic.
*   **Example:** `Batch::transition_to`, `BatchId::deterministic`.

### `src/application/` (Orchestration)
*   **Ports:** Defines `traits` (interfaces) for external services (`Storage`, `ProofProvider`, `DaStrategy`).
*   **Orchestrator:** Implements the "Saga" workflow / State Machine. It coordinates the Domain entities and Infrastructure adapters.
*   **Use Cases:** Contains the main application loops and logic flows.

### `src/infrastructure/` (Adapters)
*   **Implementations:** Concrete implementations of Application Ports.
*   **Dependencies:** Holds all heavy dependencies (`sqlx`, `ethers`, `reqwest`).
*   **Adapters:**
    *   `storage_sqlite.rs`, `storage_postgres.rs` (Persistence)
    *   `prover_http.rs` (External Service)
    *   `da_calldata.rs`, `da_blob.rs` (Blockchain Interaction)

### `src/bin/` (Entry Point)
*   **Wiring:** Thin wrappers around `src/startup.rs` (daemon) and `src/script.rs` (script).
*   **Testability:** Logic is moved to library modules to enable integration testing of the entry point logic.

---

## 2. Reliability Patterns

### Idempotency
*   **Deterministic IDs:** Batch IDs are generated using `UUID v5` based on `chain_id`, `bridge_address`, `data_hash`, `new_root`, and `da_mode`.
*   **Dedup:** The database enforces uniqueness on `id`. Re-running the submitter on the same input data yields the same ID and resumes progress instead of duplicating work.

### Outbox / Saga Pattern
*   **State Machine:** The `Batch` entity moves through persisted states: `Discovered` → `Proving` → `Proved` → `Submitting` → `Submitted` → `Confirmed`.
*   **Persistence First:** State changes are saved to the DB *before* or *immediately after* external actions to ensure crash recovery.
*   **Crash Recovery:** On startup, the system queries `Pending` batches and resumes their workflow.

### Circuit Breaker & Retry
*   **Prover Service:** The `HttpProofProvider` uses a Circuit Breaker (Closed -> Open -> HalfOpen) to prevent hammering a failing service.
*   **Exponential Backoff:** Retries use exponential backoff for transient errors.
*   **Dead Letter:** Batches exceeding `max_attempts` are moved to `Failed` status to prevent infinite loops.

### Safety
*   **Confirmation:** Transactions are considered confirmed only if `receipt.status == 1` AND (optionally) `confirmations >= N`.
*   **Reverts:** Reverted transactions trigger an error and retry/failure handling.

---

## 3. Observability

*   **Metrics:** All critical paths must be instrumented with Prometheus metrics (`counters` for events, `histograms` for duration).
*   **Logging:** Use `tracing` with structured JSON output (enabled via `LOG_JSON=true`). Never log secrets (keys, full payloads).

## 4. Testing Standards

*   **Unit Tests:** Test Domain logic and Config parsing. Use `sqlite::memory:` for storage adapter unit tests.
*   **Integration Tests:**
    *   `tests/lifecycle.rs`: Tests the `Orchestrator` loop using mocks for external services.
    *   `tests/startup.rs`: Tests the full application startup, wiring, and shutdown using `startup::run` and `wiremock`.
    *   `tests/cli.rs`: Tests the binary entry points using `assert_cmd`.
*   **Mocking:**
    *   Use Traits (`DaStrategy`, `ProofProvider`) to mock external dependencies in application layer tests.
    *   Use `crate::test_utils::MockClient` to mock `ethers` JSON-RPC responses in infrastructure adapters (`DaStrategy`, `Submitter`) to verify correct call sequences (e.g. `eth_feeHistory`, `eth_estimateGas`) and error handling.
