//! Module for providing KyberSwap Aggregator liquidity to solvers.
//!
//! KyberSwap Aggregator finds optimal routes across multiple DEXes on-chain.
//! This module queries the KyberSwap API on-demand per auction to get executable
//! swap routes.

use {
    crate::{
        interactions::allowances::{AllowanceManager, AllowanceManaging, Allowances},
        liquidity::{
            Exchange, LimitOrder, LimitOrderExecution, LimitOrderId, LiquidityOrderId,
            SettlementHandling,
        },
        liquidity_collector::LiquidityCollecting,
        settlement::SettlementEncoder,
    },
    alloy::hex,
    anyhow::Result,
    ethcontract::Bytes,
    model::{TokenPair, order::OrderKind},
    primitive_types::{H160, U256},
    shared::{
        ethrpc::Web3,
        kyberswap_api::{BuildRouteRequest, KyberSwapApi, KyberSwapApiError, RouteRequest},
    },
    std::{
        collections::{HashMap, HashSet},
        str::FromStr,
        sync::Arc,
        time::{Duration, Instant},
    },
    tokio::sync::RwLock,
    tracing::instrument,
};

/// Cache entry for a KyberSwap route
#[derive(Clone, Debug)]
struct CachedRoute {
    route: KyberSwapRoute,
    timestamp: Instant,
}

/// Cache for KyberSwap routes to reduce API calls
struct RouteCache {
    entries: HashMap<(H160, H160, U256), CachedRoute>,
}

impl RouteCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn get(
        &self,
        token_in: H160,
        token_out: H160,
        amount_in: U256,
        ttl: Duration,
    ) -> Option<KyberSwapRoute> {
        let key = (token_in, token_out, amount_in);
        if let Some(cached) = self.entries.get(&key) {
            if cached.timestamp.elapsed() < ttl {
                tracing::debug!(?token_in, ?token_out, ?amount_in, "KyberSwap cache hit");
                return Some(cached.route.clone());
            }
        }
        None
    }

    fn insert(&mut self, token_in: H160, token_out: H160, amount_in: U256, route: KyberSwapRoute) {
        let key = (token_in, token_out, amount_in);
        self.entries.insert(
            key,
            CachedRoute {
                route,
                timestamp: Instant::now(),
            },
        );
    }

    /// Clean up expired entries to prevent unbounded growth
    fn cleanup(&mut self, ttl: Duration) {
        self.entries
            .retain(|_, cached| cached.timestamp.elapsed() < ttl);
    }
}

/// A KyberSwap route with encoded transaction data
#[derive(Clone, Debug)]
pub struct KyberSwapRoute {
    pub token_in: H160,
    pub token_out: H160,
    pub amount_in: U256,
    pub amount_out: U256,
    pub encoded_data: Bytes<Vec<u8>>,
    pub router_address: H160,
    pub gas_estimate: u64,
}

/// Liquidity provider for KyberSwap Aggregator
pub struct KyberSwapLiquidity {
    /// KyberSwap API client
    api: Arc<dyn KyberSwapApi>,
    /// MetaAggregation Router v2 contract address
    meta_aggregator_router: H160,
    /// Settlement contract address (for sender/recipient in swaps)
    settlement_contract: H160,
    /// Slippage tolerance in basis points (e.g., 50 = 0.5%)
    slippage_bps: u32,
    /// Route cache with TTL
    cache: Arc<RwLock<RouteCache>>,
    /// Cache time-to-live
    cache_ttl: Duration,
    /// Allowance manager for checking token approvals
    allowance_manager: Box<dyn AllowanceManaging>,
}

impl KyberSwapLiquidity {
    /// Creates a new KyberSwap liquidity provider
    pub async fn new(
        api: Arc<dyn KyberSwapApi>,
        meta_aggregator_router: H160,
        settlement_contract: H160,
        slippage_bps: u32,
        cache_ttl: Duration,
        web3: Web3,
    ) -> Self {
        let allowance_manager = AllowanceManager::new(web3, settlement_contract);

        Self {
            api,
            meta_aggregator_router,
            settlement_contract,
            slippage_bps,
            cache: Arc::new(RwLock::new(RouteCache::new())),
            cache_ttl,
            allowance_manager: Box::new(allowance_manager),
        }
    }

    /// Fetches a route for a specific token pair and amount
    #[instrument(skip(self), fields(token_in = ?token_in, token_out = ?token_out, amount_in = ?amount_in))]
    async fn get_route_for_pair(
        &self,
        token_in: H160,
        token_out: H160,
        amount_in: U256,
    ) -> Result<KyberSwapRoute, KyberSwapApiError> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(route) = cache.get(token_in, token_out, amount_in, self.cache_ttl) {
                return Ok(route);
            }
        }

        // Cache miss - fetch from API
        tracing::debug!(
            ?token_in,
            ?token_out,
            ?amount_in,
            "KyberSwap cache miss, fetching from API"
        );

        let route = self
            .fetch_and_build_route(token_in, token_out, amount_in)
            .await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(token_in, token_out, amount_in, route.clone());
            // Periodically clean up expired entries
            cache.cleanup(self.cache_ttl);
        }

        Ok(route)
    }

    /// Fetches route from API and builds executable transaction
    async fn fetch_and_build_route(
        &self,
        token_in: H160,
        token_out: H160,
        amount_in: U256,
    ) -> Result<KyberSwapRoute, KyberSwapApiError> {
        // Step 1: Get optimal route
        let route_summary = self
            .api
            .get_routes(&RouteRequest {
                token_in,
                token_out,
                amount_in,
                gas_include: Some(true),
                gas_price: None, // Let KyberSwap use current gas price
            })
            .await?;

        tracing::info!(
            token_in = ?token_in,
            token_out = ?token_out,
            amount_in = ?amount_in,
            amount_out = ?route_summary.amount_out,
            gas = route_summary.gas,
            "KyberSwap route found"
        );

        // Step 2: Build executable transaction data
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 1800; // 30 minutes from now

        let build_response = self
            .api
            .build_route(&BuildRouteRequest {
                route_summary: route_summary.clone(),
                slippage: self.slippage_bps,
                sender: self.settlement_contract,
                recipient: self.settlement_contract,
                deadline,
            })
            .await?;

        // Parse encoded data
        let encoded_data = if build_response.encoded_data.starts_with("0x") {
            hex::decode(&build_response.encoded_data[2..])
        } else {
            hex::decode(&build_response.encoded_data)
        }
        .map_err(|e| {
            KyberSwapApiError::InvalidResponse(format!("Invalid hex in encoded_data: {}", e))
        })?;

        tracing::info!(
            amount_out = ?route_summary.amount_out,
            "fetch_and_build_route::Original amount_out"
        );

        // Update amount_out with slippage
        let slippage = self.slippage_bps;
        let amount_out = U256::from_dec_str(&route_summary.amount_out)
            .unwrap_or_default()
            .saturating_mul(U256::from(10_000 - slippage))
            .checked_div(U256::from(10_000))
            .unwrap_or_default();

        tracing::info!(
            amount_out = ?amount_out,
            "fetch_and_build_route::Updated amount_out with slippage"
        );

        Ok(KyberSwapRoute {
            token_in,
            token_out,
            amount_in,
            amount_out,
            encoded_data: Bytes(encoded_data),
            router_address: build_response.router_address,
            gas_estimate: route_summary.gas.parse::<u64>().unwrap_or_default(),
        })
    }
}

#[async_trait::async_trait]
impl LiquidityCollecting for KyberSwapLiquidity {
    #[instrument(name = "kyberswap_liquidity", skip_all)]
    async fn get_liquidity(
        &self,
        pairs: HashSet<TokenPair>,
        _block: shared::recent_block_cache::Block,
    ) -> Result<Vec<crate::liquidity::Liquidity>> {
        tracing::info!(pair_count = pairs.len(), "Fetching KyberSwap liquidity");

        let mut liquidity = Vec::new();

        for pair in pairs {
            let (token_a, token_b) = pair.get();

            // Try to get route for token_a -> token_b
            // Use a reasonable default amount for quoting (e.g., 1 token with 18 decimals)
            let default_amount = U256::from(10).pow(U256::from(18));

            match self
                .get_route_for_pair(token_a, token_b, default_amount)
                .await
            {
                Ok(route) => {
                    // Get allowances for the input token
                    let tokens = [token_a].into_iter().collect();
                    let allowances = match self
                        .allowance_manager
                        .get_allowances(tokens, self.meta_aggregator_router)
                        .await
                    {
                        Ok(allowances) => Arc::new(allowances),
                        Err(err) => {
                            tracing::warn!(
                                ?err,
                                ?token_a,
                                ?token_b,
                                "Failed to fetch allowances for KyberSwap route, skipping"
                            );
                            continue;
                        }
                    };

                    let limit_order = LimitOrder {
                        id: LimitOrderId::Liquidity(LiquidityOrderId::Protocol(
                            model::order::OrderUid([0u8; 56]), // Placeholder UID
                        )),
                        sell_token: route.token_in,
                        buy_token: route.token_out,
                        sell_amount: route.amount_in,
                        buy_amount: route.amount_out,
                        kind: OrderKind::Sell,
                        partially_fillable: false, // KyberSwap routes are all-or-nothing
                        user_fee: U256::zero(),
                        settlement_handling: Arc::new(KyberSwapSettlementHandler {
                            route: route.clone(),
                            meta_aggregator_router: self.meta_aggregator_router,
                            allowances,
                        }),
                        exchange: Exchange::ZeroEx, // Reuse ZeroEx exchange type for now
                    };

                    liquidity.push(crate::liquidity::Liquidity::LimitOrder(limit_order));
                }
                Err(KyberSwapApiError::NoRouteFound) => {
                    tracing::debug!(?token_a, ?token_b, "No KyberSwap route found for pair");
                }
                Err(KyberSwapApiError::RateLimited) => {
                    tracing::warn!(
                        ?token_a,
                        ?token_b,
                        "Rate limited by KyberSwap API, skipping pair"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        ?token_a,
                        ?token_b,
                        "Failed to fetch KyberSwap route for pair"
                    );
                }
            }
        }

        tracing::info!(route_count = liquidity.len(), "Fetched KyberSwap liquidity");

        Ok(liquidity)
    }
}

/// Settlement handler for KyberSwap routes
#[derive(Clone)]
pub struct KyberSwapSettlementHandler {
    pub route: KyberSwapRoute,
    pub meta_aggregator_router: H160,
    pub allowances: Arc<Allowances>,
}

impl SettlementHandling<LimitOrder> for KyberSwapSettlementHandler {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn encode(
        &self,
        execution: LimitOrderExecution,
        encoder: &mut SettlementEncoder,
    ) -> Result<()> {
        // Validate execution amount matches route
        if execution.filled != self.route.amount_in {
            anyhow::bail!(
                "KyberSwap execution amount mismatch: expected {}, got {}",
                self.route.amount_in,
                execution.filled
            );
        }

        // Add approval if needed
        let approval =
            self.allowances
                .approve_token(shared::http_solver::model::TokenAmount::new(
                    self.route.token_in,
                    execution.filled,
                ))?;

        if let Some(approval) = approval {
            encoder.append_to_execution_plan(Arc::new(approval));
        }

        // Add KyberSwap swap interaction
        encoder.append_to_execution_plan(Arc::new(KyberSwapInteraction {
            encoded_data: self.route.encoded_data.clone(),
            router_address: self.route.router_address,
        }));

        Ok(())
    }
}

/// Helper to format H160 for logging
#[allow(dead_code)]
fn addr2str(addr: H160) -> String {
    format!("{addr:#x}")
}

/// Interaction for calling KyberSwap MetaAggregator
#[derive(Clone, Debug)]
pub struct KyberSwapInteraction {
    pub encoded_data: Bytes<Vec<u8>>,
    pub router_address: H160,
}

impl shared::interaction::Interaction for KyberSwapInteraction {
    fn encode(&self) -> shared::interaction::EncodedInteraction {
        (
            self.router_address,
            0.into(), // No ETH value
            self.encoded_data.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_cache_expiry() {
        let mut cache = RouteCache::new();
        let token_in = H160::from_low_u64_be(1);
        let token_out = H160::from_low_u64_be(2);
        let amount = U256::from(1000);

        let route = KyberSwapRoute {
            token_in,
            token_out,
            amount_in: amount,
            amount_out: U256::from(900),
            encoded_data: Bytes(vec![]),
            router_address: H160::zero(),
            gas_estimate: 100000,
        };

        // Insert route
        cache.insert(token_in, token_out, amount, route.clone());

        // Should hit cache immediately
        assert!(
            cache
                .get(token_in, token_out, amount, Duration::from_secs(60))
                .is_some()
        );

        // Should miss cache with 0 TTL
        assert!(
            cache
                .get(token_in, token_out, amount, Duration::from_secs(0))
                .is_none()
        );
    }
}
