use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::time::SystemTime;

use speculoos::prelude::*;
use tempfile::tempdir;

use photo_backlog_exporter::{Backlog, Config, ErrorType};

#[test]
fn test_ownership_logs() {
    testing_logger::setup();
    let temp_dir = tempdir().unwrap();
    let mut fname = PathBuf::from(temp_dir.path());
    fname.push("file1.nef");
    std::fs::write(&fname, b"").expect("Can't create file");
    std::fs::set_permissions(&fname, std::fs::Permissions::from_mode(0o600))
        .expect("Can't set permissions");
    let m = std::fs::metadata(&fname).expect("Can't stat just created file!");
    let config = Config {
        root_path: temp_dir.path(),
        ignored_exts: &[],
        raw_exts: &[],
        editable_exts: &[],
        owner: Some(m.uid() + 1),
        group: None,
        raw_file_mode: Some(0o644),
        editable_file_mode: None,
        dir_mode: None,
    };
    let mut backlog = Backlog::new([].into_iter());
    let now = SystemTime::now();
    backlog.scan(&config, now);
    assert_that!(backlog.folders).has_length(1);
    assert_that!(backlog.total_files).is_equal_to(1);
    assert_that!(backlog.total_errors).contains_entry(ErrorType::Scan, 0);
    assert_that!(backlog.total_errors).contains_entry(ErrorType::Ownership, 2);
    assert_that!(backlog.total_errors).contains_entry(ErrorType::Permissions, 1);
    testing_logger::validate(|captured_logs| {
        let v: Vec<String> = captured_logs.iter().map(|e| e.body.clone()).collect();
        assert_that!(v).matching_contains(|val| val.contains("has wrong owner:group"));
        assert_that!(v).matching_contains(|val| val.contains("has wrong mode"));
    });
}
