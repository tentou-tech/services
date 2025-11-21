use {
    crate::infra::api::{State, error::{Error, Kind}},
    driver::{
        domain::{
            competition::{self, auction},
            eth,
            liquidity,
            quote::{self, Quote, QuotingFailed},
        },
        infra::{
            observe,
            solver::Solver,
        },
    },
    tracing::Instrument,
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
        
        let result = custom_quote(
            &order,
            state.eth(),
            state.solver(),
            state.tokens(),
        ).await;

        observe::quoted(state.solver().name(), &order, &result);
        let quote = result.map_err(|err| -> (hyper::StatusCode, axum::Json<Error>) { Kind::from(err).into() })?;
        Ok(axum::response::Json(dto::Quote::new(quote)))
    };

    handle_request
        .instrument(tracing::info_span!("/quote", solver = %state.solver().name()))
        .await
}

async fn custom_quote(
    order: &quote::Order,
    eth: &driver::infra::Ethereum,
    solver: &Solver,
    tokens: &driver::infra::tokens::Fetcher,
) -> Result<Quote, quote::Error> {
    let liquidity = get_fake_liquidity(order.tokens.sell(), order.tokens.buy());

    let auction = order.fake_auction(eth, tokens, solver.quote_using_limit_orders())
        .await?;
    
    // 4. Run the solver (fake).
    let solutions = fake_solve(solver, eth, &auction, &liquidity).await?;

    // 5. Create a quote.
    let quote = Quote::try_new(
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
) -> Result<Vec<driver::domain::competition::Solution>, quote::Error> {
    use driver::infra::solver::dto;
    use solvers_dto::solution::{
        Solutions, Solution, Trade, Fulfillment, OrderUid, Interaction, LiquidityInteraction,
    };
    use std::collections::HashMap;
    use web3::types::{H160, U256};

    let mut trades = Vec::new();
    let mut prices = HashMap::new();
    let mut interactions = Vec::new();

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

        // Create a liquidity interaction for the swap
        // We use the fake liquidity ID "0" which corresponds to the one created in get_fake_liquidity
        let liquidity_interaction = LiquidityInteraction {
            internalize: false,
            id: "0".to_string(),
            input_token: sell_token,
            output_token: buy_token,
            input_amount: executed_amount,
            output_amount: executed_amount, // 1:1 price
        };
        interactions.push(Interaction::Liquidity(liquidity_interaction));
    }

    let solution = Solution {
        id: 0,
        prices,
        trades,
        pre_interactions: vec![],
        interactions,
        post_interactions: vec![],
        gas: None,
        flashloans: None,
    };

    let solutions_dto = Solutions {
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
        quote::Error::QuotingFailed(quote::QuotingFailed::NoSolutions)
    })?;

    Ok(domain_solutions)
}

async fn custom_solve(
    solver: &driver::infra::solver::Solver,
    eth: &driver::infra::Ethereum,
    auction: &driver::domain::competition::Auction,
    liquidity: &[driver::domain::liquidity::Liquidity],
) -> Result<Vec<driver::domain::competition::Solution>, driver::infra::solver::Error> {
    use driver::{
        infra::{solver::dto, observe},
        util,
        domain::time::Remaining,
    };
    // use observe::tracing::tracing_headers; // Commented out to avoid import error for now
    use std::time::Instant;
    use tracing::Instrument;

    let start = Instant::now();

    let flashloan_hints = solver.assemble_flashloan_hints(auction);
    let weth = eth.contracts().weth_address();
    let auction_dto = dto::auction::new(
        auction,
        liquidity,
        weth,
        solver.config().fee_handler,
        solver.config().solver_native_token,
        &flashloan_hints,
        auction.deadline(solver.timeouts()).solvers(),
    );

    let body = {
        const BYTES_PER_ORDER: usize = 1_300;
        let mut buffer = Vec::with_capacity(auction.orders().len() * BYTES_PER_ORDER);
        serde_json::to_writer(&mut buffer, &auction_dto)?;
        String::from_utf8(buffer).expect("serde_json only writes valid utf8")
    };

    if let Some(id) = auction.id() {
        solver.persistence().archive_auction(id, &auction_dto);
        // observe::metrics::metrics().measure_auction_overhead( // Metrics might be private
        //     start,
        //     "driver",
        //     "serialize_request",
        // );
    }

    let url = solver.config().endpoint.join("solve").expect("valid url");
    observe::solver_request(&url, &body);
    let timeout = match auction.deadline(solver.timeouts()).solvers().remaining() {
        Ok(timeout) => timeout,
        Err(_) => {
            tracing::warn!("auction deadline exceeded before sending request to solver");
            return Ok(Default::default());
        }
    };
    
    let mut req = solver.client()
        .post(url.clone())
        .body(body)
        // .headers(tracing_headers()) // Commented out
        .timeout(timeout);
        
    // if let Some(id) = observe::distributed_tracing::request_id::from_current_span() {
    //     req = req.header("X-REQUEST-ID", id);
    // }
    
    observe::sending_solve_request(solver.name().as_str(), timeout);
    let started_at = std::time::Instant::now();
    let res = util::http::send(solver.config().response_size_limit_max_bytes, req).await;
    observe::solver_response(
        &url,
        res.as_deref(),
        solver.name().as_str(),
        started_at.elapsed(),
    );
    let res = res?;
    let res: solvers_dto::solution::Solutions =
        serde_json::from_str(&res).inspect_err(|err| {
            tracing::warn!(res, ?err, "failed to parse solver response");
            solver.notify(
                auction.id(),
                None,
                driver::infra::notify::Kind::DeserializationError(format!("Request format invalid: {err}")),
            );
        })?;
        
    let solutions = dto::Solutions::from(res).into_domain(
        auction,
        liquidity,
        weth,
        solver.clone(),
        &flashloan_hints,
    )?;

    observe::solutions(&solutions, auction.surplus_capturing_jit_order_owners());
    Ok(solutions)
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