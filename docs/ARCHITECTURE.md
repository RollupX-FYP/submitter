# Submitter Architecture

The Submitter Daemon follows a **Domain-Driven Design (DDD)** approach with a **Hexagonal Architecture** (Ports and Adapters). This ensures the core logic is isolated from external dependencies like databases, blockchains, and gRPC services.

## 1. Domain Layer (`src/domain/`)

The innermost layer containing pure business logic and type definitions. It has zero external dependencies.

*   **Entities**:
    *   `Batch`: The aggregate root. Tracks ID, status, data pointers, and retry counts.
*   **Value Objects**:
    *   `BatchId`: A UUID v4/v5 generated from batch parameters.
*   **Enums**:
    *   `BatchStatus`:
        *   `Discovered`: Received from Executor gRPC stream.
        *   `Submitting`: Transaction construction in progress.
        *   `Submitted`: Tx broadcasted to L1.
        *   `Confirmed`: Tx mined and settled.
        *   `Failed`: Terminal error state.

## 2. Application Layer (`src/application/`)

Contains the orchestration logic that drives the domain entities through their lifecycle.

### The Orchestrator Loop
The `Daemon` service runs an infinite loop that:
1.  **Polls** the `BatchSource` (gRPC or File) for new batch payloads.
2.  **Processes** each batch sequentially:
    *   Extracts `proof`, `batch_data`, and `state_roots`.
    *   Selects the configured DA Strategy (Calldata or Blob).
    *   Submits the transaction to L1.
    *   Waits for confirmation.

### Ports (Traits)
Interfaces defined in `ports.rs` that the Application layer depends on:
*   `BatchSource`: Abstract source of batches (e.g., `GrpcBatchSource`, `FileBatchSource`).
*   `DaStrategy`: `submit`, `check_confirmation`.
*   `L1Connector`: Interface to the L1 RPC (ethers-rs).

## 3. Infrastructure Layer (`src/infrastructure/`)

Contains the concrete implementations (Adapters).

### Batch Sources
*   **GrpcBatchSource**: Connects to the Executor's `StreamBatches` gRPC endpoint. It handles reconnection logic and deserializes the `BatchPayload` Protobuf message.
*   **FileBatchSource**: Watches a local `batch_output.json` file (legacy/dev mode).

### DA Strategies
*   **CalldataStrategy**: Encodes batch data into the `batchData` calldata field.
*   **BlobStrategy**: Creates an EIP-4844 transaction. Encodes the `VersionedHash` in `daMeta`, attaches the blob as a sidecar.

## 4. Simulation & Testing

*   **Metrics**: The system emits Prometheus metrics to measure the latency introduced by each stage.
*   **Integration Tests**: Validates the full flow against a local Hardhat node.
