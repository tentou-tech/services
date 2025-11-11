//! Boundary layer for converting KyberSwap liquidity between solver and driver domains.

use {
    crate::{
        domain::liquidity::{self, kyberswap},
        infra::{self, Ethereum},
    },
    anyhow::anyhow,
    ethcontract::Bytes,
    shared::{http_client::HttpClientFactory, kyberswap_api::DefaultKyberSwapApi},
    solver::{
        liquidity::{LimitOrder, kyberswap::KyberSwapLiquidity},
        liquidity_collector::LiquidityCollecting,
    },
    std::sync::Arc,
};

/// Converts a solver KyberSwap limit order to a driver domain liquidity
pub fn to_domain(
    id: liquidity::Id,
    limit_order: LimitOrder,
) -> anyhow::Result<liquidity::Liquidity> {
    // Downcast the settlement handler to KyberSwapSettlementHandler
    let handler = limit_order
        .settlement_handling
        .as_any()
        .downcast_ref::<solver::liquidity::kyberswap::KyberSwapSettlementHandler>()
        .ok_or(anyhow!("not a kyberswap::KyberSwapSettlementHandler"))?
        .clone();

    // Extract route data from the handler
    let solver_route = &handler.route;

    // Convert to driver domain model
    let route = kyberswap::KyberSwapRoute {
        token_in: solver_route.token_in,
        token_out: solver_route.token_out,
        amount_in: solver_route.amount_in,
        amount_out: solver_route.amount_out,
        encoded_data: Bytes(solver_route.encoded_data.0.clone()),
        router_address: solver_route.router_address,
        gas_estimate: solver_route.gas_estimate,
    };

    Ok(liquidity::Liquidity {
        id,
        gas: route.gas_estimate.into(),
        kind: liquidity::Kind::KyberSwap(route),
    })
}

/// Factory function to create a KyberSwap liquidity collector
pub async fn collector(
    eth: &Ethereum,
    config: &infra::liquidity::config::KyberSwap,
) -> anyhow::Result<Box<dyn LiquidityCollecting>> {
    let eth = eth.with_metric_label("kyberswap".into());
    let settlement = eth.contracts().settlement().clone();
    let web3 = eth.web3().clone();

    // Create HTTP client
    let http_client_factory = &HttpClientFactory::new(&shared::http_client::Arguments {
        http_timeout: config.http_timeout,
    });

    // Create KyberSwap API client
    let api_config = shared::kyberswap_api::KyberSwapConfig {
        api_url: config.api_url.clone(),
        chain_name: config.chain_name.clone(),
        http_timeout: config.http_timeout,
        client_id: config.client_id.clone(),
    };

    // let api = Arc::new(DefaultKyberSwapApi::new(
    //     http_client_factory.builder(),
    //     api_config,
    // )?);
    let api = Arc::new(DefaultKyberSwapApi::new(
        reqwest::ClientBuilder::new(),
        api_config,
    ).unwrap());

    // Create KyberSwap liquidity collector
    Ok(Box::new(
        KyberSwapLiquidity::new(
            api,
            config.meta_aggregator_router.0,
            settlement.address(),
            config.slippage_bps,
            config.cache_ttl,
            web3,
        )
        .await,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_types::{H160, U256};

    #[test]
    fn test_to_domain_conversion() {
        let solver_route = solver::liquidity::kyberswap::KyberSwapRoute {
            token_in: H160::from_low_u64_be(1),
            token_out: H160::from_low_u64_be(2),
            amount_in: U256::from(1000),
            amount_out: U256::from(900),
            encoded_data: ethcontract::Bytes(vec![0x12, 0x34]),
            router_address: H160::from_low_u64_be(99),
            gas_estimate: 150000,
        };

        let allowances = Arc::new(solver::interactions::allowances::Allowances::new(
            H160::from_low_u64_be(99),
            std::collections::HashMap::new(),
        ));

        let handler = solver::liquidity::kyberswap::KyberSwapSettlementHandler {
            route: solver_route.clone(),
            meta_aggregator_router: H160::from_low_u64_be(99),
            allowances,
        };

        let limit_order = LimitOrder {
            id: solver::liquidity::LimitOrderId::Liquidity(
                solver::liquidity::LiquidityOrderId::Protocol(model::order::OrderUid([0u8; 56])),
            ),
            sell_token: solver_route.token_in,
            buy_token: solver_route.token_out,
            sell_amount: solver_route.amount_in,
            buy_amount: solver_route.amount_out,
            kind: model::order::OrderKind::Sell,
            partially_fillable: false,
            user_fee: U256::zero(),
            settlement_handling: Arc::new(handler),
            exchange: solver::liquidity::Exchange::ZeroEx,
        };

        let domain_liquidity = to_domain(liquidity::Id(1), limit_order).unwrap();

        assert_eq!(domain_liquidity.id, liquidity::Id(1));
        assert_eq!(domain_liquidity.gas.0, U256::from(150000));
        
        match domain_liquidity.kind {
            liquidity::Kind::KyberSwap(route) => {
                assert_eq!(route.token_in, solver_route.token_in);
                assert_eq!(route.token_out, solver_route.token_out);
                assert_eq!(route.amount_in, solver_route.amount_in);
                assert_eq!(route.amount_out, solver_route.amount_out);
                assert_eq!(route.router_address, solver_route.router_address);
            }
            _ => panic!("Expected KyberSwap liquidity kind"),
        }
    }
}

