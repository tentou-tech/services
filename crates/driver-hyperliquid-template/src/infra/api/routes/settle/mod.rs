mod dto;

use {
    crate::infra::api::State,
    driver::{
        domain::{competition, eth},
        infra::{
            api::{self, error::Error},
            observe,
            solver,
        },
    },
    tracing::Instrument,
};

pub(in crate::infra::api) fn settle(router: axum::Router<State>) -> axum::Router<State> {
    router.route("/settle", axum::routing::post(route))
}

async fn route(
    state: axum::extract::State<State>,
    req: axum::Json<dto::SettleRequest>,
) -> Result<(), (hyper::StatusCode, axum::Json<Error>)> {
    let auction_id =
        competition::auction::Id::try_from(req.auction_id).map_err(driver::infra::api::routes::solve::AuctionError::from)?;
    let solver = state.solver().name().to_string();

    async move {
        observe::settling();
        let result = state
            .competition()
            .settle(
                auction_id,
                req.solution_id,
                req.submission_deadline_latest_block,
            )
            .await;
        observe::settled(state.solver().name(), &result);
        result.map(|_| ()).map_err(Into::into)
    }
    .instrument(tracing::info_span!("/settle", solver, %auction_id))
    .await
}

