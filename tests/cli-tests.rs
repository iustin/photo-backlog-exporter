use assert_cmd::cargo::CommandCargoExt;
use assert_cmd::prelude::*;
use predicates::prelude::*;
use rstest::rstest;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

#[rstest]
fn test_help(#[values("oneshot", "photo-backlog-exporter")] cmd_name: &str) {
    let mut cmd = Command::cargo_bin(cmd_name).unwrap();
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stderr(predicate::str::contains("Optional arguments:"))
        .stderr(predicate::str::contains("--age-buckets AGE-BUCKETS"));
}

#[rstest]
fn test_missing_path(#[values("oneshot", "photo-backlog-exporter")] cmd_name: &str) {
    let mut cmd = Command::cargo_bin(cmd_name).unwrap();

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("missing required option `--path`"));
}

#[test]
fn test_permissions_check() {
    // Setup the test environment.
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

#[test]
fn test_relative_dir() {
    let temp_dir = tempdir().unwrap();
    let mut fname = PathBuf::from(temp_dir.path());
    fname.push("file1.nef");
    std::fs::write(&fname, b"").expect("Can't create file");

    let mut cmd = Command::cargo_bin("oneshot").unwrap();

    cmd.current_dir(temp_dir.path()).args(["--path", "."]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(
            "photo_backlog_counts{kind=\"photos\"} 1",
        ))
        .stdout(predicate::str::contains(
            "photo_backlog_folder_sizes{path=\".\"} 1",
        ));
}

#[test]
fn test_ignores_fifo() {
    let temp_dir = tempdir().unwrap();
    let mut fname = PathBuf::from(temp_dir.path());
    fname.push("fifo.nef");
    Command::new("mkfifo").arg(&fname).spawn().unwrap();

    let mut cmd = Command::cargo_bin("oneshot").unwrap();

    cmd.current_dir(temp_dir.path()).args(["--path", "."]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(
            "photo_backlog_counts{kind=\"photos\"} 0",
        ))
        .stdout(predicate::str::contains(
            "photo_backlog_counts{kind=\"folders\"} 0",
        ));
}

#[test]
fn test_daemon_systemd_logging() {
    let mut cmd = Command::cargo_bin("photo-backlog-exporter").unwrap();
    cmd.env("RUST_LOG_SYSTEMD", "yes");
    cmd.env("RUST_LOG", "debug");
    cmd.arg("--help");

    cmd.assert().success().stderr(predicate::str::contains(
        "<7>photo_backlog_exporter: Help requested, showing usage and exiting.",
    ));
}
