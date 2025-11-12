use crate::{app, serve};
use clap::Parser;
use std::net::SocketAddr;
use tokio::sync::oneshot;
#[derive(Parser, Debug)]
#[clap(name = "driver-hyperliquid-template")]
pub struct Args {
    #[clap(long, env = "PORT", default_value = "8080")]
    pub port: u16,
}
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
    let app = app();
    if let Some(sender) = addr_sender {
        let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
        sender.send(addr).unwrap();
    }
    serve(app, args.port).await;
}
