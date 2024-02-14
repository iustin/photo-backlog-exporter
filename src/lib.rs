use std::collections::HashMap;
use std::ffi::OsString;
use std::option::Option;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};
use std::num::ParseFloatError;
use std::str::FromStr;

use log::{error, info};
use walkdir::WalkDir;

use prometheus_client::encoding::{EncodeLabelValue, LabelValueEncoder};
use prometheus_client::metrics::histogram::Histogram;

const WEEK: f64 = 7.0 * 86400.0;

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

pub fn parse_exts(s: &str) -> Vec<OsString> {
    s.split(',')
        .filter(|c| !c.is_empty())
        .map(OsString::from)
        .collect()
}

pub fn parse_weeks(s: &str) -> Result<Vec<f64>, ParseFloatError> {
    s.split(',')
        .filter(|c| !c.is_empty())
        .map(f64::from_str)
        .map(|r| r.map(|f| f * WEEK))
        .collect()
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

pub struct Backlog<'a> {
    pub root_path: &'a Path,
    owner: u32,
    group: u32,
    pub total_errors: i64,
    pub total_files: i64,
    pub folders: HashMap<String, (i64, f64)>,
    pub ages_histogram: Histogram,
}

impl<'a> Backlog<'a> {
    pub fn new<P: AsRef<Path>>(
        root: &'a P,
        owner: u32,
        group: u32,
        buckets: impl Iterator<Item = f64>,
    ) -> Self {
        Self {
            root_path: root.as_ref(),
            owner,
            group,
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

    pub fn scan(&mut self, ignored_exts: &[OsString], now: SystemTime) {
        for entry in WalkDir::new(self.root_path) {
            match entry {
                Err(e) => {
                    info!("Error while scanning recursively: {}", e);
                    self.record_error();
                }
                Ok(entry) => {
                    if entry.file_type().is_dir() {
                        match entry.metadata() {
                            Ok(m) => {
                                if m.uid() != self.owner || m.gid() != self.group {
                                    info!(
                                        "Directory {} has wrong owner:group {}:{}",
                                        entry.path().display(),
                                        m.uid(),
                                        m.gid()
                                    );
                                    self.record_error();
                                }
                            }
                            Err(e) => {
                                info!("Can't stat directory {}: {}", entry.path().display(), e);
                                self.record_error();
                            }
                        }
                        // We don't track directories by themselves,
                        // only via file contents.
                        continue;
                    }
                    match entry.path().extension() {
                        None => continue,
                        Some(ext) => {
                            if ignored_exts.iter().any(|c| c == ext) {
                                continue;
                            }
                        }
                    }

                    self.record_file();

                    // Here it's not an ignored entry, so let's process it.

                    // Find owner top-level dir.
                    let parent = match relative_top(self.root_path, entry.path()) {
                        Some(x) => x,
                        None => {
                            error!(
                                "Can't determine parent path for {}",
                                entry.path().to_string_lossy()
                            );
                            continue;
                        }
                    };

                    // And convert to valid UTF-8 string via lossy
                    // conversion. But at least we're back in safe land.
                    let folder = String::from(parent.to_string_lossy());

                    // Now update folders struct.
                    let age = relative_age(now, &entry).as_secs_f64();
                    self.folders
                        .entry(folder)
                        .and_modify(|(c, a)| {
                            *c += 1;
                            *a += age;
                        })
                        .or_insert((1, age));
                    // And observe the age for the ages histogram.
                    self.ages_histogram.observe(age);
                }
            }
        }
    }
}
