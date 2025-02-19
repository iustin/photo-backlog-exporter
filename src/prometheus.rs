use std::ffi::OsString;

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

use std::time::{Instant, SystemTime};

use prometheus_client::collector::Collector;
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::DescriptorEncoder;
use prometheus_client::encoding::EncodeMetric;
use prometheus_client::encoding::{EncodeLabelSet, EncodeLabelValue, LabelValueEncoder};
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::{ConstGauge, Gauge};
use prometheus_client::registry::Registry;

pub const PROCESSING_TIME_NAME: &str = "photo_backlog_processing_time_seconds";
pub const PROCESSING_TIME_HELP: &str = "Processing time for scanning the backlog";

#[derive(Debug)]
pub struct PhotoBacklogCollector {
    pub scan_path: PathBuf,
    pub ignored_exts: Vec<OsString>,
    pub raw_exts: Vec<OsString>,
    pub editable_exts: Vec<OsString>,
    pub age_buckets: Vec<f64>,
    pub raw_owner: Option<u32>,
    pub editable_owners: Vec<u32>,
    pub group: Option<u32>,
    pub dir_mode: Option<u32>,
    pub raw_file_mode: Option<u32>,
    pub editable_file_mode: Option<u32>,
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
            raw_exts: &self.raw_exts,
            editable_exts: &self.editable_exts,
            raw_owner: self.raw_owner,
            editable_owners: &self.editable_owners,
            group: self.group,
            dir_mode: self.dir_mode,
            raw_file_mode: self.raw_file_mode,
            editable_file_mode: self.editable_file_mode,
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

        for (kind, count) in &backlog.total_errors {
            let labels = ErrorLabels { kind: *kind };
            errors_fam.get_or_create(&labels).set(*count);
        }

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

pub fn encode_to_text(collector: PhotoBacklogCollector) -> Result<String, std::fmt::Error> {
    let mut registry = Registry::default();
    registry.register_collector(Box::new(collector));
    let mut buffer = String::new();
    encode(&mut buffer, &registry).and(Ok(buffer))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use rstest::rstest;
    use tempfile::tempdir;

    extern crate speculoos;
    use speculoos::prelude::*;

    /// Runs the collector with a variety of folder configurations and
    /// checks that the encoding contains a few expected values.
    /// Note not all encoded values are tested.
    #[rstest]
    #[case::empty_dir([0].to_vec())]
    #[case::one_dir_one_file([1].to_vec())]
    #[case::two_dirs_one_two([1, 2].to_vec())]
    #[case::three_dirs_one_zero_two([1, 0, 2].to_vec())]
    fn test_backlog_encoding(#[case] folders_config: Vec<i32>) {
        fn format_dir(pos: usize) -> String {
            format!("dir-{}", pos)
        }
        let temp_dir = tempdir().unwrap();
        for (pos, folder_size) in folders_config.iter().enumerate() {
            let folder = temp_dir.path().join(format_dir(pos));
            std::fs::create_dir(&folder).unwrap();
            for i in 0..*folder_size {
                let file = folder.join(format!("{}.nef", i));
                std::fs::File::create(&file).unwrap();
            }
        }
        let collector = super::PhotoBacklogCollector {
            scan_path: temp_dir.path().to_path_buf(),
            ignored_exts: vec![],
            raw_exts: vec![OsString::from("nef")],
            editable_exts: vec![],
            age_buckets: vec![1.0],
            raw_owner: None,
            editable_owners: vec![],
            group: None,
            dir_mode: None,
            raw_file_mode: None,
            editable_file_mode: None,
        };
        let buffer = super::encode_to_text(collector).unwrap();

        // Now check the encoded values.
        let total_photos = folders_config.iter().sum::<i32>();
        let photos_string = format!("photo_backlog_counts{{kind=\"photos\"}} {}", total_photos);
        assert_that(&buffer).contains(&photos_string);
        let folder_string = format!(
            "photo_backlog_counts{{kind=\"folders\"}} {}",
            folders_config.iter().filter(|x| **x > 0).count()
        );
        assert!(buffer.contains(&folder_string));
        for (pos, folder_size) in folders_config.iter().enumerate() {
            if *folder_size == 0 {
                continue;
            }
            let folder_string = format!(
                "photo_backlog_folder_sizes{{path=\"{}\"}} {}",
                format_dir(pos),
                folder_size
            );
            assert_that(&buffer).contains(&folder_string);
        }
        assert_that!(buffer).contains("photo_backlog_processing_time_seconds ");
        let ages_string = format!("photo_backlog_ages_count {}", total_photos);
        assert_that!(buffer).contains(ages_string);
        assert_that!(buffer).contains("photo_backlog_errors{kind=\"scan\"} 0");
        assert_that!(buffer).contains("photo_backlog_errors{kind=\"ownership\"} 0");
    }
}
