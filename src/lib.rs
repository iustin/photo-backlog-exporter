use std::option::Option;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use prometheus_client::encoding::{EncodeLabelValue, LabelValueEncoder};
use prometheus_client::metrics::histogram::Histogram;

/// Returns the first directory from a given path.
/// Example:
/// ```
/// # use std::path::{PathBuf, Path};
/// assert!(photo_backlog_exporter::first_dir(Path::new("")).is_none());
/// assert_eq!(photo_backlog_exporter::first_dir(Path::new("a")), Some(PathBuf::from(".")));
/// assert_eq!(photo_backlog_exporter::first_dir(Path::new("a/b")), Some(PathBuf::from("a")));
/// assert_eq!(photo_backlog_exporter::first_dir(Path::new("/a/b")), Some(PathBuf::from("a")));
/// ```
pub fn first_dir(p: &Path) -> Option<PathBuf> {
    // Find first element that is a normal component.
    let parent = p.components().find_map(|c| match c {
        Component::Normal(d) => Some(d),
        _ => None,
    })?;
    if parent == p {
        return Some(PathBuf::from("."));
    }
    // And convert to valid UTF-8 string via lossy conversion. But we're back in safe land.
    let parent2: &Path = parent.as_ref();
    //let parent3 = Path::from(parent2.to_string_lossy());
    Some(PathBuf::from(parent2))
}
/// Returns the first directory from a given path, after removing a top prefix.
/// Example:
/// ```
/// # use std::path::{PathBuf, Path};
/// assert!(photo_backlog_exporter::relative_top(Path::new("/a/b"), Path::new("")).is_none());
/// assert_eq!(photo_backlog_exporter::relative_top(Path::new("a"), Path::new("a/b/c")), Some(PathBuf::from("b")));
/// assert_eq!(photo_backlog_exporter::relative_top(Path::new("a/b/"), Path::new("a/b/c")), Some(PathBuf::from(".")));
/// assert_eq!(photo_backlog_exporter::relative_top(Path::new("/a/b/c"), Path::new("/a/b/c/d/e/f")), Some(PathBuf::from("d")));
/// ```
pub fn relative_top(root: &Path, p: &Path) -> Option<PathBuf> {
    let relative = p.strip_prefix(root).ok()?;
    first_dir(relative)
}

/// Returns the age of a file relative to a given timestamp, or zero if the file is newer.
pub fn relative_age(reference: SystemTime, entry: &walkdir::DirEntry) -> Duration {
    let modified = match entry.metadata() {
        Ok(m) => m.modified().unwrap_or(reference),
        Err(_) => reference,
    };
    reference.duration_since(modified).unwrap_or(Duration::ZERO)
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum ErrorType {
    Scan,
}

impl EncodeLabelValue for ErrorType {
    fn encode(&self, encoder: &mut LabelValueEncoder) -> Result<(), std::fmt::Error> {
        let s = match self {
            ErrorType::Scan => "scan",
        };
        EncodeLabelValue::encode(&s, encoder)
    }
}

pub struct Backlog {
    pub total_errors: i64,
    pub total_files: i64,
    pub folders: HashMap<String, (i64, f64)>,
    pub ages_histogram: Histogram,
}

impl Backlog {
    pub fn new(buckets: impl Iterator<Item = f64>) -> Self {
        Self {
            total_errors: 0,
            total_files: 0,
            folders: HashMap::new(),
            ages_histogram: Histogram::new(buckets),
        }
    }
    pub fn record_file(&mut self) {
        self.total_files += 1;
    }

    pub fn record_error(&mut self) {
        self.total_errors += 1;
    }
}
