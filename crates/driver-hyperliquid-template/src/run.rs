use {
    crate::{
        // app, serve
        infra::{ 
            Api
        }, 
    },
    futures::future::join_all,
    driver::{
        domain::{
            competition::{bad_tokens, order::app_data::AppDataRetriever},
        },
        infra::{
            self,
            blockchain::{self, Ethereum},
            cli::{self,Args}, 
            config, liquidity, tokens, Solver, notify
        },
    },
    clap::Parser,
    std::{net::SocketAddr, sync::Arc, time::Duration},
    tokio::sync::oneshot,
};
pub async fn start(args: impl Iterator<Item = String>) {
    let args = Args::parse_from(args);
    run_with(args, None).await
}
pub async fn run(
    args: impl Iterator<Item = String>,
    addr_sender: Option<oneshot::Sender<SocketAddr>>,
) {
    let args = Args::parse_from(args);
    run_with(args, addr_sender).await;
}
async fn run_with(args: Args, addr_sender: Option<oneshot::Sender<SocketAddr>>) {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    tracing_subscriber::fmt::init();
    // let app = app();
    // if let Some(sender) = addr_sender {
    //     let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    //     sender.send(addr).unwrap();
    // }
    let ethrpc = ethrpc(&args).await;
    let web3 = ethrpc.web3().clone();
    let config = config::file::load(ethrpc.chain(), &args.config).await;
    let commit_hash = option_env!("VERGEN_GIT_SHA").unwrap_or("COMMIT_INFO_NOT_FOUND");

    tracing::info!(%commit_hash, "running driver with {config:#?}");

    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let eth = ethereum(&config, ethrpc).await;
    let simulator = simulator(&config, &eth).await;
    let mempools = mempools(&config, &eth, &web3).await;
    let app_data_retriever = match &config.app_data_fetching {
        config::file::AppDataFetching::Enabled {
            orderbook_url,
            cache_size,
        } => Some(AppDataRetriever::new(orderbook_url.clone(), *cache_size)),
        config::file::AppDataFetching::Disabled => None,
    };
    let api = Api{
        solvers: solvers(&config, &eth).await,
        liquidity: liquidity(&config, &eth).await,
        liquidity_config: config.liquidity.clone(),
        liquidity_sources_notifier: liquidity_sources_notifier(&config, &eth),
        tokens: tokens::Fetcher::new(&eth),
        eth: eth.clone(),
        simulator,
        mempools,
        bad_token_detector: bad_tokens::simulation::Detector::new(
            config.simulation_bad_token_max_age,
            &eth,
        ),
        addr: args.addr,
        addr_sender,
    }.serve(async{
        let _ = shutdown_receiver.await;
    },
    config.order_priority_strategies,
    app_data_retriever,
    );
    futures::pin_mut!(api);
    tokio::select! {
        result = &mut api => panic!("serve task exited: {result:?}"),
        _ = shutdown_signal() => {
            tracing::info!("Gracefully shutting down API");
            shutdown_sender.send(()).expect("failed to send shutdown signal");
            // Shutdown timeout needs to be larger than the auction deadline
            match tokio::time::timeout(Duration::from_secs(20), api).await {
                Ok(inner) => inner.expect("API failed during shutdown"),
                Err(_) => panic!("API shutdown exceeded timeout"),
            }
        }
    };

    // serve(app, args.port).await;
}
async fn ethereum(config: &infra::Config, ethrpc: blockchain::Rpc) -> Ethereum {
    let gas = Arc::new(
        blockchain::GasPriceEstimator::new(ethrpc.web3(), &config.gas_estimator, &config.mempools)
            .await
            .expect("initialize gas price estimator"),
    );
    Ethereum::new(ethrpc, config.contracts.clone(), gas, config.tx_gas_limit).await
}
async fn ethrpc(args: &cli::Args) -> blockchain::Rpc {
    let args = blockchain::RpcArgs {
        url: args.ethrpc.clone(),
        max_batch_size: args.ethrpc_max_batch_size,
        max_concurrent_requests: args.ethrpc_max_concurrent_requests,
    };
    blockchain::Rpc::try_new(args)
        .await
        .expect("connect ethereum RPC")
}

#[cfg(unix)]
async fn shutdown_signal() {
    // Intercept signals for graceful shutdown. Kubernetes sends sigterm, Ctrl-C
    // sends sigint.
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .unwrap()
            .recv()
            .await
    };
    let sigint = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .unwrap()
            .recv()
            .await;
    };
    futures::pin_mut!(sigint);
    futures::pin_mut!(sigterm);
    futures::future::select(sigterm, sigint).await;
}

async fn solvers(config: &config::Config, eth: &Ethereum) -> Vec<Solver> {
    join_all(
        config
            .solvers
            .iter()
            .map(
                |config| async move { Solver::try_new(config.clone(), eth.clone()).await.unwrap() },
            )
            .collect::<Vec<_>>(),
    )
    .await
}

async fn liquidity(config: &config::Config, eth: &Ethereum) -> liquidity::Fetcher {
    liquidity::Fetcher::try_new(eth, &config.liquidity)
        .await
        .expect("initialize liquidity fetcher")
}

fn liquidity_sources_notifier(
    config: &config::Config,
    eth: &Ethereum,
) -> notify::liquidity_sources::Notifier {
    let notifier_config = config
        .liquidity_sources_notifier
        .as_ref()
        .unwrap_or(&notify::liquidity_sources::config::Config { liquorice: None });
    notify::liquidity_sources::Notifier::try_new(notifier_config, eth.chain())
        .expect("initialize notify sources notifier")
}

async fn simulator(config: &config::Config, eth: &Ethereum) -> driver::infra::Simulator {
    let mut simulator = match &config.simulator {
        Some(driver::infra::simulator::Config::Tenderly(tenderly)) => {
            driver::infra::Simulator::tenderly(
                driver::infra::simulator::tenderly::Config {
                    url: tenderly.url.clone(),
                    api_key: tenderly.api_key.clone(),
                    user: tenderly.user.clone(),
                    project: tenderly.project.clone(),
                    save: tenderly.save,
                    save_if_fails: tenderly.save_if_fails,
                },
                eth.clone(),
            )
        }
        Some(driver::infra::simulator::Config::Enso(enso)) => {
            driver::infra::Simulator::enso(
                driver::infra::simulator::enso::Config {
                    url: enso.url.clone(),
                    network_block_interval: enso.network_block_interval,
                },
                eth.clone(),
            )
        }
        None => driver::infra::Simulator::ethereum(eth.clone()),
    };
    if config.disable_access_list_simulation {
        simulator.disable_access_lists();
    }
    if let Some(gas) = config.disable_gas_simulation {
        simulator.disable_gas(gas);
    }
    simulator
}

async fn mempools(
    config: &config::Config,
    eth: &Ethereum,
    web3: &driver::boundary::Web3,
) -> driver::domain::Mempools {
    driver::domain::Mempools::try_new(
        config
            .mempools
            .iter()
            .map(|config| driver::infra::Mempool::new(config.clone(), web3.clone()))
            .collect(),
        eth.clone(),
    )
    .expect("initialize mempools")
}
