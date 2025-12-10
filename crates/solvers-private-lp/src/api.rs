use serde::{Deserialize, Serialize};
use web3::types::{H160, U256};
use anyhow::Result;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRequest {
    pub token_in: H160,
    pub token_out: H160,
    pub amount_in: U256,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RouteSummary {
    pub amount_in: String,
    pub amount_out: String,
    pub gas: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRouteRequest {
    pub route_summary: RouteSummary,
    pub slippage: u32,
    pub sender: H160,
    pub recipient: H160,
    pub deadline: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRouteResponse {
    pub encoded_data: String,
    pub router_address: H160,
    pub value: U256,
}

#[async_trait::async_trait]
pub trait HyperLiquidApi: Send + Sync {
    async fn get_routes(&self, request: &RouteRequest) -> Result<RouteSummary>;
    async fn build_route(&self, request: &BuildRouteRequest) -> Result<BuildRouteResponse>;
}

pub struct DefaultHyperLiquidApi {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

impl DefaultHyperLiquidApi {
    pub fn new(client_builder: reqwest::ClientBuilder) -> Result<Self> {
        Ok(Self {
            client: client_builder
                .timeout(std::time::Duration::from_millis(1000))
                .build()?,
            base_url: reqwest::Url::parse("https://api.hyperliquid.xyz/")?,
        })
    }
}

#[async_trait::async_trait]
impl HyperLiquidApi for DefaultHyperLiquidApi {
    async fn get_routes(&self, request: &RouteRequest) -> Result<RouteSummary> {
        // Implement actual API call here. For now returning error or mock.
        let url = self.base_url.join("routes")?;
        let res = self.client.post(url).json(request).send().await?;
        res.json().await.map_err(Into::into)
    }

    async fn build_route(&self, request: &BuildRouteRequest) -> Result<BuildRouteResponse> {
        let url = self.base_url.join("route/build")?;
        let res = self.client.post(url).json(request).send().await?;
        res.json().await.map_err(Into::into)
    }
}