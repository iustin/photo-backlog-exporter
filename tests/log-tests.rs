use std::ffi::OsString;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use speculoos::prelude::*;
use tempfile::tempdir;

use photo_backlog_exporter::{Backlog, Config, ErrorType};

fn create_file(path: &Path, name: &str, mode: u32) -> PathBuf {
    let mut fname = path.to_path_buf();
    fname.push(name);
    std::fs::write(&fname, b"").expect("Can't create file");
    std::fs::set_permissions(&fname, std::fs::Permissions::from_mode(mode))
        .expect("Can't set permissions");
    fname
}

#[test]
fn test_ownership_logs() {
    testing_logger::setup();
    let temp_dir = tempdir().unwrap();
    let fname = create_file(temp_dir.path(), "file1.nef", 0o600);
    let m = std::fs::metadata(&fname).expect("Can't stat just created file!");
    let _ = create_file(temp_dir.path(), "file2.jpg", 0o644);
    let _ = create_file(temp_dir.path(), "file3.jpg", 0o664);
    let editable_owners = vec![m.uid() + 1];
    let config = Config {
        root_path: temp_dir.path(),
        ignored_exts: &[],
        raw_exts: &[OsString::from("nef")],
        editable_exts: &[OsString::from("jpg")],
        raw_owner: Some(m.uid() + 1),
        editable_owners: &editable_owners,
        group: None,
        raw_file_mode: Some(0o644),
        editable_file_mode: Some(0o664),
        dir_mode: None,
    };
    let mut backlog = Backlog::new([].into_iter());
    let now = SystemTime::now();
    backlog.scan(&config, now);
    assert_that!(backlog.folders).has_length(1);
    assert_that!(backlog.total_files).is_equal_to(3);
    assert_that!(backlog.total_errors).contains_entry(ErrorType::Scan, 0);
    assert_that!(backlog.total_errors).contains_entry(ErrorType::Ownership, 4);
    assert_that!(backlog.total_errors).contains_entry(ErrorType::Permissions, 2);
    testing_logger::validate(|captured_logs| {
        let v: Vec<String> = captured_logs.iter().map(|e| e.body.clone()).collect();
        assert_that!(v).matching_contains(|val| val.contains("has wrong owner:group"));
        assert_that!(v).matching_contains(|val| val.contains("has wrong mode"));
    });
}
