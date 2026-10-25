# Agent instructions

photo-backlog-exporter is a very simple Rust application to export file
statistics via the Prometheus protocol.

The code-base is simple enough that it can all be read whenever features are
being added.

## Layout

- `src/lib.rs` — scan and aggregate the photo tree
- `src/cli.rs` — arguments
- `src/prometheus.rs` — metric names and text encoding
- `src/daemon.rs` — HTTP server
- `src/main.rs` — long-running daemon binary (`photo-backlog-exporter`)
- `src/bin/oneshot.rs` — text exporter; `default-run` in `Cargo.toml`, so `cargo
run` builds this one
- `tests/` — integration tests

Argument and output changes must keep both binaries in sync. They share
`cli::init_binary`.

## Invariants

Do not break these. They are easy to get wrong, and the README depends on them.

- Metric values must stay stable when the tree is unchanged. Do not use the
  current time (with the exception of `photo_backlog_processing_time_seconds`,
  which by its nature is variable). Folder age is the oldest file's Unix-epoch
  timestamp. The age histogram is relative to the newest file in the backlog.
- Aggregation is first-level only. Anything deeper rolls into that first-level
  directory, modulo the container-regex.
- `--container-regex` is a full match of the top-level directory name. The
  default empty string is compiled as `^$` and matches nothing. A matching
  directory is organization only: each child is exported as `container/child`,
  and a child is never itself a container. A counted file sitting directly in
  the container uses the container name.
- Ignored extensions are not counted. The default list is the `ignored_exts`
  default in `src/cli.rs`.

If new metrics are added, check if the grafana dashboard (under the examples
directory) needs adjustment.

## Code style

Prefer functional style over imperative style (e.g. matching on list contents
rather than indexing into lists). Do not add unsafe code unless absolutely
needed and confirmed by the user.

## Tests

- Run tests with `make all`
- That target runs `cargo llvm-cov`, writes `lcov.info`, and needs
  `cargo-llvm-cov` installed
- CI's test job is plain `cargo test`. Coverage in CI is a separate job

Always add tests when adding new features, or even when changing existing code.
Follow the existing test style (`rstest`, `speculoos`, `assert_cmd`).

Keep Makefile targets and `.github/workflows/rust.yml` logically in sync: a new
target such as `make lint` must also run in CI. The commands need not match.

## Documentation

User-facing documentation is `README.md`. Keep it up-to-date with any user-visible change, including:

- CLI help text (it is pasted into the README)
- metric names and labels
- `examples/` (Grafana dashboard, systemd unit, defaults file)
