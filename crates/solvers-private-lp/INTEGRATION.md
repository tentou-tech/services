# Nyne Protocol Solver Integration Guide

## 1. Overview

### 1.1 What is Nyne Protocol?

Nyne Protocol is a decentralized exchange (DEX) aggregation protocol built on the Hyper EVM network, designed to optimize trade execution by aggregating liquidity from multiple sources. Similar to CoW Protocol, Nyne Protocol focuses on **batch auction mechanisms**, but with a specialized emphasis on integrating **external liquidity providers** such as:

- Market Makers (MM)
- Centralized Exchanges (CEX)
- Proprietary trading systems
- Private liquidity pools
- Other off-chain liquidity sources

### 1.2 Key Architecture Components

The Nyne Protocol ecosystem consists of three primary services:

1. **Orderbook Service**: Handles user order placement, quote generation, and order validation
2. **Autopilot Service**: Manages auction creation, solution collection, winner selection, and settlement coordination
3. **Driver Service**: Acts as an intermediary between Autopilot and Solver services, handling solution encoding and on-chain transaction submission

### 1.3 Solver Role in Nyne Protocol

As an external solver, your service provides:

- **Price Quotes**: Real-time pricing information for token pairs based on your private liquidity
- **Auction Solutions**: Optimal settlement proposals that maximize user surplus while utilizing your liquidity
- **On-Chain Execution**: Transaction submission and settlement when your solution wins an auction

Solvers compete in periodic auctions to provide the best execution for user orders, with rewards based on the quality of their solutions.

### 1.4 Network Information

- **Network**: Hyper EVM
- **Chain ID**: 998 (Testnet) / 999 (Mainnet)
- **Settlement Contract**: Deployed on Hyper EVM network
- **Vault Contract**: Required for private liquidity integration

---

## 2. Preparation from Solver

### 2.1 Solver Service Implementation

#### 2.1.1 Base Template: `solvers-private-lp`

Nyne Protocol provides a foundational template (`solvers-private-lp`) that serves as the starting point for building your custom solver service. This template includes:

- **Protocol Driver Integration**: Standardized interfaces for receiving auction requests
- **Web Server Framework**: HTTP endpoints for `/solve`, `/notify`, and `/healthz`
- **Solution Construction Utilities**: Helpers for building settlement solutions
- **Vault Integration Pattern**: Template for vault-based token swaps

#### 2.1.2 Implementation Requirements

To build your solver service, refer to the detailed implementation guide in [README.md](README.md) which provides step-by-step instructions for:

- Cloning and customizing the template
- Modifying core files (`solver.rs`, `api.rs`, `config.rs`)
- Implementing your trading strategy
- Connecting to external liquidity providers

#### 2.1.3 Service Availability Requirements

Your solver service **MUST**:

- Run 24/7 with high availability (99.9%+ uptime recommended)
- Respond to `/solve` requests within the auction deadline (typically 5-10 seconds)
- Maintain low latency (< 100ms response time for quotes)
- Implement proper health checks via `/healthz` endpoint
- Handle graceful degradation when external liquidity sources are unavailable

#### 2.1.4 Service Endpoints

Your solver service must expose the following HTTP endpoints:

- **`POST /solve`**: Receives auction requests and returns solution proposals
- **`POST /notify`**: Receives settlement notifications (optional)
- **`GET /healthz`**: Health check endpoint for monitoring

### 2.2 Vault Contract Setup

#### 2.2.1 Vault Contract Requirements

For private liquidity integration, you must deploy and maintain a **Vault Contract** on the Hyper EVM network. This contract:

- Holds sufficient balances of all tokens you support for swapping
- Implements an `exchange` function that handles token transfers
- Validates solver signatures to authorize trades
- Manages token approvals and transfers securely

#### 2.2.2 Vault Funding Requirements

Your vault must maintain adequate balances for:

- **All Supported Tokens**: Every token pair you offer quotes for
- **Expected Trade Volume**: Sufficient liquidity to handle peak trading volumes
- **Buffer for Volatility**: Additional reserves to handle price movements between quote and execution

**Example**: If you support WETH/USDC swaps with a maximum trade size of 10 ETH, your vault should hold:
- Minimum 10+ ETH (WETH)
- Minimum equivalent USDC value (e.g., 30,000+ USDC at current prices)
- Additional 20-30% buffer for safety

#### 2.2.3 Vault Contract Address

You must provide your vault contract address during solver registration. This address will be:
- Used in settlement interactions
- Whitelisted by the Nyne Protocol settlement contract
- Monitored for balance adequacy

### 2.3 Solver Address Whitelisting

#### 2.3.1 Solver Account Setup

You must provide a **solver account address** (EOA) that will be whitelisted with the Nyne Protocol. This address:

- Is used to sign settlement transactions
- Must be whitelisted in the settlement contract

#### 2.3.2 Whitelisting Process

1. **Generate or Use Existing Address**: Create a new HyperEVM address or use an existing one
2. **Submit Whitelist Request**: Contact Nyne Protocol team with:
   - Solver account address
   - Vault contract address
   - Supported token list
   - Expected trading volume
3. **Wait for Approval**: Nyne Protocol will whitelist your address in the settlement contract
4. **Verify Whitelist Status**: Confirm your address is whitelisted before going live

#### 2.3.3 Address Security

**CRITICAL**: Your solver account private key must be:
- Stored securely (never commit to version control)
- Backed up with proper disaster recovery procedures
- Managed with appropriate access controls

---

## 3. Flow Integration

### 3.1 Flow: Get Quote

#### 3.1.1 Overview

The quote flow allows users to obtain price estimates before placing orders. This flow involves the Orderbook service querying your solver (via the Driver) for pricing information.

#### 3.1.2 Sequence Diagram

```
User → Orderbook → Driver → Solver Service → External Liquidity Provider
  ↑                                                    ↓
  └──────────────────────────────────────────────────┘
```

#### 3.1.3 Detailed Flow

**Step 1: User Requests Quote**
- User submits a quote request to Orderbook API: `GET /api/v1/quote`
- Request includes: `sellToken`, `buyToken`, `kind` (buy/sell), `amount`, `deadline`

**Step 2: Orderbook Queries Driver**
- Orderbook identifies your solver from `PRICE_ESTIMATION_DRIVERS` configuration
- Orderbook sends HTTP GET request to Driver: `GET /{solver-name}/quote?{params}`
- Format: `baseline|http://driver/baseline` or `your-solver|http://driver/your-solver`

**Step 3: Driver Processes Quote Request**
- Driver receives quote request and validates parameters
- Driver creates a "fake auction" with a single order matching the quote request
- Driver calls your solver service: `POST /solve` with the fake auction

**Step 4: Solver Service Generates Quote**
- Your solver receives the auction request
- Query your external liquidity provider (MM, CEX, etc.) for current prices
- Calculate optimal `buyAmount` for given `sellAmount` (or vice versa)
- Construct solution with:
  - Executed amounts (`executedSell`, `executedBuy`)
  - Interactions (vault-based swap interactions)
  - Gas estimate
  - Solver address (your vault address)

**Step 5: Solution Encoding**
- Your solver returns a `Solution` object with:
  ```json
  {
    "solutionId": 1,
    "trades": [{
      "orderUid": "...",
      "executedSell": "1000000000000000000",
      "executedBuy": "3000000000000000000"
    }],
    "interactions": [{
      "target": "0x...", // Token contract address
      "value": "0",
      "callData": "0x..." // Encoded transferFrom/transfer calls
    }],
    "solver": "0x...",
    "gas": 150000
  }
  ```

**Step 6: Driver Encodes Quote Response**
- Driver converts solution to `QuoteResponse` format
- Includes interactions for on-chain execution
- Returns quote to Orderbook

**Step 7: Orderbook Returns Quote to User**
- Orderbook may verify quote on-chain (optional)
- Returns final quote response to user with:
  - Buy/sell amounts
  - Estimated gas cost
  - Interactions for execution

### 3.2 Flow: Auction

#### 3.2.1 Overview

The auction flow is the core mechanism where solvers compete to provide the best execution for user orders. Autopilot creates periodic auctions, collects solutions from all solvers, selects winners, and coordinates settlement.

#### 3.2.2 Sequence Diagram

```
Autopilot → Driver → Solver Service → External Liquidity Provider
    ↑                                           ↓
    └───────────────────────────────────────────┘
    ↓
Winner Selection
    ↓
Settlement Execution
```

#### 3.2.3 Detailed Flow

**Step 1: Auction Creation**
- Autopilot collects pending user orders from the database
- Creates an `Auction` object containing:
  - `id`: Unique auction identifier
  - `orders`: List of solvable orders
  - `tokens`: Token information (addresses, decimals, etc.)
  - `deadline`: Time by which solvers must respond
  - `block`: Current blockchain block number

**Step 2: Autopilot Queries All Solvers**
- Autopilot sends `POST /solve` requests to all configured drivers
- Format: `DRIVERS=baseline|http://driver/baseline|{SOLVER_ACCOUNT},your-solver|http://driver/your-solver|{YOUR_SOLVER_ACCOUNT}`
- Each driver forwards the request to its corresponding solver service
- Requests are sent in parallel to minimize latency

**Step 3: Driver Forwards to Solver**
- Driver receives auction request from Autopilot
- Driver validates auction structure
- Driver calls your solver service: `POST /solve` with `SolveRequest`:
  ```json
  {
    "id": 12345,
    "orders": [...],
    "tokens": {...},
    "deadline": "2024-01-01T12:00:00Z"
  }
  ```

**Step 4: Solver Service Processes Auction**
- Your solver receives the auction request
- For each order in the auction:
  - Query your external liquidity provider for current prices
  - Calculate optimal execution amounts
  - Construct trades with vault-based interactions
  - Calculate solution score (objective value)

**Step 5: Solution Generation**
- Your solver generates one or more `Solution` objects:
  ```json
  {
    "solutionId": 1,
    "trades": [
      {
        "orderUid": "0x...",
        "executedSell": "1000000000000000000",
        "executedBuy": "3000000000000000000"
      }
    ],
    "interactions": [...],
    "score": "500000000000000000", // Objective value
    "solver": "0x...", // Your vault address
    "gas": 200000
  }
  ```

**Step 6: Solution Scoring**
- Each solution includes a `score` representing the objective value
- Score typically represents:
  - User surplus (difference between limit price and execution price)
  - Negative gas costs
  - Other optimization factors
- Higher scores indicate better solutions

**Step 7: Solver Returns Solutions**
- Your solver returns `SolveResponse` with array of solutions:
  ```json
  {
    "solutions": [
      {
        "solutionId": 1,
        "trades": [...],
        "interactions": [...],
        "score": "...",
        "solver": "0x...",
        "gas": 200000
      }
    ]
  }
  ```

**Step 8: Driver Processes Solutions**
- Driver receives solutions from your solver
- Driver validates solution structure
- Driver encodes solutions for on-chain execution
- Driver returns solutions to Autopilot

**Step 9: Autopilot Collects All Solutions**
- Autopilot waits for responses from all solvers (or timeout)
- Collects all valid solutions
- Filters solutions that don't meet requirements (e.g., invalid signatures, insufficient liquidity)

**Step 10: Winner Selection**
- Autopilot ranks solutions by score (highest first)
- Applies fairness thresholds if configured
- Selects winning solutions that maximize total objective value
- Multiple solutions can win if they don't conflict (different orders)

**Step 11: Post-Processing**
- Autopilot saves winning solutions to database
- Records auction participants and scores
- Prepares for settlement execution

### 3.3 Flow: Settle (If Win Auction)

#### 3.3.1 Overview

When your solution wins an auction, Autopilot calls your solver (via Driver) to execute the settlement on-chain. This flow handles the actual token transfers and trade execution.

#### 3.3.2 Sequence Diagram

```
Autopilot → Driver → Solver Service
    ↑                    ↓
    └────────────────────┘
    ↓
On-Chain Settlement Transaction
    ↓
Vault Contract Execution
    ↓
Token Transfers Complete
```

#### 3.3.3 Detailed Flow

**Step 1: Winner Notification**
- Autopilot identifies your solution as a winner
- Autopilot calls Driver: `POST /{solver-name}/settle` with `SettleRequest`:
  ```json
  {
    "solutionId": 1,
    "auctionId": 12345,
    "submissionDeadlineLatestBlock": 1000000
  }
  ```

**Step 2: Driver Prepares Settlement Transaction**
- Driver constructs the settlement transaction:
  - Encodes settlement calldata with your solution's interactions
  - Sets appropriate gas price (considering deadline)
  - Signs transaction with solver account private key
  - Creates both internalized and uninternalized versions

**Step 3: Transaction Submission**
- Driver submits transaction to Hyper EVM network
- Transaction calls the settlement contract's `settle()` function
- Settlement contract executes your solution's interactions:
  1. Processes user token approvals
  2. Executes vault-based swap interactions
  3. Transfers tokens between users and vault
  4. Records settlement in on-chain events

**Step 4: Vault Contract Execution**
- Settlement contract calls your vault's `exchange()` function (or token transfer functions)
- Vault validates solver signature
- Vault transfers `sellToken` from user to vault
- Vault transfers `buyToken` from vault to user
- Vault updates internal balances

**Step 5: Transaction Confirmation**
- Driver waits for transaction to be mined
- Driver monitors blockchain for settlement transaction
- Once confirmed, Driver notifies Autopilot of successful settlement

**Step 6: Settlement Observation**
- Autopilot's settlement observer detects the on-chain settlement
- Autopilot validates settlement matches the winning solution
- Autopilot updates database with settlement results
- Autopilot records solver performance metrics

#### 3.3.4 Error Handling

If settlement fails:

- **Transaction Reverts**: Settlement contract reverts, no tokens are transferred
- **Driver Retries**: Driver may retry with higher gas price if deadline allows
- **Autopilot Handles**: Autopilot marks settlement as failed and may select alternative solutions

---

## 4. Support and Resources

### 4.1 Documentation

- Nyne Protocol Documentation: [Link TBD]
- Solver Template: `crates/solvers-private-lp/`

### 4.2 Contact

For integration support, contact the Nyne Protocol team:
- Email: [TBD]
- Discord: [TBD]
- GitHub Issues: [TBD]

---

## Appendix A: Example Solver Service

See `crates/solvers-private-lp/` for a complete example implementation.

## Appendix B: API Reference

### Solve Request

```json
{
  "id": 12345,
  "orders": [
    {
      "uid": "0x...",
      "sellToken": "0x...",
      "buyToken": "0x...",
      "sellAmount": "1000000000000000000",
      "buyAmount": "3000000000000000000",
      "validTo": 1234567890,
      "kind": "sell"
    }
  ],
  "tokens": {
    "0x...": {
      "address": "0x...",
      "decimals": 18,
      "symbol": "WETH"
    }
  },
  "deadline": "2024-01-01T12:00:00Z"
}
```

### Solve Response

```json
{
  "solutions": [
    {
      "solutionId": 1,
      "trades": [
        {
          "orderUid": "0x...",
          "executedSell": "1000000000000000000",
          "executedBuy": "3000000000000000000"
        }
      ],
      "interactions": [
        {
          "target": "0x...",
          "value": "0",
          "callData": "0x..."
        }
      ],
      "score": "500000000000000000",
      "solver": "0x...",
      "gas": 200000
    }
  ]
}
```

---

**Document Version**: 1.0  
**Last Updated**: 2025-12-11  
**Status**: Draft

