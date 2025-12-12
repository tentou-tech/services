# `solvers-private-lp` - Nyne Protocol Solver Template

This crate serves as a foundational template for building custom Nyne Protocol solvers that leverage private liquidity sources. It's designed to provide a clear starting point for developers looking to integrate their own trading strategies and external liquidity providers with the Nyne Protocol ecosystem.

## Table of Contents

- [Key Features](#key-features)
- [Architecture](#architecture)
- [Getting Started: Building Your Own Solver](#getting-started-building-your-own-solver)
- [Project Structure Overview](#project-structure-overview)
- [Building and Running Locally](#building-and-running-locally)
- [Integration with Nyne Protocol Driver](#integration-with-nyne-protocol-driver)
- [Testing](#testing)
- [Next Steps for Customization](#next-steps-for-customization)

## Key Features

*   **Nyne Protocol Driver Integration:** Seamlessly integrates with the Nyne Protocol driver framework, allowing your solver to receive auction data and submit solutions.
*   **Private Liquidity Placeholder:** Includes an abstract interface (`api.rs`) and mock pricing (`fake_prices` in `solver.rs`) to guide the integration of your specific private liquidity source.
*   **Web Server:** Runs as a web server, exposing a `/solve` endpoint to receive auction requests from the Nyne Protocol driver.
*   **`solvers-dto` Usage:** Utilizes the `solvers-dto` crate for standardized data transfer objects, ensuring compatibility with the broader Nyne Protocol architecture.

## Architecture

The solver operates as a web service that responds to auction requests from the Nyne Protocol Driver. The high-level data flow is as follows:

1.  **Auction Ingestion (`main.rs`)**:
    *   The service exposes a standard HTTP endpoint (`/solve`).
    *   When the Driver sends a `BatchAuction` JSON payload, the `main` module deserializes it and passes it to the solver instance.

2.  **Solver Logic (`solver.rs`)**:
    *   The `HyperLiquidSolver::solve` method is the core orchestrator.
    *   It analyzes the auction to identify relevant orders.
    *   It queries external market data or liquidity availability via the `HyperLiquidApi`.
    *   Based on the orders and external liquidity, it computes an optimal settlement (matching orders to your private liquidity).

3.  **External Liquidity Integration (`api.rs`)**:
    *   This module acts as an adapter. It transforms internal requests for price or quantity into specific API calls required by your private liquidity source (e.g., a CEX API, a market maker bot, or an on-chain pool).

4.  **Solution Generation**:
    *   If a profitable match is found, the solver constructs a `Solution`.
    *   This solution includes the matched orders and a "calldata" interaction. This interaction defines how the settlement contract should swap tokens with your liquidity source on-chain.


## Getting Started: Building Your Own Solver

To create your own solver based on this template, follow these steps:

*   For a concrete example of a solver built using this template, see the [Hypervault Solver Flow](HYPERVAULT_FLOW.md) documentation.

1.  **Clone the Repository:** If you haven't already, clone the main Nyne Protocol Services repository.

    ```bash
    git clone https://github.com/cowprotocol/services.git
    cd services
    ```

2.  **Copy and Rename the Crate:** Create a new solver crate by copying `solvers-private-lp` and renaming it.

    ```bash
    cp -r crates/solvers-private-lp crates/your-new-solver
    # Update Cargo.toml in your new solver directory
    # e.g., change `name = "solvers-private-lp"` to `name = "your-new-solver"`
    # Also, update the workspace members in the root Cargo.toml if necessary.
    ```

3.  **Modify Core Files:**

    *   **`src/solver.rs`**: This file contains the primary solving logic. You will need to:
        *   Replace the `fake_prices` function with your actual pricing mechanism, querying your private liquidity source.
        *   Implement your custom trading strategy within the `solve` method to determine the optimal solution for a given auction.
        *   Generate appropriate `Solution` objects, potentially including custom smart contract interactions.

    *   **`src/api.rs`**: This file defines the `HyperLiquidApi` trait, which abstracts interaction with an external liquidity provider. You will need to:
        *   Implement the `HyperLiquidApi` trait (or a similar custom trait) to connect to your specific private liquidity API (e.g., Hyperliquid, a proprietary exchange).
        *   Handle API requests, error management, and data parsing.

    *   **`src/config.rs`**: Customize the `Config` and `FileConfig` structs to include any specific configuration parameters required by your solver, such as API keys, contract addresses, or liquidity provider endpoints.

    *   **`src/main.rs`**: While generally stable, you might need to adjust the entry point if your solver requires different setup or additional background tasks.

4.  **Update `Cargo.toml`:** Ensure the `Cargo.toml` file of your new solver crate reflects its new name and any new dependencies your custom logic requires.

## Project Structure Overview

*   **`src/main.rs`**: The application's entry point. Sets up the web server and routes incoming `/solve` requests to your solver logic.
*   **`src/solver.rs`**: Contains the core `HyperLiquidSolver` implementation, responsible for processing auctions, interacting with liquidity, and formulating solutions.
*   **`src/api.rs`**: Defines the trait for external liquidity provider interaction. This is where you'll implement the specifics of communicating with your private liquidity source.
*   **`src/config.rs`**: Handles the loading and parsing of solver-specific configuration from files or environment variables.
*   **`Cargo.toml`**: Manages crate dependencies and metadata.

## Building and Running Locally

To build your solver:

```bash
# From the root of the services directory
cargo build --release -p your-new-solver
```

To run your solver (replace `your-new-solver` with your crate name):

```bash
# From the root of the services directory
# You might need to provide a configuration file, e.g., using a custom `--config` flag
cargo run --release -p your-new-solver -- --config crates/your-new-solver/config/your-solver.toml
```

Refer to the main `GEMINI.md` or `Justfile` in the root of the repository for general project build and test commands.

## Integration with Nyne Protocol Driver

Your solver will run as an independent service. The Nyne Protocol `driver` will send auction requests to your solver's `/solve` endpoint. Your solver must respond with a `Solution` object (defined in `solvers-dto`) that the driver can then interpret and execute on-chain.

## Testing

It is highly recommended to write comprehensive unit and integration tests for your custom solver logic. You can use `cargo test` to run tests within your crate.

```bash
cargo test -p your-new-solver
```

Consider also setting up end-to-end tests within the `e2e` crate to verify your solver's behavior in a more complete system context.

## Next Steps for Customization

*   **Implement Your Strategy:** Develop and refine your unique trading algorithm within `src/solver.rs`.
*   **Connect to Real Liquidity:** Replace the mock `HyperLiquidApi` implementation with a robust client for your chosen private liquidity provider.
*   **Optimize Performance:** Consider performance implications of your pricing queries and solution generation.
*   **Error Handling and Monitoring:** Implement comprehensive error handling, logging, and metrics to ensure your solver is reliable in production.