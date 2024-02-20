use std::io::Write;
use std::net::SocketAddr;

use std::sync::Arc;

use axum::{routing::get, Router};

use log::info;

use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;

use photo_backlog_exporter::*;

// Enables logging with support for systemd (if enabled).
// Adopted from https://github.com/rust-cli/env_logger/issues/157.
fn enable_logging() {
    match std::env::var("RUST_LOG_SYSTEMD") {
        Ok(s) if s == "yes" => env_logger::builder()
            .format(|buf, record| {
                writeln!(
                    buf,
                    "<{}>{}: {}",
                    match record.level() {
                        log::Level::Error => 3,
                        log::Level::Warn => 4,
                        log::Level::Info => 6,
                        log::Level::Debug => 7,
                        log::Level::Trace => 7,
                    },
                    record.target(),
                    record.args()
                )
            })
            .init(),
        _ => env_logger::init(),
    };
}

#[tokio::main]
async fn main() -> Result<(), String> {
    enable_logging();

    let opts = cli::parse_args()?;

    info!("Starting up with the following options: {:?}", opts);
    let addr = SocketAddr::from((opts.listen, opts.port));
    let collector = Box::new(cli::collector_from_args(opts));
    let mut registry = Registry::default();
    registry.register_collector(collector);
    let r2 = Arc::new(registry);

    // build our application with a route
    let app = Router::new().route(
        "/metrics",
        get({
            let req_registry = Arc::clone(&r2);
            move || metrics(req_registry)
        }),
    );
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .map_err(|s| s.to_string())
}

// metrics handler
async fn metrics(registry: Arc<Registry>) -> String {
    let mut buffer = String::new();
    encode(&mut buffer, &registry).unwrap();
    buffer
}
