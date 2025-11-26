//! KyberSwap-only solution generation.
//!
//! This module generates solutions directly from KyberSwap routes without
//! calling the solver engine. Routes are fetched on-demand for each order
//! with exact order amounts.

use {
    crate::{
        domain::{
            competition::{auction::Auction, order, solution},
            eth::{self, TokenAddress},
        },
        infra::{Solver, blockchain::Ethereum, liquidity::config::KyberSwap},
        util,
    },
    anyhow::{Context, Result},
    ethcontract::{H160, U256},
    num::{BigRational, ToPrimitive},
    shared::kyberswap_api::{
        DefaultKyberSwapApi, KyberSwapApi, KyberSwapApiError, KyberSwapConfig,
    },
    std::{collections::HashMap, sync::Arc},
    tracing::instrument,
};

/// Generates solutions directly from KyberSwap routes for orders in an auction.
pub struct KyberSwapSolutionGenerator {
    api: Arc<dyn KyberSwapApi>,
    settlement_contract: H160,
    weth: eth::WethAddress,
    solver: Solver,
}

impl KyberSwapSolutionGenerator {
    /// Convert slippage from BigRational to basis points (BPS).
    ///
    /// Basis points represent the slippage as parts per 10,000.
    /// For example: 0.5 (0.5%) = 50 BPS, 0.01 (1%) = 100 BPS
    fn slippage_to_bps(slippage: &BigRational) -> u32 {
        // Multiply by 10,000 to convert to basis points
        let bps = slippage * BigRational::from_integer(100.into());

        // Convert to u32, defaulting to 50 BPS (0.5%) if conversion fails
        bps.to_u32().unwrap_or(50)
    }

    pub fn new(eth: &Ethereum, config: &KyberSwap, solver: Solver) -> Result<Self> {
        // let http_client_factory = &HttpClientFactory::new(&shared::http_client::Arguments {
        //     http_timeout: config.http_timeout,
        // });

        let api_config = KyberSwapConfig {
            api_url: config.api_url.clone(),
            chain_name: config.chain_name.clone(),
            http_timeout: config.http_timeout,
            client_id: config.client_id.clone(),
        };

        let api = Arc::new(DefaultKyberSwapApi::new(
            reqwest::ClientBuilder::new(),
            api_config.clone(),
        )?);

        Ok(Self {
            api,
            settlement_contract: eth.contracts().settlement().address(),
            weth: eth.contracts().weth_address(),
            solver,
        })
    }

    /// Generate solutions for all orders in the auction using KyberSwap routes.
    #[instrument(name = "kyberswap_solve", skip_all)]
    pub async fn solve(&self, auction: &Auction) -> Result<Vec<solution::Solution>> {
        let mut solutions = Vec::new();
        let mut solution_id_counter = 0u64;

        tracing::info!("Solving auction orders length: {}", auction.orders().len());
        println!("kyberswap_solve auction: {:#?}", auction);

        for order in auction.orders() {
            // Determine token pair and amount based on order side
            let (token_in, token_out, amount_in) = match order.side {
                order::Side::Sell => (
                    order.sell.token.as_erc20(self.weth),
                    order.buy.token.as_erc20(self.weth),
                    order.sell.amount.0,
                ),
                order::Side::Buy => (
                    order.buy.token.as_erc20(self.weth),
                    order.sell.token.as_erc20(self.weth),
                    // For buy orders, we need to estimate the input amount
                    // For now, use a reasonable default - this could be improved
                    order.buy.amount.0,
                ),
            };

            // Fetch route from KyberSwap API with exact order amount
            let mut route = match self.fetch_route(token_in, token_out, amount_in).await {
                Ok(route) => route,
                Err(KyberSwapApiError::NoRouteFound) => {
                    tracing::debug!(
                        ?token_in,
                        ?token_out,
                        ?amount_in,
                        "No KyberSwap route found for order"
                    );
                    continue;
                }
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        ?token_in,
                        ?token_out,
                        ?amount_in,
                        "Failed to fetch KyberSwap route"
                    );
                    continue;
                }
            };

            // Validate route output matches order requirements
            // Update amount_out with slippage
            let slippage_bps = Self::slippage_to_bps(&self.solver.slippage().relative);
            tracing::info!(
                slippage_bps = ?slippage_bps,
                "solve::Slippage in basis points"
            );
            tracing::info!(
                route_amount_out = ?route.amount_out,
                "solve::Original route amount out"
            );
            let route_amount_out = eth::U256::from_dec_str(&route.amount_out)
                .unwrap_or_default()
                .saturating_mul(U256::from(10_000 - slippage_bps))
                .checked_div(U256::from(10_000))
                .unwrap_or_default();

            tracing::info!(
                route_amount_out = ?route_amount_out,
                "solve::Route amount out with slippage"
            );

            let order_amount_out = match order.side {
                order::Side::Sell => order.buy.amount.0,
                order::Side::Buy => order.sell.amount.0,
            };

            // Check if route satisfies order (with slippage tolerance)
            if !self.satisfies_order(route_amount_out, order_amount_out, order.side) {
                tracing::debug!(
                    ?route_amount_out,
                    ?order_amount_out,
                    "Route does not satisfy order requirements"
                );
                continue;
            }

            // Update amount_out with slippage
            route.amount_out = route_amount_out.to_string();

            // Create solution for this order
            match self
                .create_solution(auction, order, &route, solution_id_counter, slippage_bps)
                .await
            {
                Ok(solution) => {
                    solutions.push(solution);
                    solution_id_counter += 1;
                }
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        ?order.uid,
                        "Failed to create solution for order"
                    );
                }
            }
        }

        println!("solutions kyberswap: {:#?}", solutions);

        Ok(solutions)
    }

    /// Fetch a route from KyberSwap API for the given token pair and amount.
    async fn fetch_route(
        &self,
        token_in: TokenAddress,
        token_out: TokenAddress,
        amount_in: eth::U256,
    ) -> Result<shared::kyberswap_api::RouteSummary, KyberSwapApiError> {
        use shared::kyberswap_api::RouteRequest;

        let request = RouteRequest {
            token_in: token_in.0.0,
            token_out: token_out.0.0,
            amount_in,
            gas_include: Some(true),
            gas_price: None,
        };

        tracing::info!("Fetching route for token pair: {:?}", request);

        let route_summary = self.api.get_routes(&request).await?;

        Ok(route_summary)
    }

    /// Check if route output satisfies order requirements.
    fn satisfies_order(
        &self,
        route_amount_out: eth::U256,
        order_amount_out: eth::U256,
        side: order::Side,
    ) -> bool {
        match side {
            order::Side::Sell => {
                // For sell orders, route must provide at least the buy amount
                route_amount_out >= order_amount_out
            }
            order::Side::Buy => {
                // For buy orders, route must not exceed the sell amount
                route_amount_out <= order_amount_out
            }
        }
    }

    /// Create a solution from a KyberSwap route.
    async fn create_solution(
        &self,
        _auction: &Auction,
        order: &order::Order,
        route: &shared::kyberswap_api::RouteSummary,
        solution_id: u64,
        slippage_bps: u32,
    ) -> Result<solution::Solution> {
        use {
            crate::domain::competition::solution::{
                interaction::Interaction,
                trade::{Fee, Fulfillment},
            },
            std::collections::HashSet,
        };

        // Build the route to get encoded data
        let build_request = shared::kyberswap_api::BuildRouteRequest {
            route_summary: route.clone(),
            sender: self.settlement_contract,
            recipient: self.settlement_contract,
            slippage: slippage_bps,
            deadline: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 300, // 5 minutes from now
        };

        let build_response = self
            .api
            .build_route(&build_request)
            .await
            .context("Failed to build route")?;

        println!("build_response: {:#?}", build_response);
        println!("order: {:#?}", order);
        println!("route: {:#?}", route);

        // Determine executed amount based on order side
        let executed = match order.side {
            order::Side::Sell => order::TargetAmount(eth::U256::from_dec_str(&route.amount_in)?),
            order::Side::Buy => order::TargetAmount(eth::U256::from_dec_str(&route.amount_out)?),
        };

        // let executed = order::TargetAmount(eth::U256::from_dec_str(&route.amount_in)?);
        println!("executed: {:#?}", executed);
        println!("order target amount: {:#?}", order.target());

        let fee = if order.solver_determines_fee() {
            // Limit orders require Fee::Dynamic (even if fee is 0)
            Fee::Dynamic(order::SellAmount(eth::U256::zero()))
        } else {
            // Market orders use Fee::Static
            Fee::Static
        };

        // Create fulfillment trade
        let fulfillment = Fulfillment::new(order.clone(), executed, fee).map_err(|err| {
            tracing::error!(
                ?err,
                order_uid = ?order.uid,
                order_side = ?order.side,
                executed_amount = ?executed,
                sell_token = ?order.sell.token,
                buy_token = ?order.buy.token,
                sell_amount = ?order.sell.amount,
                buy_amount = ?order.buy.amount,
                route_amount_in = ?route.amount_in,
                route_amount_out = ?route.amount_out,
                "Failed to create fulfillment"
            );
            anyhow::anyhow!("Failed to create fulfillment: {}", err)
        })?;
        // .context("Failed to create fulfillment")?;

        println!("create_solution fulfillment: {:#?}", fulfillment);

        // Calculate clearing prices from route
        let prices = self.calculate_prices(route, order)?;

        println!("create_solution prices: {:#?}", prices);

        // Parse encoded data from hex string
        let encoded_data = alloy::hex::decode(
            build_response
                .encoded_data
                .strip_prefix("0x")
                .unwrap_or(&build_response.encoded_data),
        )
        .context("Failed to decode encoded data")?;

        // Convert router address from H160 to domain types
        let router_address: eth::Address = build_response.router_address.into();
        let router_contract: eth::ContractAddress = eth::ContractAddress(router_address.into());

        // Create KyberSwap interaction
        let interaction = Interaction::Custom(solution::interaction::Custom {
            target: router_contract,
            value: eth::Ether(build_response.value),
            call_data: util::Bytes(encoded_data),
            allowances: vec![eth::allowance::Required(eth::allowance::Allowance {
                token: order.sell.token,
                spender: router_address,
                amount: eth::U256::max_value(),
            })],
            inputs: vec![eth::Asset {
                token: order.sell.token,
                amount: eth::TokenAmount(eth::U256::from_dec_str(&route.amount_in)?),
            }],
            outputs: vec![eth::Asset {
                token: order.buy.token,
                amount: eth::TokenAmount(eth::U256::from_dec_str(&route.amount_out)?),
            }],
            internalize: false,
        });

        // Create solution
        let solution = solution::Solution::new(
            solution::Id::new(solution_id),
            vec![solution::Trade::Fulfillment(fulfillment)],
            prices,
            vec![], // pre_interactions
            vec![interaction],
            vec![], // post_interactions
            self.solver.clone(),
            self.weth,
            Some(eth::Gas(
                eth::U256::from_dec_str(&route.gas)
                    .unwrap_or_default(),
            )),
            crate::infra::config::file::FeeHandler::Driver,
            &HashSet::new(), // surplus_capturing_jit_order_owners
            HashMap::new(),  // flashloans
        )
        .context("Failed to create solution")?;

        println!("solution: {:#?}", solution);

        Ok(solution)
    }

    /// Calculate clearing prices from route.
    fn calculate_prices(
        &self,
        route: &shared::kyberswap_api::RouteSummary,
        order: &order::Order,
    ) -> Result<HashMap<TokenAddress, eth::U256>> {
        let amount_in = eth::U256::from_dec_str(&route.amount_in)?;
        let amount_out = eth::U256::from_dec_str(&route.amount_out)?;

        // Set a base price for the sell token (1e18)
        let sell_token_price = eth::U256::from(10_u64.pow(18));
        let buy_token_price = sell_token_price
            .checked_mul(amount_in)
            .and_then(|x| x.checked_div(amount_out))
            .context("Price calculation overflow or division by zero")?;

        let mut prices = HashMap::new();
        prices.insert(order.sell.token, sell_token_price);
        prices.insert(order.buy.token, buy_token_price);

        Ok(prices)
    }
}
