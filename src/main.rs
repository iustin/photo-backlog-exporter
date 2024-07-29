use gumdrop::Options as _;
use log::{debug, info};

use cli::enable_logging;
use photo_backlog_exporter::*;

#[tokio::main]
async fn main() -> Result<(), String> {
    enable_logging();

    let opts = cli::parse_args()?;
    if opts.help_requested() {
        debug!("Help requested, showing usage and exiting.");
        eprintln!("{}", cli::CliOptions::usage());
        return Ok(());
    }
    info!("Starting up with the following options: {:?}", opts);

    let (addr, app) = daemon::build_app(opts);
    daemon::run_daemon(addr, app).await
}
