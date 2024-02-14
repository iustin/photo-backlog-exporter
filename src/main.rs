use std::ffi::OsString;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use axum::{routing::get, Router};
use gumdrop::Options;
use log::info;

use prometheus_client::collector::Collector;
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::DescriptorEncoder;
use prometheus_client::encoding::EncodeMetric;
use prometheus_client::encoding::{EncodeLabelSet, EncodeLabelValue, LabelValueEncoder};
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::{ConstGauge, Gauge};
use prometheus_client::registry::Registry;

use photo_backlog_exporter::*;

const PROCESSING_TIME_NAME: &str = "photo_backlog_processing_time_seconds";
const PROCESSING_TIME_HELP: &str = "Processing time for scanning the backlog";

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

    #[options(help = "Owner expected for all files", required)]
    owner: u32,

    #[options(help = "Group expected for all files", required)]
    group: u32,
}

#[derive(Debug)]
struct PhotoBacklogCollector {
    scan_path: PathBuf,
    ignored_exts: Vec<OsString>,
    age_buckets: Vec<f64>,
    owner: u32,
    group: u32,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct TotalLabels {
    kind: ItemType,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
enum ItemType {
    Photos,
    Folders,
}

impl EncodeLabelValue for ItemType {
    fn encode(&self, encoder: &mut LabelValueEncoder) -> Result<(), std::fmt::Error> {
        let s = match self {
            ItemType::Photos => "photos",
            ItemType::Folders => "folders",
        };
        EncodeLabelValue::encode(&s, encoder)
    }
}
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ErrorLabels {
    kind: ErrorType,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct FolderLabels {
    path: String,
}

impl Collector for PhotoBacklogCollector {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), std::fmt::Error> {
        let instant = Instant::now(); // for this processor's execution time.
        let now = SystemTime::now(); // for file age, which is seconds.

        let mut backlog = Backlog::new(
            &self.scan_path,
            self.owner,
            self.group,
            self.age_buckets.iter().copied(),
        );

        backlog.scan(&self.ignored_exts, now);

        let totals_fam = Family::<TotalLabels, Gauge>::default();
        let errors_fam = Family::<ErrorLabels, Gauge>::default();
        let folder_sizes_fam = Family::<FolderLabels, Gauge>::default();
        let folder_ages_fam = Family::<FolderLabels, Gauge<f64, AtomicU64>>::default();

        totals_fam
            .get_or_create(&TotalLabels {
                kind: ItemType::Photos,
            })
            .set(backlog.total_files);
        totals_fam
            .get_or_create(&TotalLabels {
                kind: ItemType::Folders,
            })
            .set(
                backlog
                    .folders
                    .len()
                    .try_into()
                    .expect("More than 2^63 entries in the map?!"),
            );

        errors_fam
            .get_or_create(&ErrorLabels {
                kind: ErrorType::Scan,
            })
            .set(backlog.total_errors);

        for (path, (cnt, age)) in backlog.folders.drain() {
            let labels = FolderLabels { path };
            folder_sizes_fam.get_or_create(&labels).set(cnt);
            folder_ages_fam.get_or_create(&labels).set(age);
        }

        let totals_encoder = encoder
            .encode_descriptor(
                "photo_backlog_counts",
                "Number of items in the photo backlog",
                None,
                totals_fam.metric_type(),
            )
            .expect("create totals_encoder");

        totals_fam
            .encode(totals_encoder)
            .expect("encode totals family");

        let errors_encoder = encoder
            .encode_descriptor(
                "photo_backlog_errors",
                "Number of errors in the photo backlog",
                None,
                totals_fam.metric_type(),
            )
            .expect("create errors_encoder");

        errors_fam
            .encode(errors_encoder)
            .expect("encode errors family");

        let folder_sizes_encoder = encoder
            .encode_descriptor(
                "photo_backlog_folder_sizes",
                "Size of folders in the backlog",
                None,
                folder_sizes_fam.metric_type(),
            )
            .expect("create totals_encoder");

        folder_sizes_fam
            .encode(folder_sizes_encoder)
            .expect("encode folder sizes");

        let folder_ages_encoder = encoder
            .encode_descriptor(
                "photo_backlog_folder_ages",
                "Per-folder picture-seconds backlog",
                None,
                folder_ages_fam.metric_type(),
            )
            .expect("create totals_encoder");

        folder_ages_fam
            .encode(folder_ages_encoder)
            .expect("encode folder sizes");

        let ages_histogram_encoder = encoder
            .encode_descriptor(
                "photo_backlog_ages",
                "Age of files in the backlog",
                None,
                backlog.ages_histogram.metric_type(),
            )
            .expect("create ages_histogram_encoderr");

        backlog
            .ages_histogram
            .encode(ages_histogram_encoder)
            .expect("encode ages_histogram");

        let elapsed_gauge = ConstGauge::new(instant.elapsed().as_secs_f64());
        let elapsed_encoder = encoder
            .encode_descriptor(
                PROCESSING_TIME_NAME,
                PROCESSING_TIME_HELP,
                None,
                elapsed_gauge.metric_type(),
            )
            .expect("register gauge");
        elapsed_gauge
            .encode(elapsed_encoder)
            .expect("encode elapsed");
        Ok(())
    }
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
    let collector = Box::new(PhotoBacklogCollector {
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
