pub mod dto;

pub use dto::AuctionError;
use {
    crate::infra::api::State,
    driver::{
        domain::{
            competition::{self, order, solution},
            eth,
        },
        infra::{api::error::Error, config::file::FeeHandler},
        util::Bytes,
    },
    solvers_dto::auction::Auction,
    std::sync::Arc,
    driver::infra::observe::solved,
    std::collections::{HashMap, HashSet},
    tracing::Instrument,
    alloy::{
        primitives::{Address as AlloyAddress, U256 as AlloyU256, Bytes as AlloyBytes, keccak256},
        sol_types::SolValue,
    },
    rand::{Rng, rngs::OsRng},
    ethcontract,
    alloy::signers::{local::PrivateKeySigner, Signer},
};

pub(in crate::infra::api) fn solve(router: axum::Router<State>) -> axum::Router<State> {
    router.route("/solve", axum::routing::post(route))
}

async fn route(
    state: axum::extract::State<State>,
    req: String,
) -> Result<axum::Json<dto::SolveResponse>, (hyper::StatusCode, axum::Json<Error>)> {
    let handle_request = async {
        let competition = state.competition();
        let result = competition.solve(Arc::new(req)).await;
        // Solving takes some time, so there is a chance for the settlement queue to
        // have capacity again.
        competition.ensure_settle_queue_capacity()?;
        solved(state.solver().name(), &result);
        Ok(axum::Json(dto::SolveResponse::new(
            result?,
            &competition.solver,
        )))
    };

    handle_request
        .instrument(tracing::info_span!("/solve", solver = %state.solver().name(), auction_id = tracing::field::Empty))
        .await
    // let handle_request = async {
    //     let auction_dto: Auction = serde_json::from_str(&req).map_err(|e| {
    //         tracing::error!(?e, "Failed to parse auction");
    //         competition::Error::MalformedRequest
    //     })?;

    //     // 1. Convert DTO Auction to Domain Auction (Simplified for this use case)
    //     let domain_orders = map_orders(&auction_dto.orders);
    //     let domain_tokens = map_tokens(&auction_dto.tokens);
    //     let deadline = chrono::Utc::now() + chrono::Duration::seconds(60); // Mock deadline

    //     let auction_id = auction_dto.id
    //         .and_then(|id| competition::auction::Id::try_from(id).ok())
    //         .unwrap_or(competition::auction::Id(0));

    //     let domain_auction = competition::Auction::new(
    //          Some(auction_id),
    //          domain_orders,
    //          domain_tokens.into_iter(),
    //          deadline,
    //          state.eth(),
    //          HashSet::new(),
    //     ).await.map_err(|e| {
    //         tracing::error!(?e, "Failed to create domain auction");
    //         competition::Error::MalformedRequest
    //     })?;

    //     // 2. Create Domain Solution with Vault interactions
    //     let (solved, domain_solution) = create_domain_solution(&domain_auction, state.solver(), state.eth())?;

    //     // 3. Encode into Settlement
    //     let settlement = domain_solution
    //         .encode(
    //             &domain_auction,
    //             state.eth(),
    //             state.simulator(),
    //             state.solver().solver_native_token(),
    //         )
    //         .await
    //         .map_err(|e| {
    //             tracing::error!(?e, "Failed to encode settlement");
    //             competition::Error::MalformedRequest
    //         })?;

    //     // 4. Store in Competition
    //     {
    //         let mut settlements = state.competition().settlements.lock().unwrap();
    //         settlements.push_back(settlement);
    //     }

    //     Ok(axum::Json(dto::SolveResponse::new(
    //         Some(solved),
    //         state.solver(),
    //     )))
    // };

    // handle_request
    //     .instrument(tracing::info_span!("/solve", solver = %state.solver().name(), auction_id = tracing::field::Empty))
    //     .await
}

fn map_orders(orders: &[solvers_dto::auction::Order]) -> Vec<competition::Order> {
    orders.iter().map(|o| {
        competition::Order {
            uid: order::Uid::from(o.uid),
            kind: match o.class {
                solvers_dto::auction::Class::Market => order::Kind::Market,
                solvers_dto::auction::Class::Limit => order::Kind::Limit,
            },
            side: match o.kind {
                solvers_dto::auction::Kind::Sell => order::Side::Sell,
                solvers_dto::auction::Kind::Buy => order::Side::Buy,
            },
            sell: eth::Asset {
                token: eth::TokenAddress::from(o.sell_token),
                amount: o.sell_amount.into(),
            },
            buy: eth::Asset {
                token: eth::TokenAddress::from(o.buy_token),
                amount: o.buy_amount.into(),
            },
            signature: order::Signature {
                scheme: match o.signing_scheme {
                    solvers_dto::auction::SigningScheme::Eip712 => order::signature::Scheme::Eip712,
                    solvers_dto::auction::SigningScheme::EthSign => order::signature::Scheme::EthSign,
                    solvers_dto::auction::SigningScheme::Eip1271 => order::signature::Scheme::Eip1271,
                    solvers_dto::auction::SigningScheme::PreSign => order::signature::Scheme::PreSign,
                },
                data: Bytes(o.signature.clone()),
                signer: eth::Address(o.owner),
            },
            receiver: None,
            created: 0.into(),
            valid_to: driver::util::Timestamp(o.valid_to),
            app_data: order::app_data::AppData::default(), // Mock
            partial: order::Partial::No,
            pre_interactions: vec![],
            post_interactions: vec![],
            sell_token_balance: order::SellTokenBalance::Erc20,
            buy_token_balance: order::BuyTokenBalance::Erc20,
            protocol_fees: vec![],
            quote: None,
        }
    }).collect()
}

fn map_tokens(tokens: &HashMap<web3::types::H160, solvers_dto::auction::Token>) -> Vec<competition::auction::Token> {
    tokens.iter().map(|(addr, t)| {
        competition::auction::Token {
            decimals: t.decimals,
            symbol: t.symbol.clone(),
            address: eth::TokenAddress::from(*addr),
            price: t.reference_price.map(|p| competition::auction::Price(eth::Ether(p.into()))),
            available_balance: t.available_balance.into(),
            trusted: t.trusted,
        }
    }).collect()
}

fn create_domain_solution(
    auction: &competition::Auction,
    solver: &driver::infra::Solver,
    eth: &driver::infra::Ethereum,
) -> Result<(competition::Solved, competition::Solution), competition::Error> {
    let mut trades = Vec::new();
    let mut interactions = Vec::new();
    let mut solved_trades = HashMap::new();
    let mut prices = HashMap::new();

    let weth_address = eth.contracts().weth_address();
    let settlement_contract_address = eth.contracts().settlement().address();
    let chain_id = eth.chain().id();

    // Set prices (mock 1:1)
    for token in auction.tokens().iter() {
         prices.insert(token.address, eth::U256::exp10(18));
    }
    // Also add ETH and WETH price to ensure clearing prices are found
    prices.insert(eth::ETH_TOKEN, eth::U256::exp10(18));
    prices.insert(weth_address.0, eth::U256::exp10(18));


    for order in auction.orders() {
        // Correctly determine executed amount based on order side
        let executed_amount = match order.side {
            order::Side::Sell => order.sell.amount,
            order::Side::Buy => order.buy.amount,
        };

        // Create Fulfillment
        let fulfillment = solution::trade::Fulfillment::new(
            order.clone(),
            executed_amount.into(),
            solution::trade::Fee::Dynamic(order::SellAmount::default()), // Dynamic fee (0)
        ).map_err(|e| {
            tracing::error!(?e, "Failed to create fulfillment");
            competition::Error::SolutionNotAvailable
        })?;
        
        trades.push(solution::Trade::Fulfillment(fulfillment));

        // Create Solved Trade
         let amounts = competition::Amounts {
            side: order.side,
            sell: order.sell,
            buy: order.buy,
            executed_sell: order.sell.amount,
            executed_buy: order.buy.amount,
        };
        solved_trades.insert(order.uid, amounts);

        // Vault Interaction
        let vault_address = "0xdf2160bf40869b75fb9634ddb51779719937a450".parse::<eth::H160>().unwrap();

        let mut rng = OsRng;
        let nonce: u64 = rng.gen();
        let random_nonce = eth::U256::from(nonce);

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
                web3::ethabi::Param { name: "validTo".to_owned(), kind: web3::ethabi::ParamType::Uint(256), internal_type: None },
                web3::ethabi::Param { name: "chainId".to_owned(), kind: web3::ethabi::ParamType::Uint(256), internal_type: None },
                web3::ethabi::Param { name: "nonce".to_owned(), kind: web3::ethabi::ParamType::Uint(256), internal_type: None },
                web3::ethabi::Param { name: "contractAddress".to_owned(), kind: web3::ethabi::ParamType::Address, internal_type: None },
                web3::ethabi::Param { name: "sender".to_owned(), kind: web3::ethabi::ParamType::Address, internal_type: None },
                web3::ethabi::Param { name: "signatures".to_owned(), kind: web3::ethabi::ParamType::Array(Box::new(web3::ethabi::ParamType::Bytes)), internal_type: None },
            ],
            outputs: vec![],
            constant: None,
            state_mutability: web3::ethabi::StateMutability::Payable,
        };

        // Use Alloy for ABI encoding the payload to sign
        let sell_h160: web3::types::H160 = order.sell.token.0.into();
        let buy_h160: web3::types::H160 = order.buy.token.0.into();
        let payload = (
            AlloyBytes::from(order.uid.0.0.to_vec()),
            AlloyAddress::from_slice(&sell_h160.0),
            AlloyU256::from_limbs(order.sell.amount.0.0),
            AlloyAddress::from_slice(&buy_h160.0),
            AlloyU256::from_limbs(order.buy.amount.0.0),
            AlloyU256::from(u32::from(order.valid_to)),
            AlloyU256::from(chain_id),
            AlloyU256::from(nonce),
            AlloyAddress::from_slice(&vault_address.0),
            AlloyAddress::from_slice(settlement_contract_address.as_ref()),
        );
        
        let encoded_payload = payload.abi_encode();
        let message_hash = keccak256(&encoded_payload);
        
        // Sign using solver account (alloy)
        let solver_key = if let ethcontract::Account::Offline(key, _) = &solver.config().account {
            key
        } else {
            panic!("Solver account is not an offline account with a private key.");
        };
        let signer = PrivateKeySigner::from_bytes(&alloy::primitives::FixedBytes(solver_key.as_ref().clone())).unwrap();
        let signature = futures::executor::block_on(signer.sign_hash(&message_hash)).unwrap();
        let signature = signature.as_bytes().to_vec();
        

        let tokens = vec![
            web3::ethabi::Token::Bytes(order.uid.0.0.to_vec()),
            web3::ethabi::Token::Address(order.sell.token.0.into()),
            web3::ethabi::Token::Uint(order.sell.amount.0),
            web3::ethabi::Token::Address(order.buy.token.0.into()),
            web3::ethabi::Token::Uint(order.buy.amount.0),
            web3::ethabi::Token::Uint(u32::from(order.valid_to).into()),
            web3::ethabi::Token::Uint(chain_id.into()), // Add chainId
            web3::ethabi::Token::Uint(random_nonce),
            web3::ethabi::Token::Address(vault_address.into()), // Add contractAddress
            web3::ethabi::Token::Address(settlement_contract_address.0.into()), // Add sender
            web3::ethabi::Token::Array(vec![web3::ethabi::Token::Bytes(signature)]),
        ];
        
        let calldata = function.encode_input(&tokens).map_err(|_| competition::Error::MalformedRequest)?;

        let interaction = solution::Interaction::Custom(solution::interaction::Custom {
            target: eth::ContractAddress(vault_address.into()),
            value: eth::Ether(eth::U256::zero()),
            call_data: Bytes(calldata),
            allowances: vec![
                eth::Allowance {
                    token: order.sell.token,
                    spender: eth::Address(vault_address),
                    amount: order.sell.amount.0,
                }.into()
            ],
            inputs: vec![order.sell],
            outputs: vec![order.buy],
            internalize: false,
        });
        interactions.push(interaction);
    }

    let solution = competition::Solution::new(
        solution::Id::new(0),
        trades,
        prices.clone(),
        vec![], // pre_interactions
        interactions,
        vec![], // post_interactions
        solver.clone(),
        weth_address,
        None,
        FeeHandler::Driver,
        &HashSet::new(),
        HashMap::new(),
    ).map_err(|e| {
         tracing::error!(?e, "Failed to create solution");
         competition::Error::SolutionNotAvailable
    })?;

    let solved = competition::Solved {
        id: solution::Id::new(0),
        score: eth::Ether(0.into()),
        trades: solved_trades,
        prices: prices.iter().map(|(k,v)| (*k, eth::TokenAmount(*v))).collect(),
        gas: None,
    };

    Ok((solved, solution))
}