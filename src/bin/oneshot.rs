use cli::enable_logging;
use gumdrop::Options as _;
use log::info;
use photo_backlog_exporter::*;

fn main() -> Result<(), String> {
    enable_logging();
    let opts = cli::parse_args()?;
    if opts.help_requested() {
        eprintln!("{}", cli::CliOptions::usage());
        return Ok(());
    }
    info!("Starting up with the following options: {:?}", opts);

    let collector = cli::collector_from_args(opts);
    let buffer = prometheus::encode_to_text(collector).map_err(|e| e.to_string())?;
    println!("{}", buffer);
    Ok(())
}
