use std::ffi::OsString;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::{routing::get, Router};
use gumdrop::Options;
use log::info;

use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;

use photo_backlog_exporter::*;

#[derive(Debug, Options)]
struct CliOptions {
    #[options(help = "print help message")]
    help: bool,

    #[options(help = "port to listen on", meta = "PORT", default = "8813")]
    port: u16,

    #[options(help = "address to listen on", default = "::")]
    listen: IpAddr,

    #[options(help = "path to root of incoming photo directory", required)]
    path: PathBuf,

    #[options(
        help = "ignored file extension",
        default = "xmp,lua,DS_Store",
        parse(from_str = "parse_exts"),
        no_multi
    )]
    ignored_exts: Vec<OsString>,

    #[options(
        help = "Photos age histogram buckets, in weeks",
        default = "1,2,3,4,5,7,10,13,17,20,26,30,35,52,104",
        parse(try_from_str = "parse_weeks"),
        // Sigh, I'm doing my own parsing!
        no_multi
    )]
    age_buckets: Vec<f64>,

    #[options(help = "Owner expected for all files")]
    owner: Option<u32>,

    #[options(help = "Group expected for all files")]
    group: Option<u32>,
}

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

    let opts = CliOptions::parse_args_default_or_exit();

    info!("Starting up with the following options: {:?}", opts);

    let path = opts.path;
    if !path.is_dir() {
        return Err(format!(
            "Given path '{}' is not a directory :(",
            path.display()
        ));
    }
    let mut registry = Registry::default();
    let collector = Box::new(prometheus::PhotoBacklogCollector {
        scan_path: path,
        ignored_exts: opts.ignored_exts,
        age_buckets: opts.age_buckets,
        owner: opts.owner,
        group: opts.group,
    });
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
    let addr = SocketAddr::from((opts.listen, opts.port));
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
