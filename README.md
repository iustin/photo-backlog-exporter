# photo-backlog-exporter

This is a very simple, file-counter and `mtime`-stats aggregator,
Linux program designed to just tell me how many pictures I still have
to process. It's an (expanded) port of a previous Python version,
mostly as a learning exercise, and is designed to work together with
the file structure as used by github.com/iustin/corydalis.

[![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/iustin/photo-backlog-exporter/rust.yml?branch=main)](https://github.com/iustin/pyxattr/actions/workflows/ci.yml)
[![Codecov](https://img.shields.io/codecov/c/github/iustin/photo-backlog-exporter)](https://codecov.io/gh/iustin/photo-backlog-exporter)

## What does it do?

The program is intended to export metrics for Prometheus based on the
count, age and distribution of files in a directory.

The directory structure is based just on the first-level directories; anything
deeper will be aggregated to the first-level directory. So for example
the output of this `tree` command:

```shell
$ tree
.
├── 2023-11-01 Long trip
│   ├── Day 1
│   │   └── b.jpg
│   └── Day 2
│       └── c.jpg
└── 2023-11-19 Some pictures
    ├── a.jpg
    ├── b.nef
    └── b.xmp

5 directories, 5 files
```

Will be exported as:

- 4 total files pending; the `xmp` file is ignored by default, see in
  the usage section;
- 2 directories (`2023-11-01 Long trip` and `2023-11-19 Some
  pictures`);
- for each directory, an aggregated "age" will be computed (sum of
  ages, relative to the current time);
- and an overall histogram with pending file ages will be exported;

### Error types

The program currently exports counters for four different error classes:

- scan errors: some directories cannot be scanned due to permissions;
- ownership errors: known file type user or group doesn't match the passed
  owner/group;
- permission errors: known file type or directory permissions doesn't match the
  configured ones;
- unknown errors: file extension is of unknown type; this is in order to make
  sure that all files are either categorized or ignored;

Suggestions for more (or less) checks are welcome.

## Motivation

I always lag behind photo processing. When I do process, I like to see
the counters going down, kind of like a gamification of the
processing. And with Prometheus and Grafana, the graphs are more or
less real-time indeed; the cost for scanning a ~5K directory tree are
around 15ms, which allows me to poll this on a normal Prometheus poll.

See the `examples/grafana-dashboard.json` file as an example
dashboard.

What is missing from the dashboard is the alert definition (Grafana
doesn't export both). I have a simple alert on the
`photo_backlog_errors` metrics, condition  '> 0 for 4 hours'. In the
future I plan to add permission/ownership checks as well.

## Installation

Run the usual `cargo build -r`. Copy the resulting binary (from
`target/release`) somewhere, then run it. You have two options:

- run the `photo-backlog-exporter` binary as a daemon
- run the `oneshot` binary as a text exporter, and save its
  output somewhere so that the common `node-exporter` can pick it up.

Since this doesn't need any special rights, just to be able to look at
directories and files, I run the daemon as a dynamic systemd user, just
with supplemental groups my photos group. You can find an example
systemd unit in `examples/systemd.server`. This is accompanied by the
"defaults" file `examples/prometheus-photo-backlog-exporter.defaults`
(see the service file, move the defaults where it is appropriate).

## Usage

Note that the binary expects at least the path to the root of the
"incoming" photos directory, i.e. a minimal invocation is:

```shell
photo-backlog-exporter -P /my/incoming/photos/directory
```

The full list of arguments is:

```shell
$ photo-backlog-exporter --help
Optional arguments:
  -h, --help                 print help message
  -p, --port PORT            port to listen on (default: 8813)
  -l, --listen LISTEN        address to listen on (default: ::)
  -P, --path PATH            path to root of incoming photo directory
  -i, --ignored-exts IGNORED-EXTS
                             ignored file extension (default: xmp,lua,DS_Store)
  -r, --raw-exts RAW-EXTS  raw or other files that should not be editable (default: nef,cr2,arw,orf,raf)
  -e, --editable-exts EDITABLE-EXTS
                           editable files, e.g. jpg, png, tif (default: jpg,jpeg,heic,heif,mov,mp4,avi,gpr,dng,png,tif,tiff,3gp,pano)
  -a, --age-buckets AGE-BUCKETS
                             Photos age histogram buckets, in weeks (default: 1,2,3,4,5,7,10,13,17,20,26,30,35,52,104)
  --raw_owner OWNER          Optional owner expected for raw files
  -g, --group GROUP          Optional group expected for all files
  -d, --dir-mode DIR-MODE    Optional numeric mode (permissions) expected for directories, e.g 750
  -R, --raw-file-mode RAW-FILE-MODE
                           Optional numeric mode (permissions) expected for non-editable files, e.g. 640
  -E, --editable-file-mode EDITABLE-FILE-MODE
                           Optional numeric mode (permissions) expected for editable files, e.g. 660
```

I hope they are self-explanatory. Well, maybe the `--ignored-exts`:
these are extensions for which files should be completely
ignored. For example, the `xmp` extension should normally be ignored
because Lightroom can store metadata (if so configured) outside of the
catalog and in `xmp` files for each proprietary RAW format. (Yes, this
also means that the mtime-counting doesn't work well for `jpeg` files,
for example. Sorry - if you have ideas, file a bug!)

The file permissions are split in two categories:

- raw files, which in general should not be edited, at least not for proprietary
  RAW files, like Nikon's NEF and Canon's CRW, as opposed to DNG which is
  editable;
- and editable files, which is everything else - DNG, TIFF, JPEG, etc.

The difference arises that ideally, the processing tool shouldn't be given write
permissions to raw files. If you do use Lightroom to store edits into RAW files
(in my opinion, not a good idea), then don't pass `-R` and override the `-r`
options.

Note that the binary uses the `env_logger` rust package, and thus
logging can be configured via the usual `RUST_LOG=info` and similar
environment variables. This is why the example systemd service file
mentioned uses an env file, to allow easy passing of both arguments
but also (in this case) `RUST_LOG`.

## Rust

I'm not a Rust programmer, just having fun learning a new language, so
please do point out mistakes, obvious or not. Thanks!
