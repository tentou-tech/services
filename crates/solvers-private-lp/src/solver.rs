use crate::api::HyperLiquidApi;
use crate::config::Config;
use anyhow::{Context, Result};
use alloy::{
    sol,
    primitives::{Address, U256 as AlloyU256, FixedBytes, hex, keccak256},
    sol_types::{SolValue, SolCall},
    signers::{local::PrivateKeySigner, Signer},
};
use std::sync::Arc;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use solvers_dto::{
    auction::{Auction, Order, Kind, Class},
    solution::{
        Solution, Trade, Fulfillment, Interaction, CustomInteraction, 
        Asset, Allowance, OrderUid,
    },
};
use web3::types::{H160, U256};
use std::str::FromStr;

sol! {
    function exchange(bytes32 orderUid, address tokenIn, uint256 amountIn, address tokenOut, uint256 amountOut, uint32 validTo, uint256 nonce, bytes[] calldata signatures) external payable;
}

pub struct SolverConfig {
    pub settlement_contract: H160,
    pub vault_address: H160,
    pub solver_private_key: String,
    pub chain_id: u64,
}

pub struct HyperLiquidSolver {
    api: Arc<dyn HyperLiquidApi>,
    config: SolverConfig,
    signer: PrivateKeySigner,
}

impl HyperLiquidSolver {
    pub fn new(api: Arc<dyn HyperLiquidApi>, config: SolverConfig) -> Result<Self> {
        let signer = PrivateKeySigner::from_str(&config.solver_private_key)?;
        Ok(Self {
            api,
            config,
            signer,
        })
    }

    pub async fn solve(&self, auction: Auction) -> Result<Vec<Solution>> {
        let mut solutions = Vec::new();
        let mut solution_id_counter = 0u64;

        tracing::info!("Solving auction orders length: {}", auction.orders.len());

        for order in &auction.orders {
            match self.create_solution(&auction, order, solution_id_counter).await {
                Ok(solution) => {
                     solutions.push(solution);
                     solution_id_counter += 1;
                }
                Err(err) => {
                    tracing::warn!(?err, "Failed to create solution for order");
                }
            }
        }
        Ok(solutions)
    }

    async fn create_solution(&self, auction: &Auction, order: &Order, solution_id: u64) -> Result<Solution> {
        tracing::info!(?order, "Processing order");

        // Mock prices for now
        let prices = self.fake_prices(auction).await?;
        
        let (token_in, amount_in, token_out, amount_out) = match order.kind {
            Kind::Sell => {
                let token_in = order.sell_token;
                let amount_in = order.sell_amount;
                let token_out = order.buy_token;
                
                let sell_price = prices.get(&token_in).context("Missing sell price")?;
                let buy_price = prices.get(&token_out).context("Missing buy price")?;
                
                if buy_price.is_zero() { return Err(anyhow::anyhow!("Buy price is zero")); }
                
                let amount_out = (amount_in * *sell_price) / *buy_price;
                (token_in, amount_in, token_out, amount_out)
            },
            Kind::Buy => {
                 let token_in = order.buy_token;
                let amount_in = order.buy_amount;
                let token_out = order.sell_token;
                
                let buy_price = prices.get(&token_in).context("Missing buy price")?;
                let sell_price = prices.get(&token_out).context("Missing sell price")?;
                 if sell_price.is_zero() { return Err(anyhow::anyhow!("Sell price is zero")); }
                 
                 let amount_out = (amount_in * *buy_price) / *sell_price;
                 (token_in, amount_in, token_out, amount_out)
            }
        };

        let valid_to = order.valid_to;
        let chain_id = self.config.chain_id;
        let nonce: u64 = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        
        // Hash the order UID to get 32 bytes ID
        let order_id_hash = keccak256(&order.uid);

        let payload = (
            order_id_hash,
            h160_to_address(token_in),
            u256_to_alloy(amount_in),
            h160_to_address(token_out),
            u256_to_alloy(amount_out),
            u32::from(valid_to),
            AlloyU256::from(chain_id),
            AlloyU256::from(nonce),
            h160_to_address(self.config.vault_address),
            h160_to_address(self.config.settlement_contract),
        );
        
        let payload_encoded = payload.abi_encode();
        let payload_hash = keccak256(&payload_encoded);
        
        let signature = self.signer.sign_message(payload_hash.as_slice()).await?;
        
        let exchange_calldata = exchangeCall {
            orderUid: order_id_hash,
            tokenIn: h160_to_address(token_in),
            amountIn: u256_to_alloy(amount_in),
            tokenOut: h160_to_address(token_out),
            amountOut: u256_to_alloy(amount_out),
            validTo: u32::from(valid_to),
            nonce: AlloyU256::from(nonce),
            signatures: vec![signature.as_bytes().to_vec().into()],
        }.abi_encode();

        let interaction = Interaction::Custom(CustomInteraction {
            internalize: false,
            target: self.config.vault_address,
            value: U256::zero(),
            calldata: exchange_calldata,
            allowances: vec![Allowance {
                token: order.sell_token,
                spender: self.config.vault_address,
                amount: U256::max_value(),
            }],
            inputs: vec![Asset {
                token: token_in,
                amount: amount_in,
            }],
            outputs: vec![Asset {
                token: token_out,
                amount: amount_out,
            }],
        });
        
        // If auction ID is None (e.g. quote/estimate), use None fee (Static).
        // Otherwise, use logic based on order class.
        let fee = if auction.id.is_none() {
            None
        } else {
            match order.class {
                Class::Market => Some(U256::zero()),
                Class::Limit => Some(U256::zero()), // <--- Changed this line
            }
        };

        let fulfillment = Fulfillment {
            order: OrderUid(order.uid),
            executed_amount: amount_in,
            fee,
        };
        tracing::info!(?fulfillment, "Created fulfillment with amount_in: {}", amount_in);

        let trades = vec![Trade::Fulfillment(fulfillment)];

        Ok(Solution {
            id: solution_id,
            prices,
            trades,
            pre_interactions: vec![],
            interactions: vec![interaction],
            post_interactions: vec![],
            gas: Some(250_000), // Default from original
            flashloans: None,
        })
    }

    async fn fake_prices(&self, auction: &Auction) -> Result<HashMap<H160, U256>> {
        let mut prices = HashMap::new();
        for order in &auction.orders {
            prices.insert(order.sell_token, U256::exp10(20));
            prices.insert(order.buy_token, U256::exp10(20));
        }
        Ok(prices)
    }
}

fn h160_to_address(h: H160) -> Address {
    Address::from_slice(h.as_bytes())
}

fn u256_to_alloy(u: U256) -> AlloyU256 {
    let mut bytes = [0u8; 32];
    u.to_big_endian(&mut bytes);
    AlloyU256::from_be_bytes(bytes)
}