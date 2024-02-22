use std::net::SocketAddr;
use std::sync::Arc;

use axum::{routing::get, Router};
use prometheus_client::{encoding::text::encode, registry::Registry};
use tokio::net::TcpListener;

use crate::cli;

pub fn build_app(opts: cli::CliOptions) -> (SocketAddr, Router) {
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
    (addr, app)
}

pub async fn run_daemon(addr: SocketAddr, app: Router) -> Result<(), String> {
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {}", e))
}

// metrics handler
async fn metrics(registry: Arc<Registry>) -> String {
    let mut buffer = String::new();
    encode(&mut buffer, &registry).unwrap();
    buffer
}
