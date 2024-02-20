use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::Metadata;
use std::num::ParseFloatError;
use std::option::Option;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime};

use log::{error, info};
use walkdir::WalkDir;

use prometheus_client::encoding::{EncodeLabelValue, LabelValueEncoder};
use prometheus_client::metrics::histogram::Histogram;

const WEEK: f64 = 7.0 * 86400.0;

const ROOT_FILE_DIR: &str = ".";

pub mod prometheus;

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
        return Some(PathBuf::from(ROOT_FILE_DIR));
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

/// Simple conversion of a comma-separated string into a vector of OsString values.
/// Example:
/// ```
/// use std::ffi::OsString;
/// assert_eq!(photo_backlog_exporter::parse_exts(""), Vec::<OsString>::from([]));
/// assert_eq!(photo_backlog_exporter::parse_exts("a"), Vec::<OsString>::from([OsString::from("a")]));
/// assert_eq!(photo_backlog_exporter::parse_exts("a,"), Vec::<OsString>::from([OsString::from("a")]));
/// assert_eq!(photo_backlog_exporter::parse_exts("a,b"),
///   Vec::<OsString>::from([OsString::from("a"), OsString::from("b")]));
/// assert_eq!(photo_backlog_exporter::parse_exts("a,,b"),
///   Vec::<OsString>::from([OsString::from("a"), OsString::from("b")]));
/// ```
pub fn parse_exts(s: &str) -> Vec<OsString> {
    s.split(',')
        .filter(|c| !c.is_empty())
        .map(OsString::from)
        .collect()
}

/// Simple conversion of a list of comma-separated week numbers into a vector of second values,
/// with failure handling.
/// Example:
/// ```
/// assert_eq!(photo_backlog_exporter::parse_weeks(""), Ok(Vec::<f64>::from([])));
/// assert_eq!(photo_backlog_exporter::parse_weeks("0,1"), Ok(Vec::<f64>::from([0.0, 7.0*24.0*3600.0])));
/// assert!(photo_backlog_exporter::parse_weeks("a").is_err());
/// ```
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

pub fn check_ownership(config: &Config, path: &Path, m: &Metadata, kind: &str) -> bool {
    let mut good = true;
    if let Some(owner) = config.owner {
        good &= owner == m.uid();
    }
    if let Some(group) = config.group {
        good &= group == m.gid();
    }
    if !good {
        fn format_id(m_id: Option<u32>) -> String {
            match m_id {
                None => "(not checked)".to_string(),
                Some(p) => p.to_string(),
            }
        }
        info!(
            "{} '{}' has wrong owner:group {}:{}, expected {}:{}",
            kind,
            path.display(),
            m.uid(),
            m.gid(),
            format_id(config.owner),
            format_id(config.group)
        );
    }
    good
}

pub struct Config<'a> {
    pub root_path: &'a Path,
    pub ignored_exts: &'a [OsString],
    pub owner: Option<u32>,
    pub group: Option<u32>,
}

#[derive(Debug)]
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

    pub fn scan(&mut self, config: &Config, now: SystemTime) {
        for entry in WalkDir::new(config.root_path) {
            match entry {
                Err(e) => {
                    info!("Error while scanning recursively: {}", e);
                    self.record_error();
                }
                Ok(entry) => {
                    if entry.file_type().is_dir() {
                        match entry.metadata() {
                            Ok(m) => {
                                if !check_ownership(config, entry.path(), &m, "Directory") {
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
                            if config.ignored_exts.iter().any(|c| c == ext) {
                                continue;
                            }
                        }
                    }

                    self.record_file();

                    // Here it's not an ignored entry, so let's process it.

                    // Find owner top-level dir.
                    let parent = match relative_top(config.root_path, entry.path()) {
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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;
    use tempfile::tempdir;
    use tempfile::TempDir;
    extern crate speculoos;
    use speculoos::prelude::*;

    use crate::Config;
    use crate::{Backlog, ROOT_FILE_DIR};

    const SUBDIR: &str = "dir1";

    fn build_config(p: &Path, owner: Option<u32>, group: Option<u32>) -> Config {
        Config {
            root_path: p,
            ignored_exts: &[],
            owner,
            group,
        }
    }

    fn get_subdir() -> (TempDir, PathBuf) {
        let temp_dir = tempdir().unwrap();
        let subdir = temp_dir.path().join(SUBDIR);
        std::fs::create_dir(&subdir).expect("Can't create subdir");
        (temp_dir, subdir)
    }

    fn add_file(d: &Path, name: &str) -> PathBuf {
        let mut p = PathBuf::from(d);
        p.push(name);
        std::fs::write(&p, b"").expect("Can't create file");
        p
    }

    fn check_backlog(
        backlog: &Backlog,
        expect_folders: usize,
        expect_files: i64,
        expect_errors: i64,
    ) {
        assert_that!(backlog.folders).has_length(expect_folders);
        assert_that!(backlog.total_files).is_equal_to(expect_files);
        assert_that!(backlog.total_errors).is_equal_to(expect_errors);
    }

    fn check_has_dir_with(backlog: &Backlog, folder: &str, file_count: i64) {
        let folder_sizes: HashMap<String, i64> = backlog
            .folders
            .iter()
            .map(|(key, &value)| (key.clone(), value.0))
            .collect();
        assert_that!(&folder_sizes)
            .named("folder_sizes")
            .contains_entry(folder.to_string(), file_count);
    }

    #[test]
    fn empty_dir() {
        let temp_dir = tempdir().unwrap();
        let config = build_config(temp_dir.path(), None, None);
        let mut backlog = Backlog::new([].into_iter());
        let now = SystemTime::now();
        backlog.scan(&config, now);
        check_backlog(&backlog, 0, 0, 0);
    }
    #[test]
    fn empty_dir_is_empty() {
        let (temp_dir, _) = get_subdir();
        let config = build_config(temp_dir.path(), None, None);
        let mut backlog = Backlog::new([].into_iter());
        let now = SystemTime::now();
        backlog.scan(&config, now);
        check_backlog(&backlog, 0, 0, 0);
    }
    #[test]
    fn no_extension_is_ignored() {
        let (temp_dir, subdir) = get_subdir();
        add_file(&subdir, "readme");
        let config = build_config(temp_dir.path(), None, None);
        let mut backlog = Backlog::new([].into_iter());
        let now = SystemTime::now();
        backlog.scan(&config, now);
        check_backlog(&backlog, 0, 0, 0);
    }
    #[test]
    fn ignored_extension_is_ignored() {
        let (temp_dir, subdir) = get_subdir();
        add_file(&subdir, "file.nef");
        add_file(&subdir, "file.xmp");
        let mut config = build_config(temp_dir.path(), None, None);
        let exts = [OsString::from("xmp")];
        config.ignored_exts = &exts;
        let mut backlog = Backlog::new([].into_iter());
        let now = SystemTime::now();
        backlog.scan(&config, now);
        check_backlog(&backlog, 1, 1, 0);
        check_has_dir_with(&backlog, SUBDIR, 1);
    }
    #[test]
    fn one_dir_one_file() {
        let (temp_dir, subdir) = get_subdir();
        add_file(&subdir, "file.nef");
        let config = build_config(temp_dir.path(), None, None);
        let mut backlog = Backlog::new([].into_iter());
        let now = SystemTime::now();
        backlog.scan(&config, now);
        check_backlog(&backlog, 1, 1, 0);
        check_has_dir_with(&backlog, SUBDIR, 1);
    }
    #[test]
    fn one_dir_two_files() {
        let (temp_dir, subdir) = get_subdir();
        add_file(&subdir, "dsc001.nef");
        add_file(&subdir, "dsc002.jpg");
        let config = build_config(temp_dir.path(), None, None);
        let mut backlog = Backlog::new([].into_iter());
        let now = SystemTime::now();
        backlog.scan(&config, now);
        check_backlog(&backlog, 1, 2, 0);
        check_has_dir_with(&backlog, SUBDIR, 2);
    }
    #[test]
    fn file_in_root_dir() {
        let temp_dir = tempdir().unwrap();
        add_file(temp_dir.path(), "file.nef");
        let config = build_config(temp_dir.path(), None, None);
        let mut backlog = Backlog::new([].into_iter());
        let now = SystemTime::now();
        backlog.scan(&config, now);
        check_backlog(&backlog, 1, 1, 0);
        check_has_dir_with(&backlog, ROOT_FILE_DIR, 1);
    }

    #[test]
    fn no_such_dir() {
        let (temp_dir, _subdir) = get_subdir();
        let mut missing_dir = temp_dir.path().to_path_buf();
        missing_dir.push("no-such_dir");
        let mut backlog = Backlog::new([].into_iter());
        let config = build_config(&missing_dir, None, None);
        let now = SystemTime::now();
        backlog.scan(&config, now);
        check_backlog(&backlog, 0, 0, 1);
    }

    enum FailMode {
        NoCheck,
        Good,
        Bad,
    }
    #[rstest]
    fn test_permissions(
        #[values(FailMode::NoCheck, FailMode::Good, FailMode::Bad)] user_mode: FailMode,
        #[values(FailMode::NoCheck, FailMode::Good, FailMode::Bad)] group_mode: FailMode,
    ) {
        let temp_dir = tempdir().unwrap();
        let fname = add_file(temp_dir.path(), "file.nef");
        let m = std::fs::metadata(fname).expect("Can't stat just created file!");
        fn generate_check(mode: &FailMode, id: u32) -> Option<u32> {
            match mode {
                FailMode::NoCheck => None,
                FailMode::Good => Some(id),
                FailMode::Bad => Some(id + 1),
            }
        }
        let user_check = generate_check(&user_mode, m.uid());
        let group_check = generate_check(&group_mode, m.gid());
        // No permissions check.
        let config = build_config(temp_dir.path(), user_check, group_check);
        let mut backlog = Backlog::new([].into_iter());
        let now = SystemTime::now();
        backlog.scan(&config, now);
        let expected_errors = match (user_mode, group_mode) {
            (FailMode::Bad, _) | (_, FailMode::Bad) => 1,
            _ => 0,
        };
        check_backlog(&backlog, 1, 1, expected_errors);
        check_has_dir_with(&backlog, ROOT_FILE_DIR, 1);
    }
}
