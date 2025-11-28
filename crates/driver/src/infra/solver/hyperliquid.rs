//! HyperLiquid-only solution generation.
//!
//! This module generates solutions directly from HyperLiquid routes without
//! calling the solver engine.

use {
    crate::{
        domain::{
            competition::{auction::Auction, order, solution},
            eth::{self, TokenAddress},
        },
        infra::{Solver, blockchain::Ethereum, liquidity::config::HyperLiquid},
        util,
    },
    anyhow::{Context, Result},
    ethcontract::{H160, U256},
    num::{BigRational, ToPrimitive},
    std::{collections::HashMap, sync::Arc},
    tracing::instrument,
    
    alloy::{self, sol, hex::FromHex, primitives::{Address, B256}, signers::local::PrivateKeySigner, sol_types::SolValue, },
};

/// Generates solutions directly from HyperLiquid routes for orders in an auction.
pub struct HyperLiquidSolutionGenerator {
    api: Arc<dyn HyperLiquidApi>,
    settlement_contract: H160,
    vault_address: H160,
    eth: Ethereum,
    weth: eth::WethAddress,
    solver: Solver,
}

impl HyperLiquidSolutionGenerator {

    pub fn new(eth: &Ethereum, config: &HyperLiquid, solver: Solver) -> Result<Self> {
        let api_config = HyperLiquidConfig {
            vault_address: config.vault_address.clone(),
        };

        let api = Arc::new(DefaultHyperLiquidApi::new(
            reqwest::ClientBuilder::new(),
            api_config,
        )?);

        Ok(Self {
            api,
            eth: eth.clone(),
            settlement_contract: eth.contracts().settlement().address(),
            vault_address: config.vault_address.into(),
            weth: eth.contracts().weth_address(),
            solver,
        })
    }

    /// Generate solutions for all orders in the auction using HyperLiquid routes.
    #[instrument(name = "hyperliquid_solve", skip_all)]
    pub async fn solve(&self, auction: &Auction) -> Result<Vec<solution::Solution>> {
        let mut solutions = Vec::new();
        let mut solution_id_counter = 0u64;

        tracing::info!("Solving auction orders length: {}", auction.orders().len());

        for order in auction.orders() {
            let (token_in, token_out, amount_in) = match order.side {
                order::Side::Sell => (
                    order.sell.token.as_erc20(self.weth),
                    order.buy.token.as_erc20(self.weth),
                    order.sell.amount.0,
                ),
                order::Side::Buy => (
                    order.buy.token.as_erc20(self.weth),
                    order.sell.token.as_erc20(self.weth),
                    order.buy.amount.0,
                ),
            };

            // let mut route = match self.fetch_route(token_in, token_out, amount_in).await {
            //     Ok(route) => route,
            //     Err(err) => {
            //         tracing::warn!(?err, ?token_in, ?token_out, "Failed to fetch HyperLiquid route");
            //         continue;
            //     }
            // };

            // let slippage_bps = Self::slippage_to_bps(&self.solver.slippage().relative);
            // let route_amount_out = eth::U256::from_dec_str(&route.amount_out)
            //     .unwrap_or_default()
            //     .saturating_mul(U256::from(10_000 - slippage_bps))
            //     .checked_div(U256::from(10_000))
            //     .unwrap_or_default();

            // let order_amount_out = match order.side {
            //     order::Side::Sell => order.buy.amount.0,
            //     order::Side::Buy => order.sell.amount.0,
            // };

            // if !self.satisfies_order(route_amount_out, order_amount_out, order.side) {
            //     continue;
            // }

            // route.amount_out = route_amount_out.to_string();

            match self
                .create_solution(auction, order, solution_id_counter)
                .await
            {
                Ok(solution) => {
                    solutions.push(solution);
                    solution_id_counter += 1;
                }
                Err(err) => {
                    tracing::warn!(?err, ?order.uid, "Failed to create solution for order");
                }
            }
        }

        Ok(solutions)
    }

    async fn fetch_route(
        &self,
        token_in: TokenAddress,
        token_out: TokenAddress,
        amount_in: eth::U256,
    ) -> Result<RouteSummary> {
        let request = RouteRequest {
            token_in: token_in.0.0,
            token_out: token_out.0.0,
            amount_in,
        };

        self.api.get_routes(&request).await
    }

    fn satisfies_order(
        &self,
        route_amount_out: eth::U256,
        order_amount_out: eth::U256,
        side: order::Side,
    ) -> bool {
        match side {
            order::Side::Sell => route_amount_out >= order_amount_out,
            order::Side::Buy => route_amount_out <= order_amount_out,
        }
    }

    async fn fake_prices(&self, auction: &Auction) -> Result<HashMap<TokenAddress, U256>> {
        let mut prices = HashMap::new();
        for order in auction.orders() {
            prices.insert(order.sell.token, eth::U256::exp10(18));
            prices.insert(order.buy.token, eth::U256::exp10(18));
        }
        Ok(prices)
    }

    async fn create_solution(
        &self,
        _auction: &Auction,
        order: &order::Order,
        solution_id: u64,
    ) -> Result<solution::Solution> {
        use {
            crate::domain::competition::solution::{
                interaction::Interaction,
                trade::{Fee, Fulfillment},
            },
            std::collections::HashSet,
        };

        // let build_request = BuildRouteRequest {
        //     route_summary: route.clone(),
        //     slippage: slippage_bps,
        //     sender: self.settlement_contract,
        //     recipient: self.settlement_contract,
        //     deadline: std::time::SystemTime::now()
        //         .duration_since(std::time::UNIX_EPOCH)
        //         .unwrap()
        //         .as_secs()
        //         + 300,
        // };

        // let build_response = self
        //     .api
        //     .build_route(&build_request)
        //     .await
        //     .context("Failed to build route")?;

        let executed = match order.side {
            order::Side::Sell => order::TargetAmount(order.sell.amount.0),
            order::Side::Buy => order::TargetAmount(order.buy.amount.0),
        };

        let fee = if order.solver_determines_fee() {
            Fee::Dynamic(order::SellAmount(eth::U256::zero()))
        } else {
            Fee::Static
        };

        let fulfillment = Fulfillment::new(order.clone(), executed, fee).map_err(|err| {
            anyhow::anyhow!("Failed to create fulfillment: {}", err)
        })?;

        let prices: HashMap<TokenAddress, U256> = self.fake_prices(_auction).await?;

        let vault_address = self.vault_address;
        // Use the existing numeric amounts from the order and provide default
        // values for call_data and gas since route/build_response is not used here.
        let input_amount = order.sell.amount.0;
        let output_amount = match order.side {
            order::Side::Sell => order.buy.amount.0,
            order::Side::Buy => order.sell.amount.0,
        };

        let order_id = order.uid;
        let token_in = order.sell.token;
        let amount_in = order.sell.amount.0;
        let token_out = order.buy.token;
        let amount_out = order.buy.amount.0;
        let valid_to = order.valid_to;
        let chain_id = self.eth.chain().id();
        let nonce: u64 = 0; 
        sol! {
            function swap(uint amount0Out, uint amount1Out, address to) external;
        }
        let swap_calldata = swapCall {
            amount0Out: alloy::primitives::U256::from(1000000000000000000_u128),
            amount1Out: alloy::primitives::U256::from(1000000000000000000_u128),
            to: Address::from_hex("0x0000000000000000000000000000000000000000")?,
        }.abi_encode();

        let func = Function::parse(
            "function exchange(bytes32 orderUid, address tokenIn, uint256 amountIn, address tokenOut, uint256 amountOut, uint32 validTo, uint256 nonce, bytes[] calldata signatures) external payable;")?;
        let input = vec![
            DynSolValue::Uint(uint!(100000000000000000_U256), 256),
            DynSolValue::Uint(U256::ZERO, 256),
            DynSolValue::Address(Address::from([0x42; 20])),
            DynSolValue::Bytes(Bytes::new().into()),
        ];

        let encoded = (
            order_id,
            token_in,
            amount_in,
            token_out,
            amount_out,
            valid_to,
            chain_id,
            nonce,
            vault_address,
            self.settlement_contract,
        ).abi_encode();

        let encoded: Vec<u8> = Vec::new();

        let interaction = Interaction::Custom(solution::interaction::Custom {
            target: eth::ContractAddress(vault_address),
            value: eth::Ether(eth::U256::zero()),
            call_data: util::Bytes(encoded),
            allowances: vec![eth::allowance::Required(eth::allowance::Allowance {
                token: order.sell.token,
                spender: eth::Address(vault_address),
                amount: order.sell.amount.0,
            })],
            inputs: vec![eth::Asset {
                token: order.sell.token,
                amount: eth::TokenAmount(input_amount),
            }],
            outputs: vec![eth::Asset {
                token: order.buy.token,
                amount: eth::TokenAmount(output_amount),
            }],
            internalize: false,
        });

        let solution = solution::Solution::new(
            solution::Id::new(solution_id),
            vec![solution::Trade::Fulfillment(fulfillment)],
            prices,
            vec![],
            vec![interaction],
            vec![],
            self.solver.clone(),
            self.weth,
            Some(eth::Gas(eth::U256::zero())),
            crate::infra::config::file::FeeHandler::Driver,
            &HashSet::new(),
            HashMap::new(),
        )
        .context("Failed to create solution")?;

        Ok(solution)
    }

    
}

// --- API Client Implementation ---

use alloy::{dyn_abi::DynSolValue, json_abi::Function, primitives::Bytes};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct HyperLiquidConfig {
    pub vault_address: eth::ContractAddress,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRequest {
    pub token_in: H160,
    pub token_out: H160,
    pub amount_in: U256,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RouteSummary {
    pub amount_in: String,
    pub amount_out: String,
    pub gas: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRouteRequest {
    pub route_summary: RouteSummary,
    pub slippage: u32,
    pub sender: H160,
    pub recipient: H160,
    pub deadline: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRouteResponse {
    pub encoded_data: String,
    pub router_address: H160,
    pub value: U256,
}

#[async_trait::async_trait]
pub trait HyperLiquidApi: Send + Sync {
    async fn get_routes(&self, request: &RouteRequest) -> Result<RouteSummary>;
    async fn build_route(&self, request: &BuildRouteRequest) -> Result<BuildRouteResponse>;
}

struct DefaultHyperLiquidApi {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

impl DefaultHyperLiquidApi {
    fn new(client_builder: reqwest::ClientBuilder, config: HyperLiquidConfig) -> Result<Self> {
        Ok(Self {
            client: client_builder
                .timeout(std::time::Duration::from_millis(1000))
                .build()?,
            base_url: reqwest::Url::parse("https://api.hyperliquid.xyz/")?,
        })
    }
}

#[async_trait::async_trait]
impl HyperLiquidApi for DefaultHyperLiquidApi {
    async fn get_routes(&self, request: &RouteRequest) -> Result<RouteSummary> {
        // Implement actual API call here. For now returning error or mock.
        // Assuming endpoints are similar to KyberSwap for now or simple REST.
        let url = self.base_url.join("routes")?;
        let res = self.client.post(url).json(request).send().await?;
        res.json().await.map_err(Into::into)
    }

    async fn build_route(&self, request: &BuildRouteRequest) -> Result<BuildRouteResponse> {
        let url = self.base_url.join("route/build")?;
        let res = self.client.post(url).json(request).send().await?;
        res.json().await.map_err(Into::into)
    }
}