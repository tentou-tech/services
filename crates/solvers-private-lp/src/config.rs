use clap::Parser;
use serde::Deserialize;
use web3::types::H160;

/// Configuration for the HyperLiquid Solver.
/// Fields can be loaded from command line arguments, environment variables or a TOML file.
#[derive(Parser, Debug, Clone, Deserialize)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// The path to a TOML configuration file.
    #[arg(long, env)]
    #[serde(skip)] // Don't try to deserialize this from the TOML file
    pub config: Option<String>,
    /// The port to listen on.
    #[arg(long, env, default_value = "8000")]
    pub port: u16,

    /// The address of the settlement contract.
    #[arg(long, env)]
    pub settlement_contract: Option<H160>,

    /// The address of the Vault contract.
    #[arg(long, env)]
    pub vault_address: Option<H160>,

    /// The private key for the solver.
    #[arg(long, env)]
    pub solver_private_key: Option<String>,

    /// The chain ID.
    #[arg(long, env, default_value = "1")]
    pub chain_id: u64,
}

// This struct will be used to deserialize from the TOML file.
// All fields are optional because they can be overridden by CLI arguments.
#[derive(Debug, Deserialize, Default)]
pub struct FileConfig {
    pub port: Option<u16>,
    #[serde(rename = "settlement-contract-address")]
    pub settlement_contract: Option<H160>,
    #[serde(rename = "vault-contract-address")]
    pub vault_address: Option<H160>,
    #[serde(rename = "account")]
    pub solver_private_key: Option<String>,
    #[serde(rename = "chain-id")]
    pub chain_id: Option<u64>,
}
