use std::ffi::OsString;
use std::path::PathBuf;

use gumdrop::Options;
use log::info;

use prometheus_client::encoding::text::encode;
use prometheus_client::registry::Registry;

use photo_backlog_exporter::*;

#[derive(Debug, Options)]
struct CliOptions {
    #[options(help = "print help message")]
    help: bool,

    #[options(
        help = "path to root of incoming photo directory",
        required,
        short = "P"
    )]
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

    #[options(help = "Owner expected for all files", required)]
    owner: u32,

    #[options(help = "Group expected for all files", required)]
    group: u32,
}

fn main() -> Result<(), String> {
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
    let mut buffer = String::new();
    encode(&mut buffer, &registry).unwrap();
    println!("{}", buffer);
    Ok(())
}