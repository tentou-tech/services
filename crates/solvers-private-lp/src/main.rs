use axum::{
    routing::{post, get},
    Router,
    Json,
    extract::{State},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;
use clap::Parser;
use std::fs;

mod api;
mod config;
mod solver;

use api::DefaultHyperLiquidApi;
use config::{Config, FileConfig};
use solver::{HyperLiquidSolver, SolverConfig};
use solvers_dto::{auction::Auction, solution::Solutions};

struct AppState {
    solver: HyperLiquidSolver,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive(tracing::Level::INFO.into()))
        .init();

    // Parse config from CLI and environment variables.
    let mut config = Config::parse();

    // If a config file path is provided, load from it.
    if let Some(config_path) = &config.config {
        let config_file_content = fs::read_to_string(config_path)?;
        let file_config: FileConfig = toml::from_str(&config_file_content)?;

        // Overlay file config with CLI/Env config (CLI/Env takes precedence)
        // Note: CLI args are Options now. If they are None, we take from file.
        // If file is also None, we have to error later.
        
        if config.port == 8000 && file_config.port.is_some() {
             config.port = file_config.port.unwrap();
        }
        
        if config.settlement_contract.is_none() {
            config.settlement_contract = file_config.settlement_contract;
        }
        if config.vault_address.is_none() {
            config.vault_address = file_config.vault_address;
        }
        if config.solver_private_key.is_none() {
            config.solver_private_key = file_config.solver_private_key;
        }
        // Chain ID has default 1 in CLI config, so we might overwrite it if file specifies it?
        // Actually, if CLI has default, it's hard to distinguish "user set 1" vs "default 1".
        // But for chain ID usually default 1 is fine or overridden.
        // Let's assume if file has it, we use it, unless CLI was explicitly set (which we can't easily check with default).
        // A better pattern with clap is using Option for everything and handle defaults manually.
        // For now, let's respect file config if it exists for chain_id.
        if let Some(chain_id) = file_config.chain_id {
            config.chain_id = chain_id;
        }
    }
    
    // Validate required fields
    let settlement_contract = config.settlement_contract
        .ok_or_else(|| anyhow::anyhow!("settlement_contract is required (CLI or Config File)"))?;
    let vault_address = config.vault_address
        .ok_or_else(|| anyhow::anyhow!("vault_address is required (CLI or Config File)"))?;
    let solver_private_key = config.solver_private_key
        .ok_or_else(|| anyhow::anyhow!("solver_private_key is required (CLI or Config File)"))?;
    
    let solver_config = SolverConfig {
        settlement_contract,
        vault_address,
        solver_private_key,
        chain_id: config.chain_id,
    };

    // Initialize API client
    let api_client = Arc::new(DefaultHyperLiquidApi::new(reqwest::ClientBuilder::new())?);
    
    // Initialize Solver
    let solver = HyperLiquidSolver::new(api_client, solver_config)?;
    
    let app_state = Arc::new(AppState { solver });

    let app = Router::new()
        .route("/solve", post(solve_auction))
        .route("/healthz", get(healthz))
        .with_state(app_state);

    let addr = if let Ok(addr_str) = std::env::var("ADDR") {
        if let Ok(addr) = addr_str.parse::<std::net::SocketAddr>() {
            addr
        } else {
            tracing::warn!("Invalid ADDR format '{}', falling back to 0.0.0.0:{}", addr_str, config.port);
            std::net::SocketAddr::from(([0, 0, 0, 0], config.port))
        }
    } else {
        std::net::SocketAddr::from(([0, 0, 0, 0], config.port))
    };

    tracing::info!("listening on {}", addr);
    
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

async fn solve_auction(
    State(state): State<Arc<AppState>>,
    Json(auction): Json<Auction>,
) -> impl IntoResponse {
    match state.solver.solve(auction).await {
        Ok(solutions) => {
            let response = Solutions { solutions };
            (StatusCode::OK, Json(response)).into_response()
        },
        Err(err) => {
            tracing::error!(?err, "Failed to solve auction");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": err.to_string() }))).into_response()
        }
    }
}

async fn healthz() -> impl IntoResponse {
    StatusCode::OK
}