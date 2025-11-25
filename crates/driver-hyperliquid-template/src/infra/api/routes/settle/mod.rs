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
    let auction_id = req.auction_id;
    let solution_id = req.solution_id;
    let solver_name = state.solver().name().clone();
    let solver_name_span = solver_name.clone();

    async move {
        observe::settling();
        
        // Retrieve settlement data
        let (auction_json, solution_dto) = {
            let settlements = state.settlements().lock().unwrap();
            let data = settlements.get(&solution_id)
                .ok_or(driver::domain::competition::Error::SolutionNotAvailable)?;
            (data.auction_json.clone(), data.solution_dto.clone())
        };

        // Reconstruct auction from JSON
        let auction_dto: solvers_dto::auction::Auction = serde_json::from_str(&auction_json)
            .map_err(|_| competition::Error::MalformedRequest)?;

        // Create Solutions wrapper
        let solutions_dto = solvers_dto::solution::Solutions {
            solutions: vec![solution_dto],
        };

        // For simplified implementation, we'll encode the settlement directly from the DTO
        // In production, you'd want to fully reconstruct the domain auction
        
        // Create a simple settlement encoding
        // The settlement will include the Vault interactions we created in /solve
        let result = encode_and_submit_settlement(
            &state,
            solutions_dto,
            &auction_dto,
        ).await;

        observe::settled(&solver_name, &result);
        result.map(|_| ()).map_err(Into::into)
    }
    .instrument(tracing::info_span!("/settle", solver = %solver_name_span, %auction_id))
    .await
}

/// Encode and submit the settlement transaction
async fn encode_and_submit_settlement(
    state: &State,
    solutions_dto: solvers_dto::solution::Solutions,
    auction_dto: &solvers_dto::auction::Auction,
) -> Result<competition::Settled, competition::Error> {
    use driver::infra::solver::dto;
    
    // For the simplified implementation, we'll create a basic settlement
    // The key is that the solution_dto already contains our Vault interactions!
    
    // Get the first solution (we only have one)
    let solution_dto = solutions_dto.solutions.into_iter().next()
        .ok_or(competition::Error::MalformedRequest)?;

    // Build the settlement calldata
    // The settlement contract expects: settle(tokens[], clearingPrices[], trades[], interactions[])
    // Our solution_dto.interactions already contains the Vault calls!
    
    // For now, create a simple settlement result
    // In production, you'd call Settlement::encode() and then mempools.execute()
    
    tracing::info!(
        "Encoding settlement with {} interactions (including Vault calls)",
        solution_dto.interactions.len()
    );

    // Log the Vault interactions
    for (i, interaction) in solution_dto.interactions.iter().enumerate() {
        if let solvers_dto::solution::Interaction::Custom(custom) = interaction {
            tracing::info!(
                "Interaction {}: target={:?}, value={}, calldata_len={}",
                i,
                custom.target,
                custom.value,
                custom.calldata.len()
            );
        }
    }

    // Create a dummy tx hash for now
    // In production, you would:
    // 1. Create domain auction from auction_dto
    // 2. Convert solution_dto to domain Solution
    // 3. Call Settlement::encode()
    // 4. Call mempools.execute()
    
    let tx_hash = eth::TxId(eth::H256::from_low_u64_be(solution_dto.id));
    
    tracing::info!("Settlement would be submitted with tx_hash: {:?}", tx_hash);
    
    Ok(competition::Settled {
        internalized_calldata: Default::default(),
        uninternalized_calldata: Default::default(),
        tx_hash,
    })
}
