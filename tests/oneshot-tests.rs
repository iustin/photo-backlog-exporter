use assert_cmd::cargo::CommandCargoExt;
use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

#[test]

fn test_oneshot_missing_help() {
    let mut cmd = Command::cargo_bin("oneshot").unwrap();
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stderr(predicate::str::contains("Optional arguments:"))
        .stderr(predicate::str::contains("--age-buckets AGE-BUCKETS"));
}

#[test]
fn test_oneshot_missing_path() {
    let mut cmd = Command::cargo_bin("oneshot").unwrap();

    // Add test cases here
    // For example:
    // cmd.arg("--input").arg("input_file.txt");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("missing required option `--path`"));
}

#[test]
fn test_oneshot_permissions_check() {
    // Set test
    let temp_dir = tempdir().unwrap();
    let mut fname = PathBuf::from(temp_dir.path());
    fname.push("file1.nef");
    std::fs::write(&fname, b"").expect("Can't create file");
    std::fs::set_permissions(&fname, std::fs::Permissions::from_mode(0o600))
        .expect("Can't set permissions");
    let m = std::fs::metadata(&fname).expect("Can't stat just created file!");

    let mut cmd = Command::cargo_bin("oneshot").unwrap();
    cmd.args(["--path", temp_dir.path().to_str().unwrap()])
        .args(["--owner", &format!("{}", m.uid() + 1)])
        .args(["--file-mode", "644"]);

    // Add test cases here
    // For example:
    // cmd.arg("--input").arg("input_file.txt");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(
            "photo_backlog_counts{kind=\"photos\"} 1",
        ))
        .stdout(predicate::str::contains(
            "photo_backlog_errors{kind=\"ownership\"} 2",
        ))
        .stdout(predicate::str::contains(
            "photo_backlog_errors{kind=\"permissions\"} 1",
        ));
}
