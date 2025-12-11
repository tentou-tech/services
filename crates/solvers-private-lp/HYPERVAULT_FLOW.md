# Hypervault Solver Flow

**Hypervault** is a specific deployment instance of the `solvers-private-lp` solver template. It is designed to act as a Private Market Maker (PMM) or Liquidity Provider (LP) within the CoW Protocol ecosystem.

This document outlines the operational flow of Hypervault, detailing how it receives tasks, processes them, and submits solutions.

## Table of Contents

- [System Overview](#system-overview)
- [Detailed Flow](#detailed-flow)
    - [1. Auction Ingestion](#1-auction-ingestion)
    - [2. Internal Processing & Strategy](#2-internal-processing--strategy)
    - [3. Solution Construction](#3-solution-construction)
    - [4. Response to Driver](#4-response-to-driver)
    - [5. Settlement Execution (Post-Flow)](#5-settlement-execution-post-flow)
- [Configuration Context](#configuration-context)
- [Running Hypervault Locally](#running-hypervault-locally)

## System Overview

Hypervault is a system composed of two main components:

1.  **HTTP Service**: This is the `solvers-private-lp` binary running as a standalone HTTP service. It interacts primarily with the CoW Protocol **Driver**, which acts as a sidecar or proxy.
    *   **Service**: `solvers-private-lp` (binary)
    *   **Role**: Solver / Private Liquidity Provider
    *   **Configuration**: Defined in `configs/hypervault/hypervault.toml`.

2.  **Contract Vault**: This is a smart contract deployed on-chain that holds and manages the liquidity for Hypervault. It must implement the `IVault.sol` interface, typically found at https://github.com/tentou-tech/evm-core-vault/blob/main/contracts/IVault.sol.
    *   **Key Function**: The primary function of interest is `function exchange(bytes32 orderUid, address tokenIn, uint256 amountIn, address tokenOut, uint256 amountOut, uint32 validTo, uint256 nonce, bytes[] calldata signatures) external payable;`.
    *   **Purpose**: This function enables the transfer of tokens from the vault to the user and from the user to the vault, facilitating atomic swaps within the CoW Protocol settlement process.
    *   **Requirement**: The `vault-contract-address` specified in the configuration must point to this smart contract, and Hypervault must have the necessary permissions (e.g., allowances or ownership) to manage the liquidity held within it.

## Detailed Flow

### 1. Auction Ingestion
The process begins when the CoW Protocol Driver sends a matching task to Hypervault.

*   **Trigger**: The Driver POSTs a JSON payload to Hypervault's `/solve` endpoint.
*   **Payload (`BatchAuction`)**: Contains information about the current batch, including:
    *   **Orders**: A list of user orders (limit orders) to be filled.
    *   **Prices**: Reference market prices (optional, depending on setup).
    *   **Metadata**: Block number, timestamps, etc.

### 2. Internal Processing & Strategy
Upon receiving the auction, Hypervault executes its internal solving logic (implemented in `src/solver.rs`).

1.  **Filtering**: The solver iterates through the provided orders to identify those it is interested in filling. This filtering can be based on:
    *   Token pairs (e.g., matching only tokens held in the Vault).
    *   Order size or type.
    *   Profitability checks.

2.  **Liquidity Provisioning (Pricing)**:
    *   For the selected orders, Hypervault determines the execution price.
    *   *Note*: In the default template, this uses a mock `fake_prices` function. In a production Hypervault setup, this step would involve querying an external pricing engine, a CEX API, or an on-chain vault state to get real-time quotes.

3.  **Matching**:
    *   The solver "matches" the user's order against its own private liquidity.
    *   It calculates the amounts to be swapped to satisfy the user's order while ensuring the trade is profitable (or at least valid) for the LP.

### 3. Solution Construction
If a valid match is found, Hypervault constructs a `Solution` object.

*   **Settlement Interaction**: The solution includes "calldata" – the specific bytecode instructions that the CoW Protocol Settlement contract will execute.
*   **Vault Interaction**: Specifically for Hypervault, this calldata instructs the Settlement contract to pull funds from the **Vault Contract** (configured via `vault-contract-address`) to pay the user, and sends the user's sell tokens back to the Vault.
*   **Signatures/Authorization**: To ensure the integrity and authenticity of the solution, the solver's private key (configured via the `account` field in `hypervault.toml`) is used to sign a specific message. This message typically includes a hash of the `settlement_calldata` and other relevant trade parameters, authorizing the CoW Protocol Settlement contract to execute the proposed trade on behalf of the solver/vault.

### 4. Response to Driver
Hypervault sends the constructed `Solution` back to the Driver as an HTTP response.

*   **Format**: JSON (following the `solvers-dto` schema).
*   **Content**: 
    *   Matched orders (IDs and executed amounts).
    *   The internal clearing prices.
    *   The interactions (calldata) described above.

### 5. Settlement Execution (Post-Flow)
This step happens outside of Hypervault but is the direct result of its output.

1.  The Driver receives the solution.
2.  It simulates the transaction to verify correctness and gas usage.
3.  If the solution is the winner (provides the best surplus for the user), the Driver submits the transaction to the blockchain.
4.  The transaction executes:
    *   User funds move to the Vault.
    *   Vault funds move to the User.
    *   The trade is settled atomically.

## Configuration Context

The behavior of Hypervault is governed by `configs/hypervault/hypervault.toml`:

*   **`chain-id`**: Ensures logic matches the target network.
*   **`settlement-contract-address`**: The address of the CoW Protocol settlement contract.
*   **`vault-contract-address`**: The specific smart contract holding the LP's funds.
*   **`account`**: The private key used to sign transactions or authenticate.

## Running Hypervault Locally

There are two primary ways to run Hypervault locally:

### 1. With Docker Compose (Recommended)

This method utilizes the project's `playground/` Docker Compose setup for a fully integrated environment.

1.  **Navigate to playground:**
    ```bash
    cd playground
    ```
2.  **Start Hypervault:**
    ```bash
    docker compose -f docker-compose.fork.yml up --build hypervault
    ```
    *(Use `docker-compose.fork.yml` for mainnet forking, `docker-compose.yml` for a simpler local network.)*
3.  **Verify:** Check logs for confirmation: `docker compose logs hypervault`.

### 2. Without Docker (`cargo run`)

This method runs Hypervault directly, requiring a local Ethereum node and manual configuration.

1.  **Create Config:** Copy `crates/solvers-private-lp/example_config.toml` to `configs/hypervault/local_hypervault.toml` and modify as needed.
2.  **Run Solver:** From the `services` root directory:
    ```bash
    ADDR=127.0.0.1:9001 cargo run --bin solvers-private-lp -- --config configs/hypervault/local_hypervault.toml
    ```
    *(Adjust `ADDR` if your Driver expects a different endpoint.)*
3.  **Verify:** Check console output for confirmation.

For more details on the `playground` environment, refer to `playground/README.md`.
