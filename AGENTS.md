# Instructions for Agents

This repository adheres to strict coding standards. When modifying this codebase, you **MUST** follow these instructions.

## Scope
These instructions apply to all files within the `src/` directory and `tests/`.

## Core Directives

1.  **Architecture Violation Check:**
    *   Do **NOT** import `infrastructure` modules into `domain` modules.
    *   Do **NOT** put business logic in `src/bin/`.
    *   **Always** define an interface in `src/application/ports.rs` before implementing a new external service adapter.

2.  **Persistence & State:**
    *   **Always** use the `Storage` trait for database interactions.
    *   **Never** perform a side effect (tx send, API call) without persisting the intent or state change immediately before or after.
    *   **Idempotency:** Ensure any new entity creation uses deterministic IDs if it represents external data.

3.  **Reliability:**
    *   **Retry Loops:** Never implement an infinite loop without a `max_attempts` check or a persistent state exit condition.
    *   **Timeouts:** All network calls (`reqwest`, `ethers`) must have timeouts configured.

4.  **Testing:**
    *   If you modify logic, you **must** add a test case.
    *   Run `cargo test` after every significant change.
    *   Run `cargo check` to ensure no unused imports or warnings are introduced.

5.  **Observability:**
    *   If you add a new code path, add a `counter!` metric.
    *   If you add a network call, add a `histogram!` metric for duration.

## Automated Verification
Before submitting your changes, verify:
1.  `cargo check` passes without warnings.
2.  `cargo test` passes.
3.  The project structure matches the `BEST_PRACTICES.md` definition.
