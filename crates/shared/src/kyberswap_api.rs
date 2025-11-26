//! KyberSwap Aggregator HTTP API client implementation.
//!
//! This module provides integration with KyberSwap's Aggregator API for finding
//! optimal swap routes across multiple DEXes.
//!
//! API Documentation: https://docs.kyberswap.com/kyberswap-solutions/kyberswap-aggregator/aggregator-api-specification/evm-swaps

use {
    anyhow::{Context, Result},
    ethcontract::{H160, U256},
    reqwest::{Client, ClientBuilder, StatusCode, Url},
    serde::{Deserialize, Serialize},
    serde_with::{DisplayFromStr, serde_as},
    std::{str::FromStr, time::Duration},
    thiserror::Error,
    tracing::instrument,
};

/// Helper to format H160 addresses for URL query parameters
fn addr2str(addr: H160) -> String {
    format!("{addr:#x}")
}

/// Configuration for KyberSwap API client
#[derive(Clone, Debug)]
pub struct KyberSwapConfig {
    /// Base URL for KyberSwap Aggregator API
    pub api_url: String,
    /// Chain name (e.g., "hyperevm" for chain hyper EVM)
    pub chain_name: String,
    /// HTTP request timeout
    pub http_timeout: Duration,
    /// Client ID for KyberSwap API
    pub client_id: String,
}

/// Request parameters for getting optimal swap routes
#[serde_as]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRequest {
    /// Source token address
    pub token_in: H160,
    /// Destination token address
    pub token_out: H160,
    /// Input amount as string (in token's smallest unit)
    #[serde_as(as = "DisplayFromStr")]
    pub amount_in: U256,
    /// Whether to include gas costs in the calculation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_include: Option<bool>,
    /// Gas price in wei as string
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub gas_price: Option<U256>,
}

/// Pool extra information
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PoolExtra {
    #[serde(rename = "swapFee", skip_serializing_if = "Option::is_none")]
    pub swap_fee: Option<u64>,
    #[serde(rename = "priceLimit", skip_serializing_if = "Option::is_none")]
    pub price_limit: Option<serde_json::Value>,
    #[serde(rename = "bufferIn", skip_serializing_if = "Option::is_none")]
    pub buffer_in: Option<String>,
    #[serde(rename = "bufferOut", skip_serializing_if = "Option::is_none")]
    pub buffer_out: Option<String>,
    #[serde(rename = "blockNumber", skip_serializing_if = "Option::is_none")]
    pub block_number: Option<u64>,
}

/// Extra information for route step
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct RouteStepExtra {
    #[serde(
        rename = "NextStateTickCurrent",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_state_tick_current: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _cs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _ts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ri: Option<String>,
    #[serde(rename = "nSqrtRx96", skip_serializing_if = "Option::is_none")]
    pub n_sqrt_rx96: Option<String>,
}

/// A single step in a swap route (can be multi-hop)
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct RouteStep {
    pub pool: String,
    #[serde(rename = "tokenIn")]
    pub token_in: String,
    #[serde(rename = "tokenOut")]
    pub token_out: String,
    #[serde(rename = "swapAmount")]
    pub swap_amount: String,
    #[serde(rename = "amountOut")]
    pub amount_out: String,
    #[serde(rename = "exchange")]
    pub exchange: String,
    #[serde(rename = "poolType")]
    pub pool_type: String,
    #[serde(rename = "poolExtra")]
    pub pool_extra: Option<PoolExtra>,
    pub extra: Option<RouteStepExtra>,
}

/// Extra fee information
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ExtraFee {
    #[serde(rename = "feeAmount")]
    pub fee_amount: String,
    #[serde(rename = "chargeFeeBy")]
    pub charge_fee_by: String,
    #[serde(rename = "isInBps")]
    pub is_in_bps: bool,
    #[serde(rename = "feeReceiver")]
    pub fee_receiver: String,
}

/// Summary of the optimal route found by KyberSwap
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct RouteSummary {
    #[serde(rename = "tokenIn")]
    pub token_in: String,
    #[serde(rename = "amountIn")]
    pub amount_in: String,
    #[serde(rename = "amountInUsd")]
    pub amount_in_usd: String,
    #[serde(rename = "tokenOut")]
    pub token_out: String,
    #[serde(rename = "amountOut")]
    pub amount_out: String,
    #[serde(rename = "amountOutUsd")]
    pub amount_out_usd: String,
    #[serde(rename = "gas")]
    pub gas: String,
    #[serde(rename = "gasPrice")]
    pub gas_price: String,
    #[serde(rename = "gasUsd")]
    pub gas_usd: String,
    #[serde(rename = "l1FeeUsd")]
    pub l1_fee_usd: String,
    #[serde(rename = "extraFee")]
    pub extra_fee: ExtraFee,
    #[serde(rename = "route")]
    pub route: Vec<Vec<RouteStep>>,
    #[serde(rename = "routeID")]
    pub route_id: String,
    pub checksum: String,
    pub timestamp: u64,
}

/// Response from GET /routes endpoint
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteResponse {
    /// Response code (0 = success)
    pub code: i32,
    /// Response message
    pub message: String,
    /// Response data
    pub data: RouteSummaryData,
    /// Request ID
    #[serde(default)]
    pub request_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSummaryData {
    /// The optimal route summary
    pub route_summary: RouteSummary,
    /// Router address (MetaAggregator)
    pub router_address: String,
}

/// Request to build executable transaction data from a route
#[serde_as]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRouteRequest {
    /// The route summary from GET /routes
    pub route_summary: RouteSummary,
    /// Slippage tolerance in basis points (e.g., 50 = 0.5%)
    pub slippage: u32,
    /// Address that will execute the swap (settlement contract)
    pub sender: H160,
    /// Address that will receive the output tokens (settlement contract)
    pub recipient: H160,
    /// Deadline timestamp (Unix seconds)
    #[serde_as(as = "DisplayFromStr")]
    pub deadline: u64,
}

/// Response from POST /route/build endpoint
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRouteResponse {
    /// Encoded calldata for MetaAggregator contract
    #[serde(rename = "data")]
    pub encoded_data: String,
    /// Router address to call (MetaAggregator)
    #[serde(rename = "routerAddress")]
    pub router_address: H160,
    /// ETH value to send with transaction (usually "0x0")
    #[serde_as(as = "DisplayFromStr")]
    pub value: U256,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct RouteBuildResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub message: String,
    pub data: RouteBuildResponseData,
    #[serde(rename = "requestId", default)]
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct RouteBuildResponseData {
    #[serde(rename = "amountIn")]
    pub amount_in: String,
    #[serde(rename = "amountInUsd")]
    pub amount_in_usd: String,
    #[serde(rename = "amountOut")]
    pub amount_out: String,
    #[serde(rename = "amountOutUsd")]
    pub amount_out_usd: String,
    pub gas: String,
    #[serde(rename = "gasUsd")]
    pub gas_usd: String,
    #[serde(rename = "additionalCostUsd")]
    pub additional_cost_usd: String,
    #[serde(rename = "additionalCostMessage")]
    pub additional_cost_message: String,
    #[serde(rename = "outputChange")]
    pub output_change: OutputChange,
    pub data: String,
    #[serde(rename = "routerAddress")]
    pub router_address: String,
    #[serde(rename = "transactionValue")]
    pub transaction_value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OutputChange {
    amount: String,
    percent: f64,
    level: i32,
}

/// Abstract KyberSwap API trait for testing/mocking
#[async_trait::async_trait]
#[mockall::automock]
pub trait KyberSwapApi: Send + Sync {
    /// Fetches the optimal swap route for given token pair and amount
    async fn get_routes(&self, request: &RouteRequest) -> Result<RouteSummary, KyberSwapApiError>;

    /// Builds executable transaction data from a route
    async fn build_route(
        &self,
        request: &BuildRouteRequest,
    ) -> Result<BuildRouteResponse, KyberSwapApiError>;
}

/// Default implementation of KyberSwap API client
#[derive(Debug)]
pub struct DefaultKyberSwapApi {
    client: Client,
    base_url: Url,
    chain_name: String,
    client_id: String,
}

impl DefaultKyberSwapApi {
    /// Creates a new KyberSwap API client
    pub fn new(client_builder: ClientBuilder, config: KyberSwapConfig) -> Result<Self> {
        Ok(Self {
            client: client_builder.build()?,
            base_url: config
                .api_url
                .parse()
                .context("invalid KyberSwap API URL")?,
            chain_name: config.chain_name,
            client_id: config.client_id,
        })
    }

    /// Helper to construct API endpoint URLs
    fn endpoint_url(&self, path: &str) -> Url {
        let full_path = format!("{}/api/v1{}", self.chain_name, path);
        crate::url::join(&self.base_url, &full_path)
    }

    /// Generic HTTP request helper with error handling
    async fn request<T: for<'a> serde::Deserialize<'a>>(
        &self,
        url: Url,
    ) -> Result<T, KyberSwapApiError> {
        let request_builder = self
            .client
            .get(url.clone())
            .header("User-Agent", "rust-test")
            .header("Accept", "*/*")
            .header("X-Client-Id", self.client_id.as_str());

        let response = request_builder
            .send()
            .await
            .map_err(KyberSwapApiError::RequestFailed)?;

        // println!("response: {:#?}", response);

        let status = response.status();
        let response_text = response
            .text()
            .await
            .map_err(KyberSwapApiError::RequestFailed)?;

        // println!("response_text: {:#?}", response_text);

        tracing::trace!("Response from KyberSwap API: {}", response_text);

        match status {
            StatusCode::TOO_MANY_REQUESTS => Err(KyberSwapApiError::RateLimited),
            StatusCode::NOT_FOUND => Err(KyberSwapApiError::NoRouteFound),
            status if status.is_success() => serde_json::from_str::<T>(&response_text)
                .map_err(|err| KyberSwapApiError::DeserializationError(err.to_string())),
            _ => Err(KyberSwapApiError::InvalidResponse(format!(
                "HTTP {}: {}",
                status, response_text
            ))),
        }
    }

    /// POST request helper for build endpoint
    async fn post_request<T: for<'a> serde::Deserialize<'a>, B: Serialize>(
        &self,
        url: Url,
        body: &B,
    ) -> Result<T, KyberSwapApiError> {
        tracing::debug!("POST request to KyberSwap API: {}", url);

        let response = self
            .client
            .post(url.clone())
            .header("User-Agent", "rust-test")
            .header("Content-Type", "application/json")
            .header("X-Client-Id", self.client_id.as_str())
            .json(body)
            .send()
            .await
            .map_err(KyberSwapApiError::RequestFailed)?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .map_err(KyberSwapApiError::RequestFailed)?;

        tracing::trace!("Response from KyberSwap API: {}", response_text);

        match status {
            StatusCode::TOO_MANY_REQUESTS => Err(KyberSwapApiError::RateLimited),
            status if status.is_success() => serde_json::from_str::<T>(&response_text)
                .map_err(|err| KyberSwapApiError::DeserializationError(err.to_string())),
            _ => Err(KyberSwapApiError::InvalidResponse(format!(
                "HTTP {}: {}",
                status, response_text
            ))),
        }
    }
}

#[async_trait::async_trait]
impl KyberSwapApi for DefaultKyberSwapApi {
    #[instrument(skip(self), fields(token_in = %request.token_in, token_out = %request.token_out))]
    async fn get_routes(&self, request: &RouteRequest) -> Result<RouteSummary, KyberSwapApiError> {
        let mut url = self.endpoint_url("/routes");

        url.query_pairs_mut()
            .append_pair("tokenIn", &addr2str(request.token_in))
            .append_pair("tokenOut", &addr2str(request.token_out))
            .append_pair("amountIn", &request.amount_in.to_string());

        if let Some(gas_include) = request.gas_include {
            url.query_pairs_mut()
                .append_pair("gasInclude", &gas_include.to_string());
        }

        if let Some(gas_price) = request.gas_price {
            url.query_pairs_mut()
                .append_pair("gasPrice", &gas_price.to_string());
        }

        tracing::debug!("url: {}", url.to_string());

        let response: RouteResponse = self.request(url).await?;

        // Check if the API returned an error code
        if response.code != 0 {
            return Err(KyberSwapApiError::InvalidResponse(format!(
                "KyberSwap API error: code={}, message={}",
                response.code, response.message
            )));
        }

        Ok(response.data.route_summary)
    }

    #[instrument(skip(self, request), fields(token_in = %request.route_summary.token_in, token_out = %request.route_summary.token_out))]
    async fn build_route(
        &self,
        request: &BuildRouteRequest,
    ) -> Result<BuildRouteResponse, KyberSwapApiError> {
        let url = self.endpoint_url("/route/build");

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct BuildRequest {
            route_summary: RouteSummary,
            sender: String,
            recipient: String,
            deadline: u64,
            slippage_tolerance: u64,
        }

        let body = BuildRequest {
            route_summary: request.route_summary.clone(),
            slippage_tolerance: request.slippage as u64,
            sender: addr2str(request.sender),
            recipient: addr2str(request.recipient),
            deadline: request.deadline,
        };

        let response: RouteBuildResponse = self.post_request(url, &body).await?;
        if response.code != 0 {
            return Err(KyberSwapApiError::InvalidResponse(format!(
                "KyberSwap API error: code={}, message={}",
                response.code, response.message
            )));
        }

        Ok(BuildRouteResponse {
            encoded_data: response.data.data,
            router_address: H160::from_str(&response.data.router_address).unwrap_or_default(),
            value: U256::from_str(&response.data.transaction_value).unwrap_or_default(),
        })
    }
}

/// Errors that can occur when interacting with KyberSwap API
#[derive(Debug, Error)]
pub enum KyberSwapApiError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("No route found for token pair")]
    NoRouteFound,

    #[error("Rate limited by KyberSwap API")]
    RateLimited,

    #[error("API response deserialization error: {0}")]
    DeserializationError(String),

    #[error("Invalid API response: {0}")]
    InvalidResponse(String),
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_addr2str_formatting() {
        let addr = H160::from_low_u64_be(0x42);
        let formatted = addr2str(addr);
        assert_eq!(formatted, "0x0000000000000000000000000000000000000042");
    }

    #[test]
    fn test_route_request_serialization() {
        let request = RouteRequest {
            token_in: H160::from_low_u64_be(1),
            token_out: H160::from_low_u64_be(2),
            amount_in: U256::from(1000000),
            gas_include: Some(true),
            gas_price: Some(U256::from(50)),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("tokenIn"));
        assert!(json.contains("tokenOut"));
        assert!(json.contains("amountIn"));
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_routes() {
        let request = RouteRequest {
            token_in: H160::from_str("0xB8CE59FC3717ada4C02eaDF9682A9e934F625ebb").unwrap(),
            token_out: H160::from_str("0xb88339CB7199b77E23DB6E890353E22632Ba630f").unwrap(),
            amount_in: U256::from(1000000),
            gas_include: Some(true),
            gas_price: None,
        };
        let api = DefaultKyberSwapApi::new(
            ClientBuilder::new(),
            KyberSwapConfig {
                api_url: "https://aggregator-api.kyberswap.com".to_string(),
                chain_name: "hyperevm".to_string(),
                http_timeout: Duration::from_secs(10),
                client_id: "test".to_string(),
            },
        )
        .unwrap();
        let response = api.get_routes(&request).await.unwrap();
        println!("response: {:#?}", response);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_routes_parallel() {
        // Create API client
        let api = std::sync::Arc::new(
            DefaultKyberSwapApi::new(
                ClientBuilder::new(),
                KyberSwapConfig {
                    api_url: "https://aggregator-api.kyberswap.com".to_string(),
                    chain_name: "hyperevm".to_string(),
                    http_timeout: Duration::from_secs(30),
                    client_id: "test".to_string(),
                },
            )
            .unwrap(),
        );

        // Define 4 requests
        let request1 = RouteRequest {
            token_in: H160::from_str("0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb").unwrap(),
            token_out: H160::from_str("0xb88339cb7199b77e23db6e890353e22632ba630f").unwrap(),
            amount_in: U256::from(1000000),
            gas_include: Some(true),
            gas_price: None,
        };

        let request2 = RouteRequest {
            token_in: H160::from_str("0x5555555555555555555555555555555555555555").unwrap(),
            token_out: H160::from_str("0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb").unwrap(),
            amount_in: U256::from(100000000000000000u64),
            gas_include: Some(true),
            gas_price: None,
        };

        let request3 = RouteRequest {
            token_in: H160::from_str("0x5555555555555555555555555555555555555555").unwrap(),
            token_out: H160::from_str("0xb88339cb7199b77e23db6e890353e22632ba630f").unwrap(),
            amount_in: U256::from(100000000000000000u64),
            gas_include: Some(true),
            gas_price: None,
        };

        let request4 = RouteRequest {
            token_in: H160::from_str("0x5555555555555555555555555555555555555555").unwrap(),
            token_out: H160::from_str("0xb88339cb7199b77e23db6e890353e22632ba630f").unwrap(),
            amount_in: U256::from(100000000000000000u64),
            gas_include: Some(true),
            gas_price: None,
        };

        // Execute all 4 requests in parallel
        let (result1, result2, result3, result4) = tokio::join!(
            api.get_routes(&request1),
            api.get_routes(&request2),
            api.get_routes(&request3),
            api.get_routes(&request4),
        );

        // Check results
        println!("Request 1 result: {:#?}", result1);
        println!("Request 2 result: {:#?}", result2);
        println!("Request 3 result: {:#?}", result3);
        println!("Request 4 result: {:#?}", result4);

        // Assert all requests succeeded
        assert!(result1.is_ok(), "Request 1 failed: {:?}", result1);
        assert!(result2.is_ok(), "Request 2 failed: {:?}", result2);
        assert!(result3.is_ok(), "Request 3 failed: {:?}", result3);
        assert!(result4.is_ok(), "Request 4 failed: {:?}", result4);

        // Verify route data
        let route1 = result1.unwrap();
        let route2 = result2.unwrap();
        let route3 = result3.unwrap();
        let route4 = result4.unwrap();

        assert_eq!(
            route1.token_in,
            "0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb"
        );
        assert_eq!(
            route1.token_out,
            "0xb88339cb7199b77e23db6e890353e22632ba630f"
        );
        assert!(!route1.route.is_empty(), "Route 1 should have steps");

        assert_eq!(
            route2.token_in,
            "0x5555555555555555555555555555555555555555"
        );
        assert_eq!(
            route2.token_out,
            "0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb"
        );
        assert!(!route2.route.is_empty(), "Route 2 should have steps");

        assert_eq!(
            route3.token_in,
            "0x5555555555555555555555555555555555555555"
        );
        assert_eq!(
            route3.token_out,
            "0xb88339cb7199b77e23db6e890353e22632ba630f"
        );
        assert!(!route3.route.is_empty(), "Route 3 should have steps");

        assert_eq!(
            route4.token_in,
            "0x5555555555555555555555555555555555555555"
        );
        assert_eq!(
            route4.token_out,
            "0xb88339cb7199b77e23db6e890353e22632ba630f"
        );
        assert!(!route4.route.is_empty(), "Route 4 should have steps");

        println!("All 4 parallel requests completed successfully!");
    }
}
