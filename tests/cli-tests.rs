use assert_cmd::cargo::CommandCargoExt;
use assert_cmd::prelude::*;
use predicates::prelude::*;
use result::ResultAssertions;
use rstest::rstest;
use speculoos::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;
use tokio::net::TcpListener;

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
    // Setup the test environment. Note that this tests/assumes what the
    // raw/editable file extensions are.
    let temp_dir = tempdir().unwrap();
    let mut fname = PathBuf::from(temp_dir.path());
    fname.push("file1.nef");
    std::fs::write(&fname, b"").expect("Can't create file");
    std::fs::set_permissions(&fname, std::fs::Permissions::from_mode(0o600))
        .expect("Can't set permissions");
    let m = std::fs::metadata(&fname).expect("Can't stat just created file!");

    std::fs::write(temp_dir.path().join("file2.zip"), b"").expect("Can't create file");
    std::fs::write(temp_dir.path().join("file3.jpg"), b"").expect("Can't create file");

    let mut cmd = Command::cargo_bin("oneshot").unwrap();
    cmd.args(["--path", temp_dir.path().to_str().unwrap()])
        .args(["--owner", &format!("{}", m.uid() + 1)])
        .args(["--raw-file-mode", "644"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(
            "photo_backlog_counts{kind=\"photos\"} 2",
        ))
        .stdout(predicate::str::contains(
            "photo_backlog_errors{kind=\"ownership\"} 3",
        ))
        .stdout(predicate::str::contains(
            "photo_backlog_errors{kind=\"permissions\"} 1",
        ))
        .stdout(predicate::str::contains(
            "photo_backlog_errors{kind=\"scan\"} 0",
        ))
        .stdout(predicate::str::contains(
            "photo_backlog_errors{kind=\"unknown\"} 1",
        ))
        .stdout(predicate::str::contains(
            "photo_backlog_folder_sizes{path=\".\"} 2",
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

#[rstest]
fn test_daemon_systemd_logging(#[values("oneshot", "photo-backlog-exporter")] cmd_name: &str) {
    let mut cmd = Command::cargo_bin(cmd_name).unwrap();
    cmd.env("RUST_LOG_SYSTEMD", "yes");
    cmd.env("RUST_LOG", "debug");
    cmd.arg("--help");

    cmd.assert().success().stderr(predicate::str::contains(
        "<7>photo_backlog_exporter::cli: Help requested, showing usage and exiting.",
    ));
}

#[tokio::test]
async fn test_daemon_fail_port() {
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);
    let listener = TcpListener::bind(&socket).await;
    let ok_listener = assert_that!(listener).is_ok().subject;
    let local_addr = ok_listener.local_addr();
    let addr_with_port = assert_that!(local_addr).is_ok().subject;

    let mut cmd = Command::cargo_bin("photo-backlog-exporter").unwrap();
    cmd.env("RUST_LOG_SYSTEMD", "yes");
    cmd.env("RUST_LOG", "debug");
    cmd.args(["--port", &addr_with_port.port().to_string(), "--path", "."]);

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Failed to bind"));
}

#[test]
fn test_oneshot_systemd_logging() {
    let temp_dir = tempdir().unwrap();
    let mut fname = PathBuf::from(temp_dir.path());
    fname.push("fifo.nef");

    std::fs::write(&fname, b"").expect("Can't create file");

    let mut cmd = Command::cargo_bin("oneshot").unwrap();
    cmd.current_dir(temp_dir.path()).args(["--path", "."]);
    cmd.env("RUST_LOG_SYSTEMD", "yes");
    cmd.env("RUST_LOG", "debug");

    cmd.assert()
        .success()
        .stderr(predicate::str::contains(
            "<4>photo_backlog_exporter: Can\'t determine parent path for ./fifo.nef",
        ))
        .stderr(predicate::str::contains(
            "<6>photo_backlog_exporter::cli: Starting up with the following options",
        ));
}
