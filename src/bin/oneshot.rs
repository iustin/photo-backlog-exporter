use photo_backlog_exporter::*;

fn main() -> Result<(), String> {
    let opts = match cli::init_binary()? {
        None => return Ok(()),
        Some(opts) => opts,
    };

    let collector = cli::collector_from_args(opts);
    let buffer = prometheus::encode_to_text(collector).map_err(|e| e.to_string())?;
    println!("{buffer}");
    Ok(())
}
