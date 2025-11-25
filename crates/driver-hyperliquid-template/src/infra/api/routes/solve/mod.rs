pub mod dto;

pub use dto::AuctionError;
use {
    crate::infra::api::State,
    driver::{
        domain::{
            competition::{self, order, solution},
            eth,
        },
        infra::{api::error::Error, observe},
    },
    solvers_dto::auction::Auction,
    std::{
        collections::HashMap,
        sync::Arc,
    },
    tracing::Instrument,
    web3::ethabi,
};

pub(in crate::infra::api) fn solve(router: axum::Router<State>) -> axum::Router<State> {
    router.route("/solve", axum::routing::post(route))
}

async fn route(
    state: axum::extract::State<State>,
    req: String,
) -> Result<axum::Json<dto::SolveResponse>, (hyper::StatusCode, axum::Json<Error>)> {
    let handle_request = async {
        let auction: Auction = serde_json::from_str(&req).map_err(|e| {
            tracing::error!(?e, "Failed to parse auction");
            competition::Error::MalformedRequest
        })?;

        // Create solution with Vault interactions
        let (solved, solution_dto) = create_solution_with_vault(&auction)?;

        // Store settlement data (manually clone Solved since it doesn't implement Clone)
        let cloned_trades = solved.trades.iter().map(|(uid, amounts)| {
            (*uid, competition::Amounts {
                side: amounts.side,
                sell: amounts.sell,
                buy: amounts.buy,
                executed_sell: amounts.executed_sell,
                executed_buy: amounts.executed_buy,
            })
        }).collect();

        let settlement_data = crate::infra::api::SettlementData {
            solved: competition::Solved {
                id: solved.id.clone(),
                score: solved.score,
                trades: cloned_trades,
                prices: solved.prices.clone(),
                gas: solved.gas,
            },
            auction_json: req.clone(),
            solution_dto,
        };

        let mut settlements = state.settlements().lock().unwrap();
        settlements.insert(solved.id.get(), settlement_data);
        drop(settlements);

        Ok(axum::Json(dto::SolveResponse::new(
            Some(solved),
            state.solver(),
        )))
    };

    handle_request
        .instrument(tracing::info_span!("/solve", solver = %state.solver().name(), auction_id = tracing::field::Empty))
        .await
}

/// Create a solution with Vault interactions
fn create_solution_with_vault(
    auction: &Auction,
) -> Result<(competition::Solved, solvers_dto::solution::Solution), competition::Error> {
    use solvers_dto::solution::{
        Solution, Trade, Fulfillment, OrderUid, Interaction, CustomInteraction,
        Allowance, Asset,
    };
    use std::collections::HashMap;
    use web3::types::{H160, U256};

    let mut trades = Vec::new();
    let mut prices = HashMap::new();
    let mut interactions: Vec<Interaction> = Vec::new();
    let mut solved_trades = HashMap::new();

    // Set 1:1 prices for all tokens
    for (token, _) in &auction.tokens {
        prices.insert(*token, U256::from_dec_str("1000000000000000000").unwrap());
    }

    for order in &auction.orders {
        let executed_amount = match order.kind {
            solvers_dto::auction::Kind::Sell => order.sell_amount,
            solvers_dto::auction::Kind::Buy => order.buy_amount,
        };

        // Create fulfillment trade for DTO
        let fulfillment = Fulfillment {
            order: OrderUid(order.uid),
            executed_amount,
            fee: None,
        };
        trades.push(Trade::Fulfillment(fulfillment));

        // Create trade for Solved response
        let amounts = competition::Amounts {
            side: match order.kind {
                solvers_dto::auction::Kind::Sell => order::Side::Sell,
                solvers_dto::auction::Kind::Buy => order::Side::Buy,
            },
            sell: eth::Asset {
                token: eth::TokenAddress::from(order.sell_token),
                amount: order.sell_amount.into(),
            },
            buy: eth::Asset {
                token: eth::TokenAddress::from(order.buy_token),
                amount: order.buy_amount.into(),
            },
            executed_sell: eth::TokenAmount(order.sell_amount.into()),
            executed_buy: eth::TokenAmount(order.buy_amount.into()),
        };
        solved_trades.insert(order::Uid::from(order.uid), amounts);

        // Create Vault interaction for this order
        let vault_address = "0xdf2160bf40869b75fb9634ddb51779719937a450".parse::<H160>().unwrap();
        
        // Encode the exchange function call
        #[allow(deprecated)]
        let function = web3::ethabi::Function {
            name: "exchange".to_owned(),
            inputs: vec![
                web3::ethabi::Param { name: "orderUid".to_owned(), kind: web3::ethabi::ParamType::Bytes, internal_type: None },
                web3::ethabi::Param { name: "tokenIn".to_owned(), kind: web3::ethabi::ParamType::Address, internal_type: None },
                web3::ethabi::Param { name: "amountIn".to_owned(), kind: web3::ethabi::ParamType::Uint(256), internal_type: None },
                web3::ethabi::Param { name: "tokenOut".to_owned(), kind: web3::ethabi::ParamType::Address, internal_type: None },
                web3::ethabi::Param { name: "amountOut".to_owned(), kind: web3::ethabi::ParamType::Uint(256), internal_type: None },
                web3::ethabi::Param { name: "validTo".to_owned(), kind: web3::ethabi::ParamType::Uint(32), internal_type: None },
                web3::ethabi::Param { name: "nonce".to_owned(), kind: web3::ethabi::ParamType::Uint(256), internal_type: None },
                web3::ethabi::Param { name: "signatures".to_owned(), kind: web3::ethabi::ParamType::Array(Box::new(web3::ethabi::ParamType::Bytes)), internal_type: None },
            ],
            outputs: vec![],
            constant: None,
            state_mutability: web3::ethabi::StateMutability::Payable,
        };

        let tokens = vec![
            web3::ethabi::Token::Bytes(order.uid.to_vec()),
            web3::ethabi::Token::Address(order.sell_token),
            web3::ethabi::Token::Uint(order.sell_amount),
            web3::ethabi::Token::Address(order.buy_token),
            web3::ethabi::Token::Uint(order.buy_amount),
            web3::ethabi::Token::Uint(U256::from(order.valid_to)),
            web3::ethabi::Token::Uint(U256::zero()),
            web3::ethabi::Token::Array(vec![]),
        ];

        let calldata = function.encode_input(&tokens).map_err(|_| competition::Error::MalformedRequest)?;

        let interaction = Interaction::Custom(CustomInteraction {
            internalize: false,
            target: vault_address,
            value: U256::zero(),
            calldata,
            allowances: vec![Allowance {
                token: order.sell_token,
                spender: vault_address,
                amount: order.sell_amount,
            }],
            inputs: vec![Asset {
                token: order.sell_token,
                amount: order.sell_amount,
            }],
            outputs: vec![Asset {
                token: order.buy_token,
                amount: order.buy_amount,
            }],
        });

        interactions.push(interaction);
    }

    // Create the DTO solution
    let solution_dto = Solution {
        id: 0,
        prices: prices.clone(),
        trades,
        pre_interactions: vec![],
        interactions,
        post_interactions: vec![],
        gas: None,
        flashloans: None,
    };

    // Create Solved for response
    let solved = competition::Solved {
        id: solution::Id::new(0),
        score: eth::Ether(0.into()),
        trades: solved_trades,
        prices: prices.iter().map(|(k, v)| (eth::TokenAddress::from(*k), eth::TokenAmount(*v))).collect(),
        gas: None,
    };

    Ok((solved, solution_dto))
}
