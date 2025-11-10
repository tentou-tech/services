use {
    crate::{
        domain::liquidity,
        infra::{self, Ethereum},
    },
    anyhow::Result,
    ethrpc::block_stream::CurrentBlockWatcher,
    reqwest::Url,
    shared::{
        http_client::HttpClientFactory,
        kyber_api::DefaultKyberApi,
        price_estimation::gas::GAS_PER_ZEROEX_ORDER,
    },
    solver::{
        liquidity::kyber::{KyberLiquidity, KyberSettlementHandler},
        liquidity::LimitOrder,
        liquidity_collector::LiquidityCollecting,
    },
    std::sync::Arc,
};

/// Convert solver liquidity to domain liquidity
pub fn to_domain(
    id: liquidity::Id,
    limit_order: LimitOrder,
) -> Result<liquidity::Liquidity> {
    let handler = limit_order
        .settlement_handling
        .as_any()
        .downcast_ref::<KyberSettlementHandler>()
        .ok_or_else(|| anyhow::anyhow!("not a KyberSettlementHandler"))?
        .clone();

    Ok(liquidity::Liquidity {
        id,
        gas: GAS_PER_ZEROEX_ORDER.into(), // Kyber gas is similar to ZeroEx
        kind: liquidity::Kind::Kyber(liquidity::kyber::KyberSwap {
            method: handler.method,
        }),
    })
}

/// Create the Kyber liquidity collector
pub async fn collector(
    eth: &Ethereum,
    blocks: CurrentBlockWatcher,
    config: &infra::liquidity::config::Kyberswap,
) -> Result<Box<dyn LiquidityCollecting>> {
    let eth = eth.with_metric_label("kyber".into());
    let settlement = eth.contracts().settlement().clone();
    let web3 = eth.web3().clone();

    // Parse the routing API URL
    let url = Url::parse(&config.routing_api)
        .map_err(|_| anyhow::anyhow!("invalid Kyber routing API URL"))?;

    // Create HTTP client
    let http_client_factory = HttpClientFactory::default();
    
    // Create Kyber API client
    let api = Arc::new(DefaultKyberApi::new(
        http_client_factory.builder(),
        url,
    )?);

    tracing::info!(
        "Initializing Kyber liquidity collector with API: {}",
        config.routing_api
    );

    Ok(Box::new(
        KyberLiquidity::new(web3, api, settlement, blocks).await,
    ))
}
