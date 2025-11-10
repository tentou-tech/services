# CoW Protocol Services Architecture

## High-Level System Overview

```mermaid
graph TB
    subgraph "User Layer"
        UI[Web UI / Frontend]
        Trader[Traders]
        Bot[Trading Bots]
    end

    subgraph "Protocol Layer - CoW Services"
        subgraph "Orderbook Service"
            API[REST API<br/>Order Submission<br/>Order Queries]
            Validation[Order Validation<br/>Balance Checks<br/>Signature Verification]
            DB[(PostgreSQL<br/>Orders, Quotes,<br/>Fee Policies)]
        end

        subgraph "Autopilot Service"
            Leader[Leader Election]
            AuctionEngine[Auction Engine<br/>Cuts Auctions<br/>Manages Lifecycle]
            SolvableCache[Solvable Orders Cache<br/>Filters & Validates<br/>Price Estimation]
            Competition[Competition Manager<br/>Winner Selection<br/>Reference Scores]
        end

        subgraph "Driver Services"
            D1[Driver 1<br/>Solver A]
            D2[Driver 2<br/>Solver B]
            D3[Driver 3<br/>Solver N]
        end
    end

    subgraph "Solver Layer"
        subgraph "Solver Engines"
            SE1[Baseline Solver<br/>DEX Routing]
            SE2[RFQ Solver<br/>Market Maker Quotes]
            SE3[Specialized Solver<br/>Custom Logic]
        end
    end

    subgraph "Blockchain Layer"
        Settlement[GPv2Settlement<br/>Smart Contract]
        DEX[DEX Liquidity<br/>Uniswap, Curve,<br/>Balancer, etc.]
        Tokens[ERC20 Tokens]
        Oracle[Chainalysis Oracle<br/>Ban Detection]
    end

    subgraph "External Services"
        S3[AWS S3<br/>Audit Trail]
        Metrics[Prometheus<br/>Grafana]
        CEX[CEX Price Feeds<br/>Binance, Coinbase]
    end

    %% User interactions
    UI --> API
    Trader --> API
    Bot --> API

    %% Orderbook flow
    API --> Validation
    Validation --> DB
    API --> DB

    %% Autopilot flow
    DB -.Read Orders.-> SolvableCache
    Leader --> AuctionEngine
    AuctionEngine --> SolvableCache
    SolvableCache --> Competition
    
    %% Solver competition
    AuctionEngine -->|Auction Data| D1
    AuctionEngine -->|Auction Data| D2
    AuctionEngine -->|Auction Data| D3
    
    D1 -->|/solve| SE1
    D2 -->|/solve| SE2
    D3 -->|/solve| SE3
    
    SE1 -->|Solutions| D1
    SE2 -->|Solutions| D2
    SE3 -->|Solutions| D3
    
    D1 -->|Best Solution| Competition
    D2 -->|Best Solution| Competition
    D3 -->|Best Solution| Competition
    
    Competition -->|Winner Selected| D1
    Competition -.Store Results.-> DB
    Competition -.Audit.-> S3
    
    %% Settlement
    D1 -->|/settle - Execute| Settlement
    
    %% Blockchain interactions
    Validation -.Check Balances.-> Tokens
    Validation -.Check Bans.-> Oracle
    SE1 -.Query Liquidity.-> DEX
    Settlement --> Tokens
    
    %% External services
    SolvableCache -.Price Feeds.-> CEX
    AuctionEngine -.Metrics.-> Metrics
    Competition -.Metrics.-> Metrics

    style API fill:#e1f5ff
    style AuctionEngine fill:#ffe1e1
    style Competition fill:#ffe1e1
    style Settlement fill:#e1ffe1
    style DB fill:#fff9e1
```

---

## Detailed Component Architecture

### 1. Orderbook Service Flow

```mermaid
sequenceDiagram
    participant User
    participant API as Orderbook API
    participant Validation as Validator
    participant DB as PostgreSQL
    participant Blockchain as Ethereum

    User->>API: POST /api/v1/orders
    API->>Validation: Validate Order
    Validation->>Blockchain: Check Token Balance
    Validation->>Blockchain: Verify Signature
    Validation->>Blockchain: Check User Not Banned
    Validation-->>API: Validation Result
    
    alt Order Valid
        API->>DB: Store Order
        DB-->>API: Order ID
        API-->>User: 201 Created (Order UID)
    else Order Invalid
        API-->>User: 400 Bad Request
    end
    
    User->>API: GET /api/v1/orders/{uid}
    API->>DB: Query Order
    DB-->>API: Order Details
    API-->>User: Order Status
```

### 2. Autopilot Auction Cycle

```mermaid
sequenceDiagram
    participant Block as Block Stream
    participant Autopilot
    participant Cache as Solvable Orders Cache
    participant DB as PostgreSQL
    participant Drivers as Drivers (Multiple)
    participant Solvers as Solver Engines
    participant Settlement as Settlement Contract

    loop Every Block
        Block->>Autopilot: New Block Event
        
        Autopilot->>Autopilot: Leader Election Check
        
        alt Is Leader
            Autopilot->>Cache: Update Cache
            Cache->>DB: Fetch Orders
            Cache->>Cache: Filter Invalid Orders
            Cache->>Cache: Estimate Native Prices
            Cache-->>Autopilot: Solvable Orders
            
            Autopilot->>Autopilot: Cut Auction
            Autopilot->>DB: Store Auction (ID)
            
            par Parallel Solve Requests
                Autopilot->>Drivers: POST /solve (Auction)
                Drivers->>Solvers: Forward Auction
                Solvers->>Solvers: Generate Solutions
                Solvers-->>Drivers: Solutions
                Drivers-->>Autopilot: Best Solution
            end
            
            Autopilot->>Autopilot: Arbitrate Winners
            Autopilot->>Autopilot: Compute Reference Scores
            Autopilot->>DB: Save Competition Results
            
            loop For Each Winner
                Autopilot->>Drivers: POST /settle (Solution ID)
                Drivers->>Settlement: Submit Transaction
                Settlement-->>Drivers: Tx Hash
                Drivers-->>Autopilot: Settlement Confirmed
            end
        else Not Leader
            Autopilot->>Autopilot: Sleep & Wait
        end
    end
```

### 3. Driver & Solver Engine Interaction

```mermaid
graph TB
    subgraph "Driver (Colocated with Solver)"
        subgraph "Driver Components"
            API[HTTP API<br/>/solve, /quote, /settle]
            Encoder[Settlement Encoder<br/>Converts to Calldata]
            Submitter[Transaction Submitter<br/>Gas Management<br/>MEV Protection]
            Liquidity[Liquidity Collector<br/>Uniswap, Balancer, etc.]
        end
        
        subgraph "Solver Engine (Your Code)"
            Logic[Solution Logic<br/>Path Finding<br/>Order Matching]
            Scorer[Score Calculator<br/>Surplus Optimization]
        end
    end

    subgraph "External"
        Autopilot[Autopilot]
        Settlement[Settlement Contract]
        DEX[DEX Protocols]
    end

    Autopilot -->|1. POST /solve| API
    API -->|2. Parse Auction| Logic
    
    Liquidity -.Fetch Pools.-> DEX
    Logic -.Query Liquidity.-> Liquidity
    
    Logic -->|3. Generate Solutions| Scorer
    Scorer -->|4. Best Solution| API
    API -->|5. Return Solution| Autopilot
    
    Autopilot -->|6. POST /settle<br/>(if winner)| API
    API -->|7. Encode Settlement| Encoder
    Encoder -->|8. Submit Tx| Submitter
    Submitter -->|9. Execute| Settlement

    style Logic fill:#ffe1e1
    style Scorer fill:#ffe1e1
    style API fill:#e1f5ff
    style Settlement fill:#e1ffe1
```

---

## Data Flow Architecture

### Order Lifecycle State Machine

```mermaid
stateDiagram-v2
    [*] --> Submitted: User creates order
    Submitted --> Invalid: Validation fails
    Submitted --> Open: Validation passes
    
    Open --> Cancelled: User cancels
    Open --> Expired: validTo reached
    Open --> Ready: Included in auction
    
    Ready --> Considered: In competition but not selected
    Ready --> Executing: Winner selected
    
    Executing --> PartiallyFilled: Partial execution
    Executing --> Fulfilled: Full execution
    Executing --> Failed: Settlement reverted
    
    PartiallyFilled --> Ready: Still fillable
    PartiallyFilled --> Fulfilled: Completed
    
    Considered --> Ready: Next auction
    Failed --> Ready: Retry next auction
    
    Cancelled --> [*]
    Expired --> [*]
    Fulfilled --> [*]
    Invalid --> [*]
```

### Database Schema Overview

```mermaid
erDiagram
    ORDERS ||--o{ ORDER_EVENTS : has
    ORDERS ||--o{ TRADES : generates
    ORDERS {
        bytea uid PK
        address owner
        timestamp creation_timestamp
        address sell_token
        address buy_token
        numeric sell_amount
        numeric buy_amount
        int valid_to
        text kind
        boolean partially_fillable
        text class
    }
    
    AUCTIONS ||--o{ ORDERS : contains
    AUCTIONS {
        bigint id PK
        bigint block
        timestamp deadline
        jsonb prices
    }
    
    COMPETITIONS ||--|| AUCTIONS : for
    COMPETITIONS {
        bigint auction_id FK
        jsonb reference_scores
        jsonb participants
        jsonb competition_table
    }
    
    SETTLEMENTS ||--o{ TRADES : contains
    SETTLEMENTS {
        bytea tx_hash PK
        bigint block_number
        address solver
        numeric gas_used
    }
    
    TRADES {
        bigint trade_id PK
        bytea order_uid FK
        bytea tx_hash FK
        numeric sell_amount
        numeric buy_amount
    }
    
    ORDER_EVENTS {
        bigint id PK
        bytea order_uid FK
        timestamp timestamp
        text label
    }
```

---

## Network Architecture

### Production Deployment

```mermaid
graph TB
    subgraph "Load Balancer"
        LB[ALB / Nginx]
    end

    subgraph "Orderbook Cluster (Horizontal Scale)"
        OB1[Orderbook 1]
        OB2[Orderbook 2]
        OB3[Orderbook N]
    end

    subgraph "Autopilot Cluster (Leader Election)"
        AP1[Autopilot 1<br/>⭐ Leader]
        AP2[Autopilot 2<br/>Standby]
        AP3[Autopilot 3<br/>Standby]
    end

    subgraph "Database Layer"
        PG_Primary[(PostgreSQL<br/>Primary)]
        PG_Replica1[(PostgreSQL<br/>Read Replica 1)]
        PG_Replica2[(PostgreSQL<br/>Read Replica 2)]
    end

    subgraph "Solver Infrastructure"
        Driver1[Driver 1 + Solver A]
        Driver2[Driver 2 + Solver B]
        Driver3[Driver 3 + Solver C]
    end

    subgraph "Blockchain Nodes"
        Node1[Ethereum Node 1<br/>Archive]
        Node2[Ethereum Node 2]
        Node3[Ethereum Node 3]
    end

    Users --> LB
    LB --> OB1
    LB --> OB2
    LB --> OB3

    OB1 --> PG_Primary
    OB2 --> PG_Primary
    OB3 --> PG_Primary

    PG_Primary -.Replication.-> PG_Replica1
    PG_Primary -.Replication.-> PG_Replica2

    AP1 --> PG_Primary
    AP2 -.Standby.-> PG_Primary
    AP3 -.Standby.-> PG_Primary

    AP1 --> PG_Replica1
    AP1 --> Driver1
    AP1 --> Driver2
    AP1 --> Driver3

    OB1 --> Node1
    OB2 --> Node2
    AP1 --> Node1
    Driver1 --> Node3
    Driver2 --> Node3
    Driver3 --> Node3

    style AP1 fill:#ffe1e1
    style PG_Primary fill:#fff9e1
    style Driver1 fill:#e1ffe1
```

---

## Component Responsibilities Matrix

| Component | Responsibilities | Scalability | State |
|-----------|-----------------|-------------|-------|
| **Orderbook** | • Order submission<br/>• Order queries<br/>• Fee estimation<br/>• Quote generation | Horizontal (stateless) | Shared DB |
| **Autopilot** | • Auction creation<br/>• Solver coordination<br/>• Winner selection<br/>• Settlement triggering | Single leader (HA standby) | Shared DB + In-memory cache |
| **Driver** | • Liquidity collection<br/>• Solution encoding<br/>• Transaction submission<br/>• Gas optimization | One per solver | Local state |
| **Solver Engine** | • Solution generation<br/>• Route finding<br/>• Order matching | Independent | Stateless |
| **PostgreSQL** | • Order persistence<br/>• Auction history<br/>• Competition results | Primary + Replicas | Persistent |
| **S3** | • Audit trail<br/>• Competition data<br/>• Analytics | Infinite | Persistent |

---

## Key Integration Points

### 1. User → Protocol

```
User Application
    ↓ HTTPS
Orderbook API (:8080)
    ↓ SQL
PostgreSQL Database
```

### 2. Autopilot → Solvers

```
Autopilot
    ↓ HTTP POST /solve
Driver (:9000)
    ↓ HTTP POST /solve
Solver Engine (:8080)
    ↓ Solution
Driver
    ↓ Best Solution
Autopilot
```

### 3. Settlement → Blockchain

```
Driver
    ↓ eth_sendRawTransaction
Ethereum Node
    ↓ Transaction
Settlement Contract
    ↓ Token Transfers
User Wallets
```

---

## Technology Stack Summary

```mermaid
graph LR
    subgraph "Languages"
        Rust[Rust 🦀<br/>Core Services]
        Python[Python 🐍<br/>Solvers/Tools]
        TS[TypeScript<br/>Frontend]
    end

    subgraph "Databases"
        PG[(PostgreSQL<br/>Primary Store)]
        Redis[(Redis<br/>Caching)]
    end

    subgraph "Infrastructure"
        Docker[Docker<br/>Containers]
        K8s[Kubernetes<br/>Orchestration]
        AWS[AWS<br/>Cloud]
    end

    subgraph "Monitoring"
        Prom[Prometheus<br/>Metrics]
        Graf[Grafana<br/>Dashboards]
        Trace[Tracing<br/>OpenTelemetry]
    end

    subgraph "Blockchain"
        Web3[Web3/Alloy<br/>RPC Client]
        Ethers[Ethcontract<br/>Contract Bindings]
    end

    Rust --> PG
    Rust --> Redis
    Rust --> Web3
    Rust --> Ethers
    Rust --> Prom
    Rust --> Trace

    Docker --> K8s
    K8s --> AWS
    Prom --> Graf
```

---

## Performance Characteristics

| Metric | Target | Notes |
|--------|--------|-------|
| **Order Submission** | < 100ms | API response time |
| **Order Validation** | < 500ms | Including blockchain checks |
| **Auction Creation** | 1-3s | Per block |
| **Solver Deadline** | 10-30s | Configurable |
| **Winner Selection** | < 500ms | Arbitration algorithm |
| **Settlement Time** | 12s (1 block) | Ethereum block time |
| **Orders per Auction** | 100-1000+ | Depends on complexity |
| **Concurrent Solvers** | 10-20+ | Parallel execution |

---

## Security Architecture

```mermaid
graph TB
    subgraph "Security Layers"
        subgraph "API Security"
            RateLimit[Rate Limiting]
            CORS[CORS Protection]
            Input[Input Validation]
        end

        subgraph "Order Security"
            SigVerify[Signature Verification<br/>EIP-712, EthSign, PreSign]
            BalCheck[Balance Verification]
            BanCheck[Ban List Check<br/>Chainalysis]
        end

        subgraph "Solver Security"
            Whitelist[Solver Whitelist]
            SolutionVal[Solution Validation]
            FairnessCheck[Fairness Filter]
            ParticipationGuard[Participation Guard]
        end

        subgraph "Settlement Security"
            GasLimit[Gas Limit Checks]
            Slippage[Slippage Protection]
            Deadline[Submission Deadline]
            Revert[Revert Protection]
        end

        subgraph "Infrastructure"
            TLS[TLS/HTTPS]
            KeyMgmt[Key Management<br/>AWS KMS]
            Audit[Audit Logging<br/>S3]
        end
    end

    Users --> RateLimit
    RateLimit --> CORS
    CORS --> Input
    Input --> SigVerify
    SigVerify --> BalCheck
    BalCheck --> BanCheck
    
    Solvers --> Whitelist
    Whitelist --> SolutionVal
    SolutionVal --> FairnessCheck
    FairnessCheck --> ParticipationGuard
    
    Settlement_Execution --> GasLimit
    GasLimit --> Slippage
    Slippage --> Deadline
    Deadline --> Revert
    
    All_Traffic --> TLS
    All_Services --> KeyMgmt
    All_Actions --> Audit

    style SigVerify fill:#ffe1e1
    style FairnessCheck fill:#ffe1e1
    style Audit fill:#fff9e1
```

---

## Summary

**CoW Protocol Services** is a sophisticated multi-component system that:

1. **Collects orders** from users via a horizontally scalable REST API
2. **Validates** orders against blockchain state and ban lists
3. **Creates auctions** periodically (every block) containing solvable orders
4. **Coordinates** a competition between multiple independent solvers
5. **Selects winners** based on surplus maximization and fairness constraints
6. **Executes settlements** on-chain via the GPv2Settlement contract
7. **Maintains** a complete audit trail and analytics in PostgreSQL and S3

The architecture is designed for:
- ✅ **High availability** (leader election, horizontal scaling)
- ✅ **Performance** (caching, parallel processing)
- ✅ **Security** (signature verification, solution validation)
- ✅ **Fairness** (uniform clearing prices, reference scores)
- ✅ **Extensibility** (pluggable solvers, configurable strategies)
