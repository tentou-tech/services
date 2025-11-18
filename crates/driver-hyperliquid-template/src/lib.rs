use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
pub mod api;
mod run;

pub mod infra;

pub use self::run::{run, start};

pub fn app(
) -> Router {
    Router::new()
        .route("/quote", get(api::get_quote))
        .route("/solve", post(api::solve))
        .route("/reveal", post(api::reveal_calldata))
        .route("/settle", post(api::settle))
        .route("/notify", post(api::receive_notification))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
        )
}
pub async fn serve(app: Router, port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}