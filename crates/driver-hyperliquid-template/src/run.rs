use {
    crate::{
        // app, serve
        infra::{ 
            Api
        }, 
    },
    futures::future::join_all,
    driver::infra::{
        self,
        blockchain::{self, Ethereum},
        cli::{self,Args}, 
        config, Solver
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
    let api = Api{
        solvers: solvers(&config, &eth).await,
        addr: args.addr,
        addr_sender,
    }.serve(async{
        let _ = shutdown_receiver.await;
    }
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
