# Hyper EVM Testnet Integration

This document describes the integration of Hyper EVM Testnet (Chain ID: 998) into the CoW Protocol services codebase.

## Network Specifications

- **Chain ID**: 998
- **Network Name**: Hyper EVM Testnet
- **Native Token**: HYPE
- **Wrapped Native Token**: WHYPE
- **Block Time**: 1 second (1000ms)
- **Native Price Estimation Amount**: 0.1 HYPE (100000000000000000 wei)

## DEX Ecosystem

- **HyperSwap V2**: Uniswap V2 compatible DEX
- **HyperSwap V3**: Uniswap V3 compatible DEX
- **KyperSwap**: DEX aggregator (integration pending)

## Changes Made

### 1. Chain Definition (`crates/chain/src/lib.rs`)

Added `HyperEvmTestnet = 998` to the `Chain` enum with full implementation:
- Chain name: "Hyper EVM Testnet"
- Block time: 1000ms (1 second)
- Native price estimation amount: 0.1 HYPE (10^17 wei)
- Added to `TryFrom<u64>` implementation for chain ID conversion

### 2. Contract Build Configuration (`crates/contracts/build.rs`)

Added `HYPER_EVM_TESTNET` constant and placeholder addresses for:
- `WETH9` (WHYPE - Wrapped HYPE token)
- `GPv2Settlement` (Core settlement contract)
- `GPv2AllowListAuthentication` (Solver authorization contract)
- `Balances` (Balance checking helper contract)

**Important**: All contract addresses are currently set to `0x0000000000000000000000000000000000000000` and marked with `TODO` comments. These MUST be updated after contracts are deployed.

### 3. Alloy Network Constants (`crates/contracts/src/alloy.rs`)

Added `HYPER_EVM_TESTNET: u64 = 998` to the `networks` module for Alloy-based contract bindings.

### 4. Default Liquidity Sources (`crates/shared/src/sources/mod.rs`)

Configured default liquidity sources for Hyper EVM Testnet:
- `BaselineSource::UniswapV2` (for HyperSwap V2)
- `BaselineSource::UniswapV3` (for HyperSwap V3)

### 5. Example Configuration (`configs/local/hyper-evm-testnet.toml`)

Created example configuration file with:
- Chain ID setting
- Placeholder base token addresses
- Routing parameters (max-hops, max-partial-attempts)
- Native token price estimation amount

## Required Contract Deployments

The following CoW Protocol contracts need to be deployed to Hyper EVM Testnet:

### Core CoW Protocol Contracts

1. **GPv2Settlement**
   - Main settlement contract for order execution
   - Location in code: `crates/contracts/build.rs` line ~318

2. **GPv2AllowListAuthentication**
   - Controls which solvers can submit settlements
   - Location in code: `crates/contracts/build.rs` line ~218

3. **Balances** (Helper Contract)
   - Used for balance checking and simulations
   - Location in code: `crates/contracts/build.rs` line ~461

### Token Contracts

4. **WHYPE (Wrapped HYPE)**
   - ERC20 wrapper for native HYPE token
   - Location in code: `crates/contracts/build.rs` line ~354

### DEX Integration Contracts (If Not Already Deployed)

5. **HyperSwap V2 Router**
   - Uniswap V2 compatible router
   - May need configuration in liquidity sources

6. **HyperSwap V3 Router**
   - Uniswap V3 compatible router
   - May need configuration in liquidity sources

7. **HyperSwap Factories**
   - V2 and V3 factory contracts for pool creation

## Post-Deployment Steps

### Step 1: Update Contract Addresses

After deploying the CoW Protocol contracts, update the following files:

#### `crates/contracts/build.rs`

Search for `TODO: Update with actual` comments and replace `0x0000000000000000000000000000000000000000` with deployed addresses:

```rust
// Line ~354: WHYPE address
.add_network_str(HYPER_EVM_TESTNET, "0xYOUR_WHYPE_ADDRESS_HERE")

// Line ~218: GPv2AllowListAuthentication
.add_network(
    HYPER_EVM_TESTNET,
    Network {
        address: addr("0xYOUR_AUTHENTICATION_ADDRESS_HERE"),
        deployment_information: Some(DeploymentInformation::BlockNumber(YOUR_BLOCK_NUMBER)),
    },
)

// Line ~318: GPv2Settlement
.add_network(
    HYPER_EVM_TESTNET,
    Network {
        address: addr("0xYOUR_SETTLEMENT_ADDRESS_HERE"),
        deployment_information: Some(DeploymentInformation::BlockNumber(YOUR_BLOCK_NUMBER)),
    },
)

// Line ~461: Balances
.add_network_str(HYPER_EVM_TESTNET, "0xYOUR_BALANCES_ADDRESS_HERE")
```

### Step 2: Update Base Token Addresses

Update `configs/local/hyper-evm-testnet.toml` with actual token addresses:

```toml
base-tokens = [
    "0xACTUAL_WHYPE_ADDRESS",  # WHYPE (Wrapped HYPE)
    "0xACTUAL_USDC_ADDRESS",   # USDC or equivalent
    "0xACTUAL_USDT_ADDRESS",   # USDT or equivalent
    "0xACTUAL_DAI_ADDRESS",    # DAI or equivalent
]
```

### Step 3: Configure DEX Router Addresses

If HyperSwap routers are deployed, add their addresses to the alloy contract deployments.

Create entries similar to existing DEX routers in `crates/contracts/src/alloy.rs`:

```rust
crate::bindings!(
    HyperSwapV2Router,
    crate::deployments! {
        HYPER_EVM_TESTNET => address!("0xYOUR_HYPERSWAP_V2_ROUTER"),
    }
);

crate::bindings!(
    HyperSwapV3Router,
    crate::deployments! {
        HYPER_EVM_TESTNET => address!("0xYOUR_HYPERSWAP_V3_ROUTER"),
    }
);
```

### Step 4: Rebuild the Project

After updating all addresses:

```bash
# Rebuild contracts crate to regenerate bindings
cd crates/contracts
cargo build

# Or rebuild the entire project
cd ../..
cargo build
```

### Step 5: Test the Integration

1. Configure your driver to connect to Hyper EVM Testnet RPC
2. Set the chain ID to 998 in your driver configuration
3. Deploy test orders and verify settlement functionality

## Configuration Example

Example `driver.toml` configuration for Hyper EVM Testnet:

```toml
# Hyper EVM Testnet Driver Configuration
chain-id = 998

tx-gas-limit = "45000000"

[[solver]]
name = "hyperswap-solver"
endpoint = "http://localhost:8080"
absolute-slippage = "40000000000000000"
relative-slippage = "0.1"
account = "0xYOUR_SOLVER_PRIVATE_KEY"

[submission]
gas-price-cap = "1000000000000"

[[submission.mempool]]
mempool = "public"

[contracts]
gp-v2-settlement = "0xYOUR_SETTLEMENT_ADDRESS"
weth = "0xYOUR_WHYPE_ADDRESS"
balances = "0xYOUR_BALANCES_ADDRESS"
signatures = "0xYOUR_SIGNATURES_ADDRESS"

[liquidity]
base-tokens = [
    "0xYOUR_WHYPE_ADDRESS",
    "0xYOUR_USDC_ADDRESS",
    "0xYOUR_USDT_ADDRESS",
]

[[liquidity.uniswap-v2]]
preset = "uniswap-v2"  # Will use HyperSwap V2

[[liquidity.uniswap-v3]]
# Configure HyperSwap V3
```

## Additional Notes

### Liquidity Source Configuration

The system will automatically use HyperSwap V2 and V3 as liquidity sources based on the default configuration in `defaults_for_network()`. Manual configuration can override these defaults via TOML config files.

### Price Estimation

Native price estimation uses 0.1 HYPE (10^17 wei) as the default amount for querying prices. This can be adjusted in the chain configuration if needed.

### Block Time Considerations

With a 1-second block time, the Hyper EVM Testnet is significantly faster than Ethereum mainnet (12s) but slower than Arbitrum (250ms). This affects:
- Settlement finality timing
- Block number calculations for historical queries
- Reorg protection strategies

### Testing Checklist

- [ ] All contract addresses updated with actual deployments
- [ ] Base token addresses configured
- [ ] DEX router addresses added if applicable
- [ ] Driver successfully connects to Hyper EVM Testnet RPC
- [ ] Orders can be created and signed
- [ ] Settlements execute successfully
- [ ] Liquidity sources (HyperSwap) accessible
- [ ] Native price estimation working
- [ ] Gas estimation functioning correctly

## Support and Troubleshooting

### Common Issues

1. **"Chain ID not supported" error**
   - Ensure you've rebuilt the project after adding the chain
   - Verify chain ID is 998 in your configuration

2. **"No deployment info for chain" error**
   - Update placeholder contract addresses with actual deployments
   - Rebuild the contracts crate

3. **Liquidity source errors**
   - Verify DEX router addresses are correct
   - Check that pools exist for your token pairs
   - Ensure sufficient liquidity in pools

### Contract Deployment Resources

- CoW Protocol contracts repository: https://github.com/cowprotocol/contracts
- Deployment scripts are available in the contracts repo
- Recommended deployment order:
  1. WHYPE (if not already deployed)
  2. GPv2AllowListAuthentication
  3. GPv2Settlement
  4. Balances helper contract
  5. Additional support contracts as needed

## Version Information

- **Integration Date**: 2025-10-21
- **Target Chain**: Hyper EVM Testnet (Chain ID 998)
- **Status**: Placeholder addresses - requires contract deployment
- **Compatibility**: CoW Protocol services (current version)

---

For questions or issues, refer to the main CoW Protocol documentation or the services repository.

