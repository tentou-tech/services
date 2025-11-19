#[tokio::main]
async fn main() {
    driver_hyperliquid_template::start(std::env::args()).await;
}
