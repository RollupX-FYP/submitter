# Architecture

The Submitter Daemon follows a **Domain-Driven Design (DDD)** approach with a **Hexagonal Architecture** (Ports and Adapters). This ensures the core logic is isolated from external dependencies like databases, blockchains, and HTTP services.

## High-Level Overview

```mermaid
graph TD
    subgraph "Core Domain"
        Batch[Batch Entity]
        StateMachine[State Machine Logic]
    end

    subgraph "Application Layer"
        Orchestrator[Orchestrator]
        Ports[Ports (Traits)]
    end

    subgraph "Infrastructure Layer"
        Storage[Storage Adapter (SQLx)]
        Prover[Prover Adapter (HTTP/Mock)]
        DA[DA Strategy (Calldata/Blob)]
    end

    Orchestrator --> Ports
    Ports -.-> Storage
    Ports -.-> Prover
    Ports -.-> DA
    
    Orchestrator --> Batch
    Batch --> StateMachine
```

## Layers

### 1. Domain (`src/domain/`)
Contains the pure business logic and entities.
*   **Batch**: The core entity tracking the lifecycle of a rollup batch.
*   **BatchStatus**: Enum representing the state (`Discovered`, `Proving`, `Proved`, `Submitting`, `Submitted`, `Confirmed`, `Failed`).
*   **DomainError**: Standardized error types for the application.

### 2. Application (`src/application/`)
Orchestrates the flow of data using the Domain entities and Ports.
*   **Orchestrator**: The main loop that fetches pending batches and drives them through the state machine.
*   **Ports**: Rust `traits` defining the interfaces for external systems:
    *   `Storage`: For saving/retrieving batches.
    *   `ProofProvider`: For generating ZK proofs.
    *   `DaStrategy`: For submitting data to Ethereum.

### 3. Infrastructure (`src/infrastructure/`)
Concrete implementations (Adapters) of the Application Ports.
*   **Storage**:
    *   `PostgresStorage`: Production database adapter.
    *   `SqliteStorage`: Development/Testing database adapter.
*   **ProofProvider**:
    *   `HttpProofProvider`: Talks to an external Prover Service (includes Circuit Breaker).
    *   `MockProofProvider`: Returns dummy proofs for testing.
*   **DaStrategy**:
    *   `CalldataStrategy`: Submits batch data as transaction calldata (Standard Rollup).
    *   `BlobStrategy`: Submits batch data using EIP-4844 Blobs.

## Batch Lifecycle (State Machine)

1.  **Discovered**: Batch is created/ingested.
2.  **Proving**: Orchestrator requests a proof from the `ProofProvider`.
3.  **Proved**: Proof is received and stored.
4.  **Submitting**: Orchestrator constructs the transaction using the `DaStrategy`.
5.  **Submitted**: Transaction broadcasted to Ethereum (Tx Hash stored).
6.  **Confirmed**: Transaction successfully mined with sufficient confirmations.
7.  **Failed**: Permanent failure (e.g., max retries exceeded, invalid data).

## Reliability Features

*   **Circuit Breaker**: The `HttpProofProvider` stops sending requests if the Prover Service fails repeatedly, allowing it to recover.
*   **Exponential Backoff**: Used for retrying transient network errors.
*   **Idempotency**: Batch IDs are deterministic (UUID v5) to prevent duplicate processing.
*   **Observability**: Structured logging (Tracing) and Prometheus metrics.
