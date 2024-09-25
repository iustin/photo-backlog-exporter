use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::Metadata;
use std::option::Option;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use log::{info, warn};
use walkdir::WalkDir;

use prometheus_client::encoding::{EncodeLabelValue, LabelValueEncoder};
use prometheus_client::metrics::histogram::Histogram;

const ROOT_FILE_DIR: &str = ".";

pub mod cli;
pub mod daemon;
pub mod prometheus;

/// Returns the first named directory from a given path.
///
/// When no named directories are passed (see examples), the behaviour
/// is to return none.
///
/// Example:
/// ```
/// # use std::path::{PathBuf, Path};
/// assert!(photo_backlog_exporter::first_dir(Path::new("")).is_none());
/// assert!(photo_backlog_exporter::first_dir(Path::new("a")).is_none());
/// assert_eq!(photo_backlog_exporter::first_dir(Path::new("a/b")), Some(PathBuf::from("a")));
/// assert_eq!(photo_backlog_exporter::first_dir(Path::new("/a/b")), Some(PathBuf::from("a")));
/// assert!(photo_backlog_exporter::first_dir(Path::new(".")).is_none());
/// assert!(photo_backlog_exporter::first_dir(Path::new("..")).is_none());
/// ```
pub fn first_dir(p: &Path) -> Option<PathBuf> {
    // Find first element that is a normal component.
    let parent = p.components().find_map(|c| match c {
        Component::Normal(d) => Some(d),
        _ => None,
    })?;
    if parent == p {
        // No parent for this item, so return None in this case.
        return None;
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
/// assert!(photo_backlog_exporter::relative_top(Path::new("a/b/"), Path::new("a/b/c")).is_none());
/// assert_eq!(photo_backlog_exporter::relative_top(Path::new("/a/b/c"), Path::new("/a/b/c/d/e/f")), Some(PathBuf::from("d")));
/// ```
pub fn relative_top(root: &Path, p: &Path) -> Option<PathBuf> {
    let relative = p.strip_prefix(root).ok()?;
    first_dir(relative)
}

/// Returns the age of a file relative to a given timestamp, or zero if the file is newer.
pub fn relative_age(reference: SystemTime, m: &Metadata) -> Duration {
    let modified = m.modified().unwrap_or(reference);
    reference.duration_since(modified).unwrap_or(Duration::ZERO)
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum ErrorType {
    Scan,
    Ownership,
    Permissions,
}

impl EncodeLabelValue for ErrorType {
    fn encode(&self, encoder: &mut LabelValueEncoder) -> Result<(), std::fmt::Error> {
        let s = match self {
            ErrorType::Scan => "scan",
            ErrorType::Ownership => "ownership",
            ErrorType::Permissions => "permissions",
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

pub fn check_mode(config: &Config, path: &Path, m: &Metadata) -> bool {
    let mut good = true;
    let mut kind = "(unknown)";
    let mut expected = 0o0;
    let actual = m.mode() & 0o777;
    if m.is_dir() {
        kind = "directory";
        if let Some(dir_mode) = config.dir_mode {
            expected = dir_mode;
            good &= dir_mode == actual;
        }
    } else if m.is_file() {
        kind = "file";
        if let Some(raw_file_mode) = config.raw_file_mode {
            expected = raw_file_mode;
            good &= raw_file_mode == actual;
        }
    }
    if !good {
        info!(
            "{} '{}' has wrong mode {:o}, expected {:o}",
            kind,
            path.display(),
            actual,
            expected,
        );
    }
    good
}

pub struct Config<'a> {
    pub root_path: &'a Path,
    pub ignored_exts: &'a [OsString],
    pub raw_exts: &'a [OsString],
    pub editable_exts: &'a [OsString],
    pub owner: Option<u32>,
    pub group: Option<u32>,
    pub dir_mode: Option<u32>,
    pub raw_file_mode: Option<u32>,
    pub editable_file_mode: Option<u32>,
}

#[derive(Debug)]
pub struct Backlog {
    pub total_errors: HashMap<ErrorType, i64>,
    pub total_files: i64,
    pub folders: HashMap<String, (i64, f64)>,
    pub ages_histogram: Histogram,
}

impl Backlog {
    pub fn new(buckets: impl Iterator<Item = f64>) -> Self {
        Self {
            total_errors: HashMap::from([
                (ErrorType::Scan, 0),
                (ErrorType::Ownership, 0),
                (ErrorType::Permissions, 0),
            ]),
            total_files: 0,
            folders: HashMap::new(),
            ages_histogram: Histogram::new(buckets),
        }
    }
    pub fn record_file(&mut self) {
        self.total_files += 1;
    }

    pub fn record_error(&mut self, err: ErrorType) {
        self.total_errors
            .entry(err)
            .and_modify(|f| *f += 1)
            .or_insert(1);
    }

    pub fn scan(&mut self, config: &Config, now: SystemTime) {
        for maybe_entry in WalkDir::new(config.root_path) {
            let entry = match maybe_entry {
                Err(e) => {
                    info!("Error while scanning recursively: {}", e);
                    self.record_error(ErrorType::Scan);
                    continue;
                }
                Ok(entry) => entry,
            };
            let path = entry.path();
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    info!("Can't stat '{}': {}", path.display(), e);
                    self.record_error(ErrorType::Scan);
                    continue;
                }
            };
            if entry.file_type().is_dir() {
                if !check_ownership(config, path, &metadata, "Directory") {
                    self.record_error(ErrorType::Ownership);
                }
                if !check_mode(config, path, &metadata) {
                    self.record_error(ErrorType::Permissions);
                }
                // We don't track directories by themselves,
                // only via file contents.
                continue;
            }
            if !entry.file_type().is_file() {
                // We don't care about other file types.
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

            // Here it's not an ignored entry, so let's process it.
            self.record_file();
            if !check_ownership(config, path, &metadata, "File") {
                self.record_error(ErrorType::Ownership);
            }
            if !check_mode(config, path, &metadata) {
                self.record_error(ErrorType::Permissions);
            }

            // Find owner top-level dir.
            let parent = match relative_top(config.root_path, entry.path()) {
                Some(x) => x,
                None => {
                    warn!(
                        "Can't determine parent path for {}",
                        entry.path().to_string_lossy()
                    );
                    PathBuf::from(ROOT_FILE_DIR)
                }
            };

            // And convert to valid UTF-8 string via lossy
            // conversion. But at least we're back in safe land.
            let folder = String::from(parent.to_string_lossy());

            // Now update folders struct.
            let age = relative_age(now, &metadata).as_secs_f64();
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

#[cfg(test)]
mod tests {
    use rstest::fixture;
    use rstest::rstest;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;
    use tempfile::tempdir;
    use tempfile::TempDir;
    extern crate speculoos;
    use speculoos::prelude::*;

    use crate::{Backlog, ROOT_FILE_DIR};
    use crate::{Config, ErrorType};

    const SUBDIR: &str = "dir1";

    pub struct TestData {
        pub temp_dir: TempDir,
        pub now: SystemTime,
        pub ignored_exts: Vec<OsString>,
        pub raw_exts: Vec<OsString>,
        pub editable_exts: Vec<OsString>,
    }

    impl TestData {
        pub fn get_subdir(&self) -> PathBuf {
            let subdir = self.temp_dir.path().join(SUBDIR);
            std::fs::create_dir(&subdir).expect("Can't create subdir");
            subdir
        }

        pub fn build_config(
            &self,
            owner: Option<u32>,
            group: Option<u32>,
            dir_mode: Option<u32>,
            raw_file_mode: Option<u32>,
        ) -> Config {
            Config {
                root_path: self.temp_dir.path(),
                ignored_exts: &self.ignored_exts,
                raw_exts: &self.raw_exts,
                editable_exts: &self.editable_exts,
                owner,
                group,
                dir_mode,
                raw_file_mode,
                editable_file_mode: None,
            }
        }
    }

    // This can't be moved into TestData because it needs to be mutable, and
    // that breaks the read-only borrow that Config does on the TestData
    // members.
    #[fixture]
    fn backlog() -> Backlog {
        Backlog::new([].into_iter())
    }

    #[fixture]
    fn test_data() -> TestData {
        TestData {
            temp_dir: tempdir().unwrap(),
            now: SystemTime::now(),
            ignored_exts: vec![],
            raw_exts: vec![],
            editable_exts: vec![],
        }
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
        scan_errors: i64,
        ownership_errors: i64,
        permissions_errors: i64,
    ) {
        assert_that!(backlog.folders).has_length(expect_folders);
        assert_that!(backlog.total_files).is_equal_to(expect_files);
        assert_that!(backlog.total_errors).contains_entry(ErrorType::Scan, scan_errors);
        assert_that!(backlog.total_errors).contains_entry(ErrorType::Ownership, ownership_errors);
        assert_that!(backlog.total_errors)
            .contains_entry(ErrorType::Permissions, permissions_errors);
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
    fn first_dir_fails() {
        assert_that!(crate::first_dir(Path::new("."))).is_none();
        assert_that!(crate::first_dir(Path::new("a"))).is_none();
    }

    #[rstest]
    fn empty_dir(test_data: TestData, mut backlog: Backlog) {
        let config = test_data.build_config(None, None, None, None);
        backlog.scan(&config, test_data.now);
        check_backlog(&backlog, 0, 0, 0, 0, 0);
    }
    #[rstest]
    fn empty_dir_is_empty(test_data: TestData, mut backlog: Backlog) {
        let _ = test_data.get_subdir();
        let config = test_data.build_config(None, None, None, None);
        backlog.scan(&config, test_data.now);
        check_backlog(&backlog, 0, 0, 0, 0, 0);
    }
    #[rstest]
    fn no_extension_is_ignored(test_data: TestData, mut backlog: Backlog) {
        let subdir = test_data.get_subdir();
        add_file(&subdir, "readme");
        let config = test_data.build_config(None, None, None, None);
        backlog.scan(&config, test_data.now);
        check_backlog(&backlog, 0, 0, 0, 0, 0);
    }
    #[rstest]
    fn ignored_extension_is_ignored(test_data: TestData, mut backlog: Backlog) {
        let subdir = test_data.get_subdir();
        add_file(&subdir, "file.nef");
        add_file(&subdir, "file.xmp");
        let mut config = test_data.build_config(None, None, None, None);
        let exts = [OsString::from("xmp")];
        config.ignored_exts = &exts;
        backlog.scan(&config, test_data.now);
        check_backlog(&backlog, 1, 1, 0, 0, 0);
        check_has_dir_with(&backlog, SUBDIR, 1);
    }
    #[rstest]
    fn one_dir_one_file(test_data: TestData, mut backlog: Backlog) {
        let subdir = test_data.get_subdir();
        add_file(&subdir, "file.nef");
        let config = test_data.build_config(None, None, None, None);
        backlog.scan(&config, test_data.now);
        check_backlog(&backlog, 1, 1, 0, 0, 0);
        check_has_dir_with(&backlog, SUBDIR, 1);
    }
    #[rstest]
    fn one_dir_two_files(test_data: TestData, mut backlog: Backlog) {
        let subdir = test_data.get_subdir();
        add_file(&subdir, "dsc001.nef");
        add_file(&subdir, "dsc002.jpg");
        let config = test_data.build_config(None, None, None, None);
        backlog.scan(&config, test_data.now);
        check_backlog(&backlog, 1, 2, 0, 0, 0);
        check_has_dir_with(&backlog, SUBDIR, 2);
    }
    #[rstest]
    fn file_in_root_dir(test_data: TestData, mut backlog: Backlog) {
        add_file(test_data.temp_dir.path(), "file.nef");
        let config = test_data.build_config(None, None, None, None);
        backlog.scan(&config, test_data.now);
        check_backlog(&backlog, 1, 1, 0, 0, 0);
        check_has_dir_with(&backlog, ROOT_FILE_DIR, 1);
    }

    #[rstest]
    fn no_such_dir(test_data: TestData, mut backlog: Backlog) {
        let _subdir = test_data.get_subdir();
        let mut missing_dir = test_data.temp_dir.path().to_path_buf();
        missing_dir.push("no-such_dir");
        let mut config = test_data.build_config(None, None, None, None);
        config.root_path = &missing_dir;
        backlog.scan(&config, test_data.now);
        check_backlog(&backlog, 0, 0, 1, 0, 0);
    }

    enum FailMode {
        NoCheck,
        Good,
        Bad,
    }

    #[rstest]
    fn test_ownership(
        test_data: TestData,
        mut backlog: Backlog,
        #[values(FailMode::NoCheck, FailMode::Good, FailMode::Bad)] user_mode: FailMode,
        #[values(FailMode::NoCheck, FailMode::Good, FailMode::Bad)] group_mode: FailMode,
    ) {
        let _ = env_logger::builder().is_test(true).try_init();

        let fname = add_file(test_data.temp_dir.path(), "file.nef");
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
        let config = test_data.build_config(user_check, group_check, None, None);
        backlog.scan(&config, test_data.now);
        let expected_errors = match (user_mode, group_mode) {
            // The expected errors is two, because both the top level directory
            // and the file should fail the check.
            (FailMode::Bad, _) | (_, FailMode::Bad) => 2,
            _ => 0,
        };
        check_backlog(&backlog, 1, 1, 0, expected_errors, 0);
        check_has_dir_with(&backlog, ROOT_FILE_DIR, 1);
    }

    #[rstest]
    fn test_permissions(
        test_data: TestData,
        mut backlog: Backlog,
        // This is just the file permissions, not the directory. Directory
        // always gets execute on user.
        #[values(0o664, 0o644, 0o660, 0o640, 0o600)] perm: u32,
        #[values(FailMode::NoCheck, FailMode::Good, FailMode::Bad)] raw_file_mode: FailMode,
        #[values(FailMode::NoCheck, FailMode::Good, FailMode::Bad)] dir_mode: FailMode,
    ) {
        let _ = env_logger::builder().is_test(true).try_init();

        let subdir = test_data.get_subdir();
        let fname = add_file(&subdir, "file.nef");
        fn dir_mode_from_file(perm: u32) -> u32 {
            perm | 0o100
        }
        fn maybe_dir_mode(is_dir: bool, perm: u32) -> u32 {
            if is_dir {
                dir_mode_from_file(perm)
            } else {
                perm
            }
        }
        fn generate_check(mode: &FailMode, perm: u32, is_dir: bool) -> Option<u32> {
            let bad_perm = if perm == 0o600 { 0o640 } else { 0o600 };
            match mode {
                FailMode::NoCheck => None,
                FailMode::Good => Some(maybe_dir_mode(is_dir, perm)),
                FailMode::Bad => Some(maybe_dir_mode(is_dir, bad_perm)),
            }
        }
        let dir_check = generate_check(&dir_mode, perm, true);
        let file_check = generate_check(&raw_file_mode, perm, false);
        // Set the actual permissions on the file first, then the two directories.
        std::fs::set_permissions(fname, std::fs::Permissions::from_mode(perm)).unwrap();
        let dir_perms = std::fs::Permissions::from_mode(dir_mode_from_file(perm));
        std::fs::set_permissions(&test_data.temp_dir, dir_perms.clone()).unwrap();
        std::fs::set_permissions(&subdir, dir_perms).unwrap();
        // Now actually do the permissions check.
        let config = test_data.build_config(None, None, dir_check, file_check);
        backlog.scan(&config, test_data.now);
        let file_errors = match raw_file_mode {
            FailMode::Bad => 1,
            _ => 0,
        };
        let dir_errors = match dir_mode {
            FailMode::Bad => 2,
            _ => 0,
        };
        let expected_errors = file_errors + dir_errors;
        check_backlog(&backlog, 1, 1, 0, 0, expected_errors);
        check_has_dir_with(&backlog, subdir.file_name().unwrap().to_str().unwrap(), 1);
    }

    #[rstest]
    fn ignored_files_are_ignored(test_data: TestData, mut backlog: Backlog) {
        let _ = env_logger::builder().is_test(true).try_init();

        let subdir = test_data.get_subdir();
        // File with good extension.
        let nef = add_file(&subdir, "file.nef");
        // File with ignored extension.
        let _readme = add_file(&subdir, "readme.md");
        // File with no extension.
        let _checksums = add_file(&subdir, "SHA1SUMS");
        std::fs::set_permissions(&nef, std::fs::Permissions::from_mode(0o600)).unwrap();
        let m = std::fs::metadata(&nef).expect("Can't stat just created file!");
        let wrong_mode = 0o644;
        let wrong_uid = m.uid() + 1;
        let wrong_gid = m.gid() + 1;

        let mut config =
            test_data.build_config(Some(wrong_uid), Some(wrong_gid), None, Some(wrong_mode));
        let exts = [OsString::from("md")];
        config.ignored_exts = &exts;
        backlog.scan(&config, test_data.now);
        // The top-level directory and sub-directory have wrong ownership (the
        // assumption here is that both temp directories and temp files have the
        // same ownership, which is generally correct), and the real file as
        // well, but the two extra files are ignored.
        let expected_errors = 3;
        check_backlog(&backlog, 1, 1, 0, expected_errors, 1);
        check_has_dir_with(&backlog, subdir.file_name().unwrap().to_str().unwrap(), 1);
    }

    #[rstest]
    fn test_scan_errors(test_data: TestData, mut backlog: Backlog) {
        let temp_dir = &test_data.temp_dir;
        let _f1 = add_file(temp_dir.path(), "file1.nef");
        // File f2 is ignored (for statistics), but current semantics is that
        // all items should be scanable.
        let _f2 = add_file(temp_dir.path(), "file1.xmp");
        let _f3 = add_file(temp_dir.path(), "file2.nef");
        // Sigh, Rust. Do the dance of adding a finalizer that resets the
        // permissions to something that allows the directory and its files to
        // be deleted.
        struct Cleanup<'a> {
            path: &'a Path,
        }
        impl<'a> Drop for Cleanup<'a> {
            fn drop(&mut self) {
                std::fs::set_permissions(self.path, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
        }
        let _cleanup = Cleanup {
            path: temp_dir.path(),
        };
        std::fs::set_permissions(temp_dir, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = test_data.build_config(None, None, None, None);
        let exts = [OsString::from("xmp")];
        config.ignored_exts = &exts;
        backlog.scan(&config, test_data.now);
        std::fs::set_permissions(temp_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        check_backlog(&backlog, 0, 0, 3, 0, 0);
    }
}
