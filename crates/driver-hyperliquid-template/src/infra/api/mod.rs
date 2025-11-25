use {
    driver::{
        domain::{competition, Mempools},
        infra::{
            Simulator,
            blockchain::Ethereum,
            liquidity,
            solver::Solver,
            tokens,
        },
    },
    futures::Future,
    std::{
        collections::HashMap,
        net::SocketAddr,
        sync::{Arc, Mutex},
    },
    tokio::sync::oneshot,
};

mod error;
pub mod routes;

const REQUEST_BODY_LIMIT: usize = 10 * 1024 * 1024;

/// Stored settlement data for a solution
pub struct SettlementData {
    pub solved: competition::Solved,
    pub auction_json: String,
    pub solution_dto: solvers_dto::solution::Solution,
}

pub struct Api {
    pub solvers: Vec<Solver>,
    pub eth: Ethereum,
    pub liquidity: liquidity::Fetcher,
    pub tokens: tokens::Fetcher,
    pub simulator: Simulator,
    pub mempools: Mempools,
    pub addr: SocketAddr,
    /// If this channel is specified, the bound address will be sent to it. This
    /// allows the driver to bind to 0.0.0.0:0 during testing.
    pub addr_sender: Option<oneshot::Sender<SocketAddr>>,
}

#[derive(Clone)]
struct State(Arc<Inner>);

impl State {
    fn eth(&self) -> &Ethereum {
        &self.0.eth
    }

    fn solver(&self) -> &Solver {
        &self.0.solver
    }

    fn liquidity(&self) -> &liquidity::Fetcher {
        &self.0.liquidity
    }

    fn tokens(&self) -> &tokens::Fetcher {
        &self.0.tokens
    }

    fn simulator(&self) -> &Simulator {
        &self.0.simulator
    }

    fn mempools(&self) -> &Mempools {
        &self.0.mempools
    }

    fn settlements(&self) -> &Arc<Mutex<HashMap<u64, SettlementData>>> {
        &self.0.settlements
    }
}

struct Inner {
    eth: Ethereum,
    solver: Solver,
    liquidity: liquidity::Fetcher,
    tokens: tokens::Fetcher,
    simulator: Simulator,
    mempools: Mempools,
    settlements: Arc<Mutex<HashMap<u64, SettlementData>>>,
}

impl Api {
    pub async fn serve(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static
    ) -> Result<(), hyper::Error> {
        // Add middleware.
       
        let mut app = axum::Router::new()
            .layer(tower::ServiceBuilder::new().layer(
            tower_http::limit::RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT),
        ));

        // Add the metrics, healthz, and gasprice endpoints.
        app = routes::metrics(app);
        app = routes::healthz(app);


        // Multiplex each solver as part of the API. Multiple solvers are multiplexed
        // on the same driver so only one liquidity collector collects the liquidity
        // for all of them. This is important because liquidity collection is
        // computationally expensive for the Ethereum node.
        for solver in self.solvers {
            let name = solver.name().clone();
            let router = axum::Router::new();
            let router = routes::info(router);
            let router = routes::quote(router);
            let router = routes::solve(router);
            let router = routes::reveal(router);
            let router = routes::settle(router);
            let router = routes::notify(router);

            let router = router.with_state(State(Arc::new(Inner{
                eth: self.eth.clone(),
                solver: solver.clone(),
                liquidity: self.liquidity.clone(),
                tokens: self.tokens.clone(),
                simulator: self.simulator.clone(),
                mempools: self.mempools.clone(),
                settlements: Default::default(),
            })));
            let path = format!("/{name}");
            app = app.nest(&path, router);
        }

        app = app
            // axum's default body limit needs to be disabled to not have the default limit on top of our custom limit
            .layer(axum::extract::DefaultBodyLimit::disable())
            .layer(
                tower::ServiceBuilder::new()
                    .layer(tower_http::trace::TraceLayer::new_for_http())
            );

        // Start the server.
        let server = axum::Server::bind(&self.addr).serve(app.into_make_service());
        tracing::info!(port = server.local_addr().port(), "serving driver");
        if let Some(addr_sender) = self.addr_sender {
            addr_sender.send(server.local_addr()).unwrap();
        }
        server.with_graceful_shutdown(shutdown).await
    }

    
}


