use std::ffi::OsString;

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

use std::time::{Instant, SystemTime};

use prometheus_client::collector::Collector;

use prometheus_client::encoding::DescriptorEncoder;
use prometheus_client::encoding::EncodeMetric;
use prometheus_client::encoding::{EncodeLabelSet, EncodeLabelValue, LabelValueEncoder};
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::{ConstGauge, Gauge};

pub const PROCESSING_TIME_NAME: &str = "photo_backlog_processing_time_seconds";
pub const PROCESSING_TIME_HELP: &str = "Processing time for scanning the backlog";

#[derive(Debug)]
pub struct PhotoBacklogCollector {
    pub scan_path: PathBuf,
    pub ignored_exts: Vec<OsString>,
    pub age_buckets: Vec<f64>,
    pub owner: Option<u32>,
    pub group: Option<u32>,
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
    kind: super::ErrorType,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct FolderLabels {
    path: String,
}

impl Collector for PhotoBacklogCollector {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), std::fmt::Error> {
        let instant = Instant::now(); // for this processor's execution time.
        let now = SystemTime::now(); // for file age, which is seconds.

        let config = super::Config {
            root_path: &self.scan_path,
            ignored_exts: &self.ignored_exts,
            owner: self.owner,
            group: self.group,
        };

        let mut backlog = super::Backlog::new(self.age_buckets.iter().copied());

        backlog.scan(&config, now);

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
                kind: super::ErrorType::Scan,
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
