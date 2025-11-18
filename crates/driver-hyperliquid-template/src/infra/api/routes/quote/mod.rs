use {
    crate::infra::api::State,
    driver::infra::{
        api::{error::Error},
        observe,
    },
    tracing::Instrument,
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
        // let order = order.0.into_domain().inspect_err(|err| {
        //     observe::invalid_dto(err, "order");
        // })?;
        // // observe::quoting(&order);
        // let quote = order
        //     .quote(
        //         state.solver(),
        //     )
        //     .await;
        // observe::quoted(state.solver().name(), &order, &quote);
        
        let empty_quote = driver::domain::quote::Quote {
            clearing_prices: std::collections::HashMap::new(),
            pre_interactions: vec![],
            interactions: vec![],
            solver: driver::domain::eth::Address::default(),
            gas: None,
            tx_origin: None,
            jit_orders: vec![],
        };
        Ok(axum::response::Json(dto::Quote::new(empty_quote))) 
    };

    handle_request
        .instrument(tracing::info_span!("/quote", solver = %state.solver().name()))
        .await
}