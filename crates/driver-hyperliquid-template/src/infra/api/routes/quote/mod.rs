use {
    crate::infra::api::{State, error::{Error, Kind}},
    driver::{
        domain::{
            competition::{self, order},
            eth,
            liquidity,
            quote as quote_domain,
        },
        infra::{
            api::{self, error},
            observe,
            solver::Solver,
        },
    },tracing::Instrument,
    std::collections::HashSet,
    anyhow::Result,
};

mod dto;

pub use dto::OrderError;

pub(in crate::infra::api) fn quote(router: axum::Router<State>) -> axum::Router<State> {
    router.route("/quote", axum::routing::get(route))
}

async fn route(
    state: axum::extract::State<State>,
    order: axum::extract::Query<dto::Order>,
) -> Result<axum::Json<dto::Quote>, (hyper::StatusCode, axum::Json<Error>)> {
    let handle_request = async {
        let order = order.0.into_domain().inspect_err(|err| {
            observe::invalid_dto(err, "order");
        }).map_err(|err| -> (hyper::StatusCode, axum::Json<Error>) { Kind::from(err).into() })?;
        observe::quoting(&order);

        let result = order.quote(state.eth(), state.solver(), state.liquidity(), state.liquidity_config(), state.tokens()).await;

        observe::quoted(state.solver().name(), &order, &result);
        let quote = result.map_err(|err| -> (hyper::StatusCode, axum::Json<Error>) { Kind::from(err).into() })?;
        Ok(axum::response::Json(dto::Quote::new(quote)))
    };

    handle_request
        .instrument(tracing::info_span!("/quote", solver = %state.solver().name()))
        .await
}

async fn custom_quote(
    order: &quote_domain::Order,
    eth: &driver::infra::Ethereum,
    solver: &Solver,
    tokens: &driver::infra::tokens::Fetcher,
) -> Result<quote_domain::Quote, quote_domain::Error> {
    let liquidity = get_fake_liquidity(order.tokens.sell(), order.tokens.buy());

    let auction = order.fake_auction(eth, tokens, solver.quote_using_limit_orders())
        .await?;
    
    // 4. Run the solver (fake).
    let solutions = fake_solve(solver, eth, &auction, &liquidity).await?;

    // 5. Create a quote.
    let quote = quote_domain::Quote::try_new(
        eth,
        solutions
            .into_iter()
            .find(|solution| !solution.is_empty(auction.surplus_capturing_jit_order_owners()))
            .ok_or(driver::domain::quote::QuotingFailed::NoSolutions)?,
    )?;

    Ok(quote)
}

async fn fake_solve(
    solver: &driver::infra::solver::Solver,
    eth: &driver::infra::Ethereum,
    auction: &driver::domain::competition::Auction,
    liquidity: &[driver::domain::liquidity::Liquidity],
) -> Result<Vec<driver::domain::competition::Solution>, quote_domain::Error> {
    use driver::infra::solver::dto;
    use solvers_dto::solution::{
        Solutions, Solution, Trade, Fulfillment, OrderUid, Interaction, LiquidityInteraction,
    };
    use std::collections::HashMap;
    use web3::types::{H160, U256};

    let mut trades = Vec::new();
    let mut prices = HashMap::new();


    for order in auction.orders() {
        // Determine executed amount based on partial fill availability
        let target_amount = match &order.partial {
            competition::order::Partial::Yes { available } => available.0,
            competition::order::Partial::No => order.target().0,
        };

        // Convert primitive_types::U256 to web3::types::U256
        let amount_bytes = [0u8; 32];
        let mut amount_bytes = amount_bytes;
        target_amount.to_big_endian(&mut amount_bytes);
        let executed_amount = U256::from_big_endian(&amount_bytes);

        // Determine fee based on whether solver determines fee
        let fee = if order.solver_determines_fee() {
            Some(U256::zero())
        } else {
            None
        };

        let fulfillment = Fulfillment {
            order: OrderUid(order.uid.0.0),
            executed_amount,
            fee,
        };
        trades.push(Trade::Fulfillment(fulfillment));

        // Set 1:1 prices
        let sell_token = H160(order.sell.token.0.0.into());
        let buy_token = H160(order.buy.token.0.0.into());
        
        // 1 ETH (1e18)
        let price = U256::from_dec_str("1000000000000000000").unwrap();
        prices.insert(sell_token, price);
        prices.insert(buy_token, price);
    }
    
    // Create interactions
    let mut interactions: Vec<solvers_dto::solution::Interaction> = Vec::new();
    for order in auction.orders() {
        // Encode the "exchange" function call for the Vault contract
        // Function signature: exchange(bytes,address,uint256,address,uint256,uint32,uint256,bytes[])
        // We use a dummy Vault address and dummy signatures for now.

        // vault address for hyperliquid testnet
        let vault_address = "0x24be1e421e38a9ef728c5c36a69d724820ab58ed".parse::<H160>().unwrap();

        // Construct the function selector and encode arguments manually using web3::ethabi
        // Since we don't have the full contract binding, we use raw encoding.
        // exchange signature: 0x...
        
        // Define the function interface
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

        let executed_amount = if order.side == driver::domain::competition::order::Side::Sell {
            order.sell.amount.0
        } else {
            order.buy.amount.0
        };

        // Use 1:1 exchange rate for simplicity
        let input_amount = executed_amount;
        let output_amount = executed_amount;

        let tokens = vec![
            web3::ethabi::Token::Bytes(order.uid.0.0.to_vec()),
            web3::ethabi::Token::Address(order.sell.token.0.0.into()),
            web3::ethabi::Token::Uint(input_amount),
            web3::ethabi::Token::Address(order.buy.token.0.0.into()),
            web3::ethabi::Token::Uint(output_amount),
            web3::ethabi::Token::Uint(U256::from(order.valid_to.0)),
            web3::ethabi::Token::Uint(U256::zero()), // nonce
            web3::ethabi::Token::Array(vec![]), // signatures
        ];

        let calldata = function.encode_input(&tokens).map_err(|e| {
            tracing::error!(?e, "Failed to encode vault exchange calldata");
            quote_domain::Error::QuotingFailed(quote_domain::QuotingFailed::NoSolutions)
        })?;

        let interaction = solvers_dto::solution::Interaction::Custom(solvers_dto::solution::CustomInteraction {
            internalize: false,
            target: vault_address,
            value: U256::zero(),
            calldata,
            allowances: vec![solvers_dto::solution::Allowance {
                token: order.sell.token.0.0.into(),
                spender: vault_address,
                amount: input_amount,
            }],
            inputs: vec![solvers_dto::solution::Asset {
                token: order.sell.token.0.0.into(),
                amount: input_amount,
            }],
            outputs: vec![solvers_dto::solution::Asset {
                token: order.buy.token.0.0.into(),
                amount: output_amount,
            }],
        });

        interactions.push(interaction);

        // Add a fulfillment trade so the solution is not considered empty
        let executed_amount = match order.side {
            driver::domain::competition::order::Side::Sell => input_amount,
            driver::domain::competition::order::Side::Buy => output_amount,
        };

        trades.push(solvers_dto::solution::Trade::Fulfillment(solvers_dto::solution::Fulfillment {
            order: solvers_dto::solution::OrderUid(order.uid.0.0),
            executed_amount,
            fee: None,
        }));
    }

    let solution = solvers_dto::solution::Solution {
        id: 0,
        prices: prices.clone(), // Use clone as prices is used later
        trades,
        pre_interactions: vec![],
        interactions,
        post_interactions: vec![],
        gas: None,
        flashloans: None,
    };

    let solutions_dto = solvers_dto::solution::Solutions {
        solutions: vec![solution],
    };

    let weth = eth.contracts().weth_address();
    let flashloan_hints = solver.assemble_flashloan_hints(auction);

    let domain_solutions = dto::Solutions::from(solutions_dto).into_domain(
        auction,
        liquidity,
        weth,
        solver.clone(),
        &flashloan_hints,
    ).map_err(|e| {
        tracing::error!(?e, "fake solve conversion failed");
        quote_domain::Error::QuotingFailed(quote_domain::QuotingFailed::NoSolutions)
    })?;

    Ok(domain_solutions)
}

fn get_fake_liquidity(
    sell_token: driver::domain::eth::TokenAddress,
    buy_token: driver::domain::eth::TokenAddress,
) -> Vec<liquidity::Liquidity> {
    use driver::domain::{eth, liquidity::{self, uniswap}};

    // Create fake reserves
    let reserves = uniswap::v2::Reserves::try_new(
        eth::Asset {
            token: sell_token,
            amount: eth::TokenAmount::from(1_000_000_000_000_000_000_000_u128), // 1000 units
        },
        eth::Asset {
            token: buy_token,
            amount: eth::TokenAmount::from(1_000_000_000_000_000_000_000_u128), // 1000 units
        },
    ).expect("tokens must be different");

    // Create a fake pool
    let pool = uniswap::v2::Pool {
        address: eth::H160::from_low_u64_be(1).into(),
        router: eth::ContractAddress(eth::H160::from_low_u64_be(2).into()),
        reserves,
    };

    vec![liquidity::Liquidity {
        id: liquidity::Id(0),
        gas: eth::Gas::from(100_000),
        kind: liquidity::Kind::UniswapV2(pool),
    }]
}