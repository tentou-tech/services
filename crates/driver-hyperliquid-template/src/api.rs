//! This module implements the solver API that is served by the driver.
use axum::Json;
use serde::{Deserialize, Serialize};
use solvers_dto::solution::{JitOrder, Solutions as SolveResponse};
use std::collections::HashMap;
use web3::types::H160;

#[derive(Debug, Deserialize, Serialize)]
pub struct SolveRequest {
    pub id: u64,
    pub orders: Vec<solvers_dto::auction::Order>,
    pub tokens: HashMap<H160, solvers_dto::auction::Token>,
    pub deadline: String,
    #[serde(rename = "surplusCapturingJitOrderOwners")]
    pub surplus_capturing_jit_order_owners: Vec<H160>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RevealRequest {
    pub solution_id: u64,
    pub auction_id: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SettleRequest {
    pub solution_id: u64,
    pub submission_deadline_latest_block: u64,
    pub auction_id: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Calldata {
    pub internalized: String,
    pub uninternalized: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RevealResponse {
    pub calldata: Calldata,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum QuoteResponseKind {
    Legacy(LegacyQuoteResponse),
    Modern(QuoteResponse),
    Error(Error),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LegacyQuoteResponse {
    pub amount: String,
    pub interactions: Vec<solvers_dto::auction::InteractionData>,
    pub solver: String,
    pub gas: u64,
    pub tx_origin: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct QuoteResponse {
    pub clearing_prices: HashMap<String, String>,
    pub pre_interactions: Vec<solvers_dto::auction::InteractionData>,
    pub interactions: Vec<solvers_dto::auction::InteractionData>,
    pub solver: String,
    pub gas: u64,
    pub tx_origin: String,
    pub jit_orders: Vec<JitOrder>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Error {
    pub kind: String,
    pub description: String,
}

/// Get price estimation quote.
pub(super) async fn get_quote(
) -> Result<Json<QuoteResponseKind>, String> {
    let response = QuoteResponse {
        clearing_prices: HashMap::new(),
        pre_interactions: vec![],
        interactions: vec![],
        solver: "0x0000000000000000000000000000000000000000".to_string(),
        gas: 50000,
        tx_origin: "0x0000000000000000000000000000000000000000".to_string(),
        jit_orders: vec![],
    };
    Ok(Json(QuoteResponseKind::Modern(response)))
}
/// Solve the passed in auction.
pub(super) async fn solve(
    Json(_body): Json<SolveRequest>,
) -> Result<Json<SolveResponse>, String> {
    Ok(Json(SolveResponse {
        solutions: vec![],
    }))
}
/// Reveal the calldata of the previously solved auction.
pub(super) async fn reveal_calldata(
    Json(_body): Json<RevealRequest>,
) -> Result<Json<RevealResponse>, String> {
    Ok(Json(RevealResponse {
        calldata: Calldata {
            internalized: "0x1234".to_string(),
            uninternalized: "0x1234".to_string(),
        },
    }))
}
/// Execute the previously solved auction on chain.
pub(super) async fn settle(
    Json(_body): Json<SettleRequest>,
) -> Result<String, String> {
    Ok("".to_string())
}
/// Receive a notification with a specific reason.
pub(super) async fn receive_notification(
    Json(_body): Json<String>,
) -> Result<String, String> {
    Ok("".to_string())
}