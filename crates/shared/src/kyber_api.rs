//! Kyber Aggregator HTTP API client implementation.
//!
//! For more information on the HTTP API, consult:
//! <https://docs.kyberswap.com/kyberswap-solutions/kyberswap-aggregator/aggregator-api-specification/evm-swaps>

use {
    anyhow::{Context, Result},
    chrono::{DateTime, NaiveDateTime, Utc},
    derivative::Derivative,
    ethcontract::{H160, H256, U256},
    number::serialization::HexOrDecimalU256,
    reqwest::{Client, ClientBuilder, IntoUrl, Url},
    serde::{Deserialize, Serialize},
    serde_with::{DisplayFromStr, serde_as},
    std::sync::Arc,
    thiserror::Error,
    tracing::instrument,
};

#[derive(Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TradeType {
    #[serde(rename = "exactIn")]
    ExactIn,
    #[serde(rename = "exactOut")]
    ExactOut,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct RoutingApiQuery {
    token_in: String,
    token_out: String,
    token_in_chain_id: u64,
    token_out_chain_id: u64,
    #[serde(rename = "type")]
    trade_type: TradeType,
    #[serde(rename = "amountIn")]
    amount_in: String,
    recipient: String,
    slippage_tolerance: String,
    deadline: u64,
    #[serde(rename = "enableUniversalRouter")]
    enable_universal_router: bool,
    protocols: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct KyberPoolExtra {
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

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct KyberExtra {
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
    #[serde(rename = "nT", skip_serializing_if = "Option::is_none")]
    pub n_t: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct KyberRouteStep {
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
    pub pool_extra: KyberPoolExtra,
    pub extra: KyberExtra,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct KyberExtraFee {
    #[serde(rename = "feeAmount")]
    pub fee_amount: String,
    #[serde(rename = "chargeFeeBy")]
    pub charge_fee_by: String,
    #[serde(rename = "isInBps")]
    pub is_in_bps: bool,
    #[serde(rename = "feeReceiver")]
    pub fee_receiver: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct KyberRouteSummary {
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
    pub extra_fee: KyberExtraFee,
    #[serde(rename = "route")]
    pub route: Vec<Vec<KyberRouteStep>>,
    #[serde(rename = "routeID")]
    pub route_id: String,
    pub checksum: String,
    pub timestamp: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct OutputChange {
    pub amount: String,
    pub percent: f64,
    pub level: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct KyberRouteBuildResponseData {
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
pub struct KyberRouteBuildResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub message: String,
    pub data: KyberRouteBuildResponseData,
    #[serde(rename = "requestId", default)]
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct KyberRouteResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub message: String,
    pub data: KyberRouteData,
    #[serde(rename = "requestId", default)]
    pub request_id: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct KyberRouteData {
    #[serde(rename = "routeSummary")]
    pub route_summary: KyberRouteSummary,
    #[serde(rename = "routerAddress")]
    pub router_address: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KyberRoutingApiQuery {
    pub token_in: String,
    pub token_out: String,
    #[serde(rename = "amountIn")]
    pub amount_in: String,
}

impl KyberRoutingApiQuery {
    fn format_url(&self, base_url: &Url) -> Url {
        let mut url = crate::url::join(base_url, "/routes");
        url.query_pairs_mut().append_pair("tokenIn", &self.token_in);
        url.query_pairs_mut()
            .append_pair("tokenOut", &self.token_out);
        url.query_pairs_mut()
            .append_pair("amountIn", &self.amount_in);
        url
    }
}

impl Default for KyberRoutingApiQuery {
    fn default() -> Self {
        Self {
            token_in: String::from("0x9b498C3c8A0b8CD8BA1D9851d40D186F1872b44E"), // PURR
            token_out: String::from("0xb50A96253aBDF803D85efcDce07Ad8becBc52BD5"), // USDHL
            amount_in: String::from("1000000000000000000"),
        }
    }
}

#[serde_as]
#[derive(Debug, Derivative, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[derivative(Default)]
#[serde(rename_all = "camelCase")]
pub struct OrderMetadata {
    #[derivative(Default(value = "DateTime::<Utc>::MIN_UTC"))]
    pub created_at: DateTime<Utc>,
    #[serde(with = "bytes_hex")]
    pub order_hash: Vec<u8>,
    #[serde_as(as = "DisplayFromStr")]
    pub remaining_fillable_taker_amount: u128,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ZeroExSignature {
    pub r: H256,
    pub s: H256,
    pub v: u8,
    pub signature_type: u8,
}

#[serde_as]
#[derive(Debug, Derivative, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[derivative(Default)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    /// The ID of the Ethereum chain where the `verifying_contract` is located.
    pub chain_id: u64,
    /// Timestamp in seconds of when the order expires. Expired orders cannot be
    /// filled.
    #[derivative(Default(value = "NaiveDateTime::MAX.and_utc().timestamp() as u64"))]
    #[serde_as(as = "DisplayFromStr")]
    pub expiry: u64,
    /// The address of the entity that will receive any fees stipulated by the
    /// order. This is typically used to incentivize off-chain order relay.
    pub fee_recipient: H160,
    /// The address of the party that creates the order. The maker is also one
    /// of the two parties that will be involved in the trade if the order
    /// gets filled.
    pub maker: H160,
    /// The amount of `maker_token` being sold by the maker.
    #[serde_as(as = "DisplayFromStr")]
    pub maker_amount: u128,
    /// The address of the ERC20 token the maker is selling to the taker.
    pub maker_token: H160,
    /// The staking pool to attribute the 0x protocol fee from this order. Set
    /// to zero to attribute to the default pool, not owned by anyone.
    pub pool: H256,
    /// A value that can be used to guarantee order uniqueness. Typically it is
    /// set to a random number.
    #[serde_as(as = "HexOrDecimalU256")]
    pub salt: U256,
    /// It allows the maker to enforce that the order flow through some
    /// additional logic before it can be filled (e.g., a KYC whitelist).
    pub sender: H160,
    /// The signature of the signed order.
    pub signature: ZeroExSignature,
    /// The address of the party that is allowed to fill the order. If set to a
    /// specific party, the order cannot be filled by anyone else. If left
    /// unspecified, anyone can fill the order.
    pub taker: H160,
    /// The amount of `taker_token` being sold by the taker.
    #[serde_as(as = "DisplayFromStr")]
    pub taker_amount: u128,
    /// The address of the ERC20 token the taker is selling to the maker.
    pub taker_token: H160,
    /// Amount of takerToken paid by the taker to the feeRecipient.
    #[serde_as(as = "DisplayFromStr")]
    pub taker_token_fee_amount: u128,
    /// Address of the contract where the transaction should be sent, usually
    /// this is the 0x exchange proxy contract.
    pub verifying_contract: H160,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, Eq, PartialEq)]
pub struct OrderRecord(Arc<Inner>);

#[derive(Debug, Default, Clone, Deserialize, Serialize, Eq, PartialEq)]
struct Inner {
    pub order: Order,
    #[serde(rename = "metaData")]
    pub metadata: OrderMetadata,
}

impl OrderRecord {
    pub fn new(order: Order, metadata: OrderMetadata) -> Self {
        OrderRecord(Arc::new(Inner { metadata, order }))
    }

    pub fn metadata(&self) -> &OrderMetadata {
        &self.0.metadata
    }

    pub fn order(&self) -> &Order {
        &self.0.order
    }

    /// Scales the `maker_amount` according to how much of the partially
    /// fillable amount was already used.
    pub fn remaining_maker_amount(&self) -> Result<u128> {
        if self.metadata().remaining_fillable_taker_amount > self.order().taker_amount {
            anyhow::bail!("remaining taker amount bigger than total taker amount");
        }

        // all numbers are at most u128::MAX so none of these operations can overflow
        let scaled_maker_amount = U256::from(self.order().maker_amount)
            * U256::from(self.metadata().remaining_fillable_taker_amount)
            / U256::from(self.order().taker_amount);

        // `scaled_maker_amount` is at most as big as `maker_amount` which already fits
        // in an u128
        Ok(scaled_maker_amount.as_u128())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodParameters {
    pub calldata: String,
    pub value: String,
    pub to: String,
}

/// Abstract Kyber API. Provides a mockable implementation.
#[async_trait::async_trait]
#[mockall::automock]
pub trait KyberApi: Send + Sync {
    /// Retrieves routing information for a swap.
    async fn get_quote(
        &self,
        query: &KyberRoutingApiQuery,
    ) -> Result<KyberRouteResponse, KyberResponseError>;

    /// Build method for a swap.
    async fn build_swap(
        &self,
        route_summary: KyberRouteSummary,
        sender: String,
        recipient: String,
        deadline: u64,
        slippage_tolerance: u64,
    ) -> Result<MethodParameters, KyberResponseError>;
}

/// 0x API Client implementation.
#[derive(Debug)]
pub struct DefaultKyberApi {
    client: Client,
    base_url: Url,
}

impl DefaultKyberApi {
    /// Default Kyber API URL.
    pub const DEFAULT_URL: &'static str = "https://aggregator-api.kyberswap.com/hyperevm/api/v1";

    /// Create a new Kyber Aggregator HTTP API client with the specified base URL.
    pub fn new(client_builder: ClientBuilder, base_url: impl IntoUrl) -> Result<Self> {
        Ok(Self {
            client: client_builder.build().unwrap(),
            base_url: base_url.into_url().context("kyber aggregator api url")?,
        })
    }

    /// Create a Kyber HTTP API client for testing using the default HTTP client.
    ///
    /// This method will attempt to read the `KYBER_URL` (falling back to the
    /// default URL) from the local environment when creating the API client.
    pub fn test() -> Self {
        Self::new(
            Client::builder(),
            std::env::var("KYBER_URL").unwrap_or_else(|_| Self::DEFAULT_URL.to_string()),
        )
        .unwrap()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawResponse<Ok> {
    ResponseOk(Ok),
    ResponseErr { reason: String, code: u32 },
}

#[derive(Error, Debug)]
pub enum KyberResponseError {
    #[error("ServerError from query {0}")]
    ServerError(String),

    #[error("uncatalogued error message: {0}")]
    UnknownZeroExError(String),

    #[error("insufficient liquidity")]
    InsufficientLiquidity,

    #[error("Error({0}) for response {1}")]
    SerializeError(serde_qs::Error, String),

    #[error("Error({0}) for response {1}")]
    DeserializeError(serde_json::Error, String),

    // Recovered Response but failed on async call of response.text()
    #[error(transparent)]
    TextFetch(reqwest::Error),

    // Connectivity or non-response error
    #[error("Failed on send")]
    Send(reqwest::Error),

    #[error("Rate limited")]
    RateLimited,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct KyberBuildRequest {
    route_summary: KyberRouteSummary,
    sender: String,
    recipient: String,
    deadline: u64,
    slippage_tolerance: u64,
}

#[async_trait::async_trait]
impl KyberApi for DefaultKyberApi {
    #[instrument(skip_all)]
    async fn get_quote(
        &self,
        params: &KyberRoutingApiQuery,
    ) -> Result<KyberRouteResponse, KyberResponseError> {
        let query = KyberRoutingApiQuery {
            token_in: resolve_address(&params.token_in),
            token_out: resolve_address(&params.token_out),
            amount_in: params.amount_in.clone(),
        };

        let query_string = serde_qs::to_string(&query).map_err(|e| {
            KyberResponseError::SerializeError(e, format!("kyber routing api query: {:?}", query))
        })?;

        let full_query = format!("{}{}?{}", self.base_url, "/routes", query_string);
        tracing::debug!("kyber routing api query: {}", full_query);
        let client = reqwest::Client::new();

        let response = client
            .get(full_query)
            .header("User-Agent", "rust-test")
            .send()
            .await
            .map_err(|e| KyberResponseError::Send(e))?;
        let response_text = response
            .text()
            .await
            .map_err(KyberResponseError::TextFetch)?;
        let response: KyberRouteResponse = serde_json::from_str(&response_text)
            .map_err(|e| KyberResponseError::DeserializeError(e, response_text.clone()))?;
        tracing::debug!("kyber routing api response: {:?}", response);
        Ok(response)
    }

    async fn build_swap(
        &self,
        route_summary: KyberRouteSummary,
        sender: String,
        recipient: String,
        deadline: u64,
        slippage_tolerance: u64,
    ) -> Result<MethodParameters, KyberResponseError> {
        let url = format!("{}{}", self.base_url, "/route/build");
        let client = reqwest::Client::new();
        // Create a copy of route_summary and recursively remove any None fields
        let mut route_summary = route_summary.clone();
        route_summary.route = route_summary
            .route
            .into_iter()
            .map(|steps| {
                steps
                    .into_iter()
                    .map(|mut step| {
                        let mut new_pool_extra = KyberPoolExtra {
                            swap_fee: None,
                            price_limit: None,
                            buffer_in: None,
                            buffer_out: None,
                            block_number: None,
                        };
                        if step.pool_extra.swap_fee.is_some() {
                            new_pool_extra.swap_fee = step.pool_extra.swap_fee;
                        }
                        if step.pool_extra.price_limit.is_some() {
                            new_pool_extra.price_limit = step.pool_extra.price_limit;
                        }
                        if step.pool_extra.buffer_in.is_some() {
                            new_pool_extra.buffer_in = step.pool_extra.buffer_in;
                        }
                        if step.pool_extra.buffer_out.is_some() {
                            new_pool_extra.buffer_out = step.pool_extra.buffer_out;
                        }
                        if step.pool_extra.block_number.is_some() {
                            new_pool_extra.block_number = step.pool_extra.block_number;
                        }
                        step.pool_extra = new_pool_extra;
                        step
                    })
                    .collect()
            })
            .collect();

        tracing::debug!("route_summary: {:?}", route_summary);

        let body = KyberBuildRequest {
            route_summary: route_summary.clone(),
            sender: sender.clone(),
            recipient,
            deadline,
            slippage_tolerance,
        };

        let response = client
            .post(url)
            .json(&body)
            .header("User-Agent", "rust-test")
            .send()
            .await
            .map_err(|e| KyberResponseError::Send(e))?;

        tracing::debug!(
            "build_method_parameters response status: {:?}",
            response.status()
        );

        let response_text = response
            .text()
            .await
            .map_err(KyberResponseError::TextFetch)?;
        let kyber_response = serde_json::from_str::<KyberRouteBuildResponse>(&response_text)
            .map_err(|e| KyberResponseError::DeserializeError(e, response_text))?;
        tracing::debug!("kyber_response: {:?}", kyber_response);
        let method_parameters: MethodParameters = MethodParameters {
            calldata: kyber_response.data.data,
            value: kyber_response.data.transaction_value,
            to: kyber_response.data.router_address,
        };
        Ok(method_parameters)
    }
}

// The Uniswap routing API requires that "ETH" be used instead of the zero address
fn resolve_address(token: &str) -> String {
    if token == "0x0000000000000000000000000000000000000000" {
        return "ETH".to_string();
    }
    token.to_string()
}

/// Check if the response indicates success.
fn is_successful_response(response: &KyberRouteResponse) -> bool {
    response.code == 0 && (response.message == "successfully" || response.message == "OK")
}

#[derive(prometheus_metric_storage::MetricStorage)]
struct Metrics {
    /// Counter for Kyber API requests by URI path and result.
    #[metric(labels("path", "result"))]
    kyber_api_requests: prometheus::IntCounterVec,
}

impl Metrics {
    fn get() -> &'static Self {
        Metrics::instance(observe::metrics::get_storage_registry()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_get_quote() {
        let api = DefaultKyberApi::test();
        let result = api.get_quote(&KyberRoutingApiQuery::default()).await;
        dbg!(&result);
        assert!(result.is_ok());
    }

    #[test]
    fn deserialize_route_response() {
        let response_json = r#"{
            "code": 0,
            "message": "successfully",
            "data": {
                "routeSummary": {
                    "tokenIn": "0x9b498c3c8a0b8cd8ba1d9851d40d186f1872b44e",
                    "amountIn": "100000000000000000",
                    "amountInUsd": "0.01089082118538589",
                    "tokenOut": "0xb50a96253abdf803d85efcdce07ad8becbc52bd5",
                    "amountOut": "11238",
                    "amountOutUsd": "0.01124049516363144",
                    "gas": "467166",
                    "gasPrice": "100000000",
                    "gasUsd": "0.001481587105465152",
                    "l1FeeUsd": "0",
                    "extraFee": {
                        "feeAmount": "",
                        "chargeFeeBy": "",
                        "isInBps": false,
                        "feeReceiver": ""
                    },
                    "route": [
                        [
                            {
                                "pool": "0x3bd124f04bf51f2e63de2bd15af8b85bf081df83",
                                "tokenIn": "0x9b498c3c8a0b8cd8ba1d9851d40d186f1872b44e",
                                "tokenOut": "0xca79db4b49f608ef54a5cb813fbed3a6387bc645",
                                "swapAmount": "100000000000000000",
                                "amountOut": "11358218958396596",
                                "exchange": "hyperswap-v3",
                                "poolType": "uniswapv3",
                                "poolExtra": {
                                    "swapFee": 3000,
                                    "priceLimit": "79252090486426485397"
                                },
                                "extra": {
                                    "_cs": "11230250078353113006",
                                    "_ts": "1762418141",
                                    "nSqrtRx96": "26595685076849928296381740286",
                                    "nT": -21833,
                                    "ri": "68114c04-aa9a-448a-a402-c95ed9d0507e"
                                }
                            }
                        ]
                    ],
                    "routeID": "68114c04-aa9a-448a-a402-c95ed9d0507e",
                    "checksum": "11230250078353113006",
                    "timestamp": 1762418141
                },
                "routerAddress": "0x6131B5fae19EA4f9D964eAc0408E4408b66337b5"
            },
            "requestId": "68114c04-aa9a-448a-a402-c95ed9d0507e"
        }"#;

        let response = serde_json::from_str::<KyberRouteResponse>(response_json).unwrap();

        assert_eq!(response.code, 0);
        assert_eq!(response.message, "successfully");
        assert_eq!(response.request_id, "68114c04-aa9a-448a-a402-c95ed9d0507e");
        assert_eq!(
            response.data.router_address,
            "0x6131B5fae19EA4f9D964eAc0408E4408b66337b5"
        );
        assert_eq!(
            response.data.route_summary.token_in,
            "0x9b498c3c8a0b8cd8ba1d9851d40d186f1872b44e"
        );
        assert_eq!(response.data.route_summary.amount_out, "11238");
        assert_eq!(response.data.route_summary.route.len(), 1);
        assert_eq!(response.data.route_summary.route[0].len(), 1);
    }

    #[test]
    fn compute_remaining_maker_amount() {
        let bogous_order = OrderRecord::new(
            Order {
                taker_amount: u128::MAX - 1,
                maker_amount: u128::MAX,
                ..Default::default()
            },
            OrderMetadata {
                // remaining amount bigger than total amount
                remaining_fillable_taker_amount: u128::MAX,
                ..Default::default()
            },
        );
        assert!(bogous_order.remaining_maker_amount().is_err());

        let biggest_unfilled_order = OrderRecord::new(
            Order {
                taker_amount: u128::MAX,
                maker_amount: u128::MAX,
                ..Default::default()
            },
            OrderMetadata {
                remaining_fillable_taker_amount: u128::MAX,
                ..Default::default()
            },
        );
        assert_eq!(
            u128::MAX,
            // none of the operations overflow with u128::MAX for all values
            biggest_unfilled_order.remaining_maker_amount().unwrap()
        );

        let biggest_partially_filled_order = OrderRecord::new(
            Order {
                taker_amount: u128::MAX,
                maker_amount: u128::MAX,
                ..Default::default()
            },
            OrderMetadata {
                remaining_fillable_taker_amount: u128::MAX / 2,
                ..Default::default()
            },
        );
        assert_eq!(
            u128::MAX / 2,
            biggest_partially_filled_order
                .remaining_maker_amount()
                .unwrap()
        );
    }

    #[test]
    fn deserialize_build_response() {
        let response_json = r#"{
            "code": 0,
            "message": "successfully",
            "data": {
                "amountIn": "100000000000000000",
                "amountInUsd": "0.01080271703652871",
                "amountOut": "11237",
                "amountOutUsd": "0.011217435827217851",
                "gas": "467166",
                "gasUsd": "0.0014911730293156766",
                "additionalCostUsd": "0",
                "additionalCostMessage": "",
                "outputChange": {
                    "amount": "0",
                    "percent": 0,
                    "level": 0
                },
                "data": "0xe21fd0e9000000000000000000000000000000000000000000000000000000000000002000000000000000000000000063242a4ea82847b20e506b63b0e2e2eff0cc6cb0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000760",
                "routerAddress": "0x6131B5fae19EA4f9D964eAc0408E4408b66337b5",
                "transactionValue": "0"
            },
            "requestId": "0b60244b-276c-46ea-8631-0468f7017695"
        }"#;

        let response = serde_json::from_str::<KyberRouteBuildResponse>(response_json).unwrap();
        assert_eq!(response.code, 0);
        assert_eq!(response.message, "successfully");
        assert_eq!(response.data.amount_in, "100000000000000000");
        assert_eq!(response.data.amount_in_usd, "0.01080271703652871");
        assert_eq!(response.data.amount_out, "11237");
        assert_eq!(response.data.amount_out_usd, "0.011217435827217851");
        assert_eq!(response.data.gas, "467166");
        assert_eq!(response.data.gas_usd, "0.0014911730293156766");
        assert_eq!(response.data.additional_cost_usd, "0");
        assert_eq!(response.data.additional_cost_message, "");
        assert_eq!(response.data.output_change.amount, "0");
        assert_eq!(response.data.output_change.percent, 0.0);
        assert_eq!(response.data.output_change.level, 0);
        assert!(response.data.data.starts_with("0xe21fd0e9"));
        assert_eq!(
            response.data.router_address,
            "0x6131B5fae19EA4f9D964eAc0408E4408b66337b5"
        );
        assert_eq!(response.data.transaction_value, "0");
        assert_eq!(
            response.request_id,
            Some("0b60244b-276c-46ea-8631-0468f7017695".to_string())
        );
    }
}
