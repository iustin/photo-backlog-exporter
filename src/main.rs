use photo_backlog_exporter::*;

#[tokio::main]
async fn main() -> Result<(), String> {
    let opts = match cli::init_binary()? {
        None => return Ok(()),
        Some(opts) => opts,
    };

    let (addr, app) = daemon::build_app(opts);
    daemon::run_daemon(addr, app).await
}
