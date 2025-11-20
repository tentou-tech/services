# CoW Protocol Services

## Project Overview

This repository contains the backend services for [CoW Protocol](https://docs.cow.fi/), a decentralized trading protocol. The project is written in Rust and organized as a Cargo workspace with multiple crates, implementing a microservices architecture.

### Key Components

*   **`orderbook`**: The HTTP API service.
    *   Allows users (and frontends) to place signed orders and query their status.
    *   Validates orders (signatures, balance, allowance) before storing them.
    *   Persists orders in a PostgreSQL database.
    *   Provides an API for solvers to query open orders.
*   **`autopilot`**: The protocol driver.
    *   "Cuts" new auctions by selecting orders for the next settlement batch.
    *   Determines auction parameters.
    *   Shares the PostgreSQL database with the `orderbook`.
*   **`driver`**: A framework/sidecar for Solvers.
    *   Simplifies solver development by handling common tasks like liquidity collection, settlement encoding, and transaction publishing.
    *   Allows "Solver Engines" to focus purely on the core logic of finding optimal trade paths.
*   **`solvers`**: Contains solver implementations and logic.

### Other Crates

*   **`alerter`**: Monitoring tool for the orderbook.
*   **`contracts`**: Rust bindings for Ethereum smart contracts (via `ethcontract-rs`).
*   **`database`**: Shared database logic and migrations.
*   **`e2e`**: End-to-end test suite.
*   **`model`**: Data models and serialization for the API.
*   **`shared`**: Common utilities shared across services.

## Building and Running

The project uses `cargo` for building and `just` as a command runner for common tasks. Docker Compose is used for local development environments.

### Local Development (Playground)

The `playground` directory contains a Docker Compose setup for running the services locally.

```bash
# Run the full stack (or specific services like: driver autopilot)
docker compose -f playground/docker-compose.fork.yml up --build
```

### Common Commands (`just`)

Use `just` to run predefined tasks:

*   **Unit Tests:** `just test-unit` (uses `cargo nextest`)
*   **Doc Tests:** `just test-doc`
*   **Database Tests:** `just test-db` (requires a running Postgres instance)
*   **E2E Tests (Local Node):** `just test-e2e-local`
*   **E2E Tests (Forked Node):** `just test-e2e-forked` (requires `FORK_MAINNET_URL`)
*   **Linting:** `just clippy`
*   **Formatting:** `just fmt`

### Manual Database Setup

If running tests outside of `just` or the playground:

```bash
# Start a local Postgres instance
docker compose up -d
```

## Development Conventions

*   **Language:** Rust (latest stable).
*   **Formatting:** Enforced via `rustfmt`. Run `just fmt` before committing.
*   **Linting:** Enforced via `clippy`. Run `just clippy` to ensure code quality.
*   **Testing:** High test coverage is expected. Use `cargo nextest` for faster execution.
    *   **Flaky Tests:** Flaky tests in CI can be debugged using the `run-flaky-test` GitHub Action.
*   **Database:** Uses `sqlx` for interaction and `flyway` (implied by `database/conf/flyway.conf`) or custom logic for migrations.
*   **Architecture:** Microservices sharing a database (`orderbook`, `autopilot`). Solvers interact via API.
*   **Logging:** Uses `tracing`. Log filters can be dynamically changed at runtime via UNIX sockets in `/tmp/`.
