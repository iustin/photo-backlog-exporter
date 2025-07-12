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
        .map_err(|e| format!("Failed to bind to {addr}: {e}"))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {e}"))
}

// metrics handler
async fn metrics(registry: Arc<Registry>) -> String {
    let mut buffer = String::new();
    encode(&mut buffer, &registry).unwrap();
    buffer
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use ::axum_test::TestServer;
    use speculoos::prelude::*;

    use tempfile::tempdir;
    use tokio::net::TcpListener;

    use crate::{cli, daemon::run_daemon};

    #[tokio::test]
    async fn test_metrics() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_str = temp_dir.path().to_str().expect("convert tempdir to str");
        std::fs::File::create(temp_dir.path().join("test1.nef")).unwrap();
        std::fs::File::create(temp_dir.path().join("test2.nef")).unwrap();

        let opts = cli::parse_args_from(&["--path", temp_dir_str]).expect("parse_args");
        let (_addr, app) = super::build_app(opts);
        let server = TestServer::new(app).unwrap();
        let response = server.get("/metrics").await;
        response.assert_status_ok();
        let raw_text = response.text();
        assert_that!(raw_text).contains("photo_backlog_counts{kind=\"folders\"} 1");
        assert_that!(raw_text).contains("photo_backlog_counts{kind=\"photos\"} 2");
        assert_that!(raw_text).contains("photo_backlog_ages_count 2");
        assert_that!(raw_text).contains("photo_backlog_processing_time_seconds ");
    }

    #[tokio::test]
    async fn test_bind_conflict() {
        // First, create and initialize app.
        let temp_dir = tempdir().unwrap();
        let temp_dir_str = temp_dir.path().to_str().expect("convert tempdir to str");
        let opts = cli::parse_args_from(&["--path", temp_dir_str]).expect("parse_args");
        let (_addr, app) = super::build_app(opts);

        // Bind to a random localhost port, and remember the full address,
        // including port.
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);
        let listener = TcpListener::bind(&socket).await;
        let ok_listener = assert_that!(listener).is_ok().subject;
        let local_addr = ok_listener.local_addr();
        let addr_with_port = assert_that!(local_addr).is_ok().subject;

        // Now try to run a demon against the same address/port combination,
        // which should fail.
        let result = run_daemon(*addr_with_port, app).await;
        assert_that!(result).is_err().contains("Failed to bind to");
    }
}
