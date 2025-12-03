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
    std::{collections::HashMap, sync::Arc},
    tracing::instrument,
    alloy::{
        self, sol, 
        primitives::{Address, U256 as alloyU256, keccak256, hex}, 
        sol_types::{SolValue, SolCall}, 
        signers::{local::PrivateKeySigner, Signer},
    },
    std::time::{SystemTime, UNIX_EPOCH},
};

sol! {
    function exchange(bytes32 orderUid, address tokenIn, uint256 amountIn, address tokenOut, uint256 amountOut, uint32 validTo, uint256 nonce, bytes[] calldata signatures) external payable;
}

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
            match self
                .create_solution(auction, order, solution_id_counter)
                .await
            {
                Ok(solution) => {
                    // print solution for debugging
                    tracing::debug!("Created solution for order UID: {:?}", order.uid);
                    println!("{:#?}", solution); 
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

        // TODO: Need to call API to get prices. Currently all prices are fake
        let prices: HashMap<TokenAddress, U256> = self.fake_prices(_auction).await?;

        let vault_address = self.vault_address;
        // Use the existing numeric amounts from the order and provide default

        let output_amount = match order.side {
            order::Side::Sell => order.buy.amount.0,
            order::Side::Buy => order.sell.amount.0,
        };

        let order_id = keccak256(order.uid.0.0);
        let token_in = order.sell.token;
        let amount_in = order.sell.amount.0;
        let token_out = order.buy.token;
        let amount_out = order.buy.amount.0;
        let valid_to = order.valid_to;
        let chain_id = self.eth.chain().id();
        let nonce: u64 = 1764727619; 
        // let nonce: u64 = SystemTime::now()
        // .duration_since(UNIX_EPOCH)
        // .unwrap()
        // .as_secs();
        
        let payload = (
            order_id,
            Address::from_slice(token_in.0.0.as_bytes()),
            alloyU256::from_limbs(amount_in.0),
            Address::from_slice(token_out.0.0.as_bytes()),
            alloyU256::from_limbs(amount_out.0),
            u32::from(valid_to),
            alloyU256::from(chain_id),
            alloyU256::from(nonce),
            Address::from(vault_address.0),
            Address::from(self.settlement_contract.0),
        );
        let payload_abi_encode = keccak256(payload.abi_encode());
        let solver_key = if let ethcontract::Account::Offline(key, _) = &self.solver.config().account {
            key
        } else {
            panic!("Solver account is not an offline account with a private key.");
        };
        let signer = PrivateKeySigner::from_bytes(&alloy::primitives::FixedBytes(solver_key.as_ref().clone())).unwrap();
        let signature = signer.sign_message(payload_abi_encode.as_slice()).await.unwrap();
        println!("Payload: {:?}", payload);
        println!("Payload ABI encoded(hex): {}\n", hex::encode_prefixed(&payload_abi_encode));
        println!("Signer: {:?}", signer);
        println!("Signature (hex): {}", hex::encode_prefixed(signature.as_bytes()));
        
        
        let exchange_calldata = exchangeCall{
            orderUid: order_id,
            tokenIn: Address::from_slice(token_in.0.0.as_bytes()),
            amountIn: alloyU256::from_limbs(amount_in.0),
            tokenOut: Address::from_slice(token_out.0.0.as_bytes()),
            amountOut: alloyU256::from_limbs(amount_out.0),
            validTo: u32::from(valid_to),
            nonce: alloyU256::from(nonce),
            signatures: vec![signature.as_bytes().to_vec().into()],
        }.abi_encode();

        println!("Exchange Calldata (hex): {}\n", hex::encode_prefixed(&exchange_calldata));
        

        let encoded: Vec<u8> = exchange_calldata;

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
                amount: eth::TokenAmount(amount_in),
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