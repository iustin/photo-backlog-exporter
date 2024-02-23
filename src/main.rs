use std::io::Write;

use gumdrop::Options as _;
use log::info;

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
    if opts.help_requested() {
        eprintln!("{}", cli::CliOptions::usage());
        return Ok(());
    }
    info!("Starting up with the following options: {:?}", opts);

    let (addr, app) = daemon::build_app(opts);
    daemon::run_daemon(addr, app).await
}
