use {
    super::{LimitOrderExecution, LimitOrderId, LiquidityOrderId, SettlementHandling},
    crate::{
        interactions::allowances::{AllowanceManager, AllowanceManaging},
        liquidity::{Exchange, LimitOrder, Liquidity},
        liquidity_collector::LiquidityCollecting,
        settlement::SettlementEncoder,
    },
    anyhow::{Context, Result},
    arc_swap::ArcSwap,
    contracts::GPv2Settlement,
    ethrpc::block_stream::CurrentBlockWatcher,
    model::{TokenPair, order::OrderKind},
    primitive_types::{H160, U256},
    shared::{
        ethrpc::Web3,
        kyber_api::{KyberApi, KyberRoutingApiQuery, MethodParameters},
        recent_block_cache::Block,
    },
    std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    },
    tracing::instrument,
};

type RouteCache = ArcSwap<HashMap<(H160, H160), Vec<MethodParameters>>>;

pub struct KyberLiquidity {
    pub api: Arc<dyn KyberApi>,
    pub settlement: GPv2Settlement,
    pub allowance_manager: Box<dyn AllowanceManaging>,
    pub route_cache: Arc<RouteCache>,
}

impl KyberLiquidity {
    pub async fn new(
        web3: Web3,
        api: Arc<dyn KyberApi>,
        settlement: GPv2Settlement,
        _blocks: CurrentBlockWatcher,
    ) -> Self {
        let settlement_address = settlement.address();
        let allowance_manager = AllowanceManager::new(web3, settlement_address);
        let route_cache: Arc<RouteCache> = Default::default();

        Self {
            api,
            settlement,
            allowance_manager: Box::new(allowance_manager),
            route_cache,
        }
    }

    /// Convert Kyber method parameters into liquidity that solvers can use
    fn method_into_liquidity(
        &self,
        method: MethodParameters,
        token_in: H160,
        token_out: H160,
        sell_amount: U256,
        buy_amount: U256,
    ) -> Option<Liquidity> {
        if sell_amount.is_zero() || buy_amount.is_zero() {
            return None;
        }

        // Create a unique ID for this route
        let route_id = format!(
            "kyber_{}_{}_{}",
            token_in, token_out, sell_amount
        );

        let limit_order = LimitOrder {
            id: LimitOrderId::Liquidity(LiquidityOrderId::Kyber(route_id)),
            sell_token: token_in,
            buy_token: token_out,
            sell_amount,
            buy_amount,
            kind: OrderKind::Sell,
            partially_fillable: false, // Kyber routes are typically exact amounts
            user_fee: U256::zero(),
            settlement_handling: Arc::new(KyberSettlementHandler {
                method,
            }),
            exchange: Exchange::Kyber,
        };

        Some(Liquidity::LimitOrder(limit_order))
    }
}

#[async_trait::async_trait]
impl LiquidityCollecting for KyberLiquidity {
    #[instrument(name = "kyber_liquidity", skip_all)]
    async fn get_liquidity(
        &self,
        pairs: HashSet<TokenPair>,
        _block: Block,
    ) -> Result<Vec<Liquidity>> {
        let mut all_routes = Vec::new();

        tracing::info!("Kyber: Fetching liquidity for {} token pairs", pairs.len());

        // For each token pair, query Kyber API
        for pair in pairs.iter() {
            let (token_a, token_b) = (pair.get().0, pair.get().1);

            // Query both directions
            for (token_in, token_out) in [(token_a, token_b), (token_b, token_a)] {
                // Use a reasonable default amount for querying (e.g., 1 ETH equivalent)
                let amount_in = "1000000000000000000".to_string(); // 1 ETH in wei

                let query = KyberRoutingApiQuery {
                    token_in: format!("{:#x}", token_in),
                    token_out: format!("{:#x}", token_out),
                    amount_in,
                };

                // Step 1: Get the best route
                match self.api.get_quote(&query).await {
                    Ok(response) => {
                        let sell_amount: U256 = match response.data.route_summary.amount_in.parse() {
                            Ok(amount) => amount,
                            Err(_) => continue,
                        };
                        let buy_amount: U256 = match response.data.route_summary.amount_out.parse() {
                            Ok(amount) => amount,
                            Err(_) => continue,
                        };

                        // Step 2: Build the swap transaction
                        let settlement_address = self.settlement.address();
                        let deadline = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs()
                            + 3600; // 1 hour from now

                        match self.api.build_swap(
                            response.data.route_summary,
                            format!("{:#x}", settlement_address),
                            format!("{:#x}", settlement_address),
                            deadline,
                            50, // 0.5% slippage tolerance
                        ).await {
                            Ok(method) => {
                                if let Some(liquidity) = self.method_into_liquidity(
                                    method,
                                    token_in,
                                    token_out,
                                    sell_amount,
                                    buy_amount,
                                ) {
                                    all_routes.push(liquidity);
                                    tracing::debug!(
                                        "Kyber: Got route for {:?} -> {:?} (sell: {}, buy: {})",
                                        token_in,
                                        token_out,
                                        sell_amount,
                                        buy_amount
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "Kyber: Failed to build swap for {:?} -> {:?}: {}",
                                    token_in,
                                    token_out,
                                    e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Kyber: Failed to get route for {:?} -> {:?}: {}",
                            token_in,
                            token_out,
                            e
                        );
                    }
                }
            }
        }

        tracing::info!("Kyber: Got {} routes total", all_routes.len());
        Ok(all_routes)
    }
}

/// Settlement handler for Kyber swaps
#[derive(Clone)]
pub struct KyberSettlementHandler {
    pub method: MethodParameters,
}

impl SettlementHandling<super::LimitOrder> for KyberSettlementHandler {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn encode(
        &self,
        execution: LimitOrderExecution,
        encoder: &mut SettlementEncoder,
    ) -> Result<()> {
        // Decode the calldata from build_swap response
        let call_data_hex = self.method.calldata.trim_start_matches("0x");
        let call_data = const_hex::decode(call_data_hex)
            .context("failed to decode Kyber calldata from build_swap")?;

        // Parse the target router address
        let target = self.method.to.trim_start_matches("0x");
        let target_bytes = const_hex::decode(target)
            .context("failed to decode Kyber router address")?;
        let target_address = H160::from_slice(&target_bytes);

        // Parse the value (usually 0 for token swaps)
        let value: U256 = self.method.value.parse()
            .unwrap_or(U256::zero());

        tracing::debug!(
            "Encoding Kyber swap: router={:?}, data_len={}, value={}, filled={}",
            target_address,
            call_data.len(),
            value,
            execution.filled
        );

        // Encode as a generic interaction using tuple format
        encoder.append_to_execution_plan_internalizable(
            Arc::new((
                target_address,
                value,
                ethcontract::Bytes(call_data),
            )),
            false, // Kyber swaps should not be internalized
        );

        Ok(())
    }
}
