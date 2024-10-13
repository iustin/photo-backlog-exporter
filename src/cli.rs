use std::ffi::OsString;
use std::io::Write;
use std::net::IpAddr;
use std::num::{ParseFloatError, ParseIntError};
use std::path::PathBuf;
use std::str::FromStr;

use gumdrop::Options;

const WEEK: f64 = 7.0 * 86400.0;

/// Simple conversion of a comma-separated string into a vector of OsString values.
/// Example:
/// ```
/// use std::ffi::OsString;
/// use photo_backlog_exporter::cli::parse_exts;
/// assert_eq!(parse_exts(""), Vec::<OsString>::from([]));
/// assert_eq!(parse_exts("a"), Vec::<OsString>::from([OsString::from("a")]));
/// assert_eq!(parse_exts("a,"), Vec::<OsString>::from([OsString::from("a")]));
/// assert_eq!(parse_exts("a,b"),
///   Vec::<OsString>::from([OsString::from("a"), OsString::from("b")]));
/// assert_eq!(parse_exts("a,,b"),
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
/// use photo_backlog_exporter::cli::parse_weeks;
/// assert_eq!(parse_weeks(""), Ok(Vec::<f64>::from([])));
/// assert_eq!(parse_weeks("0,1"), Ok(Vec::<f64>::from([0.0, 7.0*24.0*3600.0])));
/// assert!(parse_weeks("a").is_err());
/// ```
pub fn parse_weeks(s: &str) -> Result<Vec<f64>, ParseFloatError> {
    s.split(',')
        .filter(|c| !c.is_empty())
        .map(f64::from_str)
        .map(|r| r.map(|f| f * WEEK))
        .collect()
}

/// Parses the string as an octal number.
/// Example:
/// ```
/// use photo_backlog_exporter::cli::parse_octal_mode;
/// assert_eq!(parse_octal_mode("0"), Ok(0));
/// assert_eq!(parse_octal_mode("777"), Ok(0o777));
/// assert_eq!(parse_octal_mode("640"), Ok(0o640));
/// assert!(parse_octal_mode("a").is_err());
pub fn parse_octal_mode(mode_str: &str) -> Result<u32, ParseIntError> {
    u32::from_str_radix(mode_str, 8)
}

#[derive(Debug, Options)]
pub struct CliOptions {
    #[options(help = "print help message")]
    pub help: bool,

    #[options(help = "port to listen on", meta = "PORT", default = "8813")]
    pub port: u16,

    #[options(help = "address to listen on", default = "::")]
    pub listen: IpAddr,

    #[options(help = "path to root of incoming photo directory", required)]
    pub path: PathBuf,

    #[options(
        help = "ignored file extension",
        default = "xmp,lua,DS_Store",
        parse(from_str = "parse_exts"),
        no_multi
    )]
    pub ignored_exts: Vec<OsString>,

    #[options(
        help = "raw or other files that should not be editable",
        default = "nef,cr2,arw,orf,raf",
        parse(from_str = "parse_exts"),
        no_multi
    )]
    pub raw_exts: Vec<OsString>,

    #[options(
        help = "editable files, e.g. jpg, png, tif",
        default = "jpg,jpeg,heic,heif,mov,mp4,avi,gpr,dng,png,tif,tiff,3gp,pano",
        parse(from_str = "parse_exts"),
        no_multi
    )]
    pub editable_exts: Vec<OsString>,

    #[options(
        help = "Photos age histogram buckets, in weeks",
        default = "1,2,3,4,5,7,10,13,17,20,26,30,35,52,104",
        parse(try_from_str = "parse_weeks"),
        // Sigh, I'm doing my own parsing!
        no_multi
    )]
    pub age_buckets: Vec<f64>,

    #[options(help = "Optional owner expected for all files")]
    pub owner: Option<u32>,

    #[options(help = "Optional group expected for all files")]
    pub group: Option<u32>,

    #[options(
        help = "Optional numeric mode (permissions) expected for directories, e.g 750",
        parse(try_from_str = "parse_octal_mode")
    )]
    pub dir_mode: Option<u32>,

    #[options(
        help = "Optional numeric mode (permissions) expected for non-editable files, e.g. 640",
        parse(try_from_str = "parse_octal_mode"),
        short = "R"
    )]
    pub raw_file_mode: Option<u32>,

    #[options(
        help = "Optional numeric mode (permissions) expected for editable files, e.g. 660",
        parse(try_from_str = "parse_octal_mode"),
        short = "E"
    )]
    pub editable_file_mode: Option<u32>,
}

pub fn parse_args() -> Result<CliOptions, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    parse_args_from(args.as_slice())
}

pub fn parse_args_from<S>(args: &[S]) -> Result<CliOptions, String>
where
    S: AsRef<str>,
{
    let opts = CliOptions::parse_args_default(args).map_err(|e| e.to_string())?;
    if opts.help_requested() {
        return Ok(opts);
    }
    let path = &opts.path;
    if !path.is_dir() {
        return Err(format!(
            "Given path '{}' is not a directory :(",
            path.display()
        ));
    }
    Ok(opts)
}

pub fn init_binary() -> Result<Option<CliOptions>, String> {
    enable_logging();
    match parse_args() {
        Err(e) => Err(log_error(e)),
        Ok(opts) if opts.help_requested() => {
            log::debug!("Help requested, showing usage and exiting.");
            eprintln!("{}", CliOptions::usage());
            Ok(None)
        }
        Ok(opts) => {
            log::info!("Starting up with the following options: {:?}", opts);
            Ok(Some(opts))
        }
    }
}

pub fn log_error(e: String) -> String {
    log::error!("{}", e);
    e
}

pub fn collector_from_args(opts: CliOptions) -> crate::prometheus::PhotoBacklogCollector {
    crate::prometheus::PhotoBacklogCollector {
        scan_path: opts.path,
        ignored_exts: opts.ignored_exts,
        raw_exts: opts.raw_exts,
        editable_exts: opts.editable_exts,
        age_buckets: opts.age_buckets,
        owner: opts.owner,
        group: opts.group,
        dir_mode: opts.dir_mode,
        raw_file_mode: opts.raw_file_mode,
        editable_file_mode: opts.editable_file_mode,
    }
}

// Enables logging with support for systemd (if enabled).
// Adopted from https://github.com/rust-cli/env_logger/issues/157.
pub fn enable_logging() {
    match std::env::var("RUST_LOG_SYSTEMD") {
        Ok(s) if s == "yes" => env_logger::builder()
            .format(|buf, record| {
                writeln!(
                    buf,
                    "<{}>{}: {}",
                    match record.level() {
                        log::Level::Error => 3,
                        log::Level::Warn => 4,
                        log::Level::Info => 6,
                        log::Level::Debug => 7,
                        log::Level::Trace => 7,
                    },
                    record.target(),
                    record.args()
                )
            })
            .init(),
        _ => env_logger::init(),
    };
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use speculoos::prelude::*;
    use tempfile::tempdir;

    #[test]
    fn test_path_is_not_dir() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test1.nef");
        std::fs::File::create(&file_path).unwrap();
        let file_path_str = file_path.to_str().expect("convert file path to str");
        let opts = super::parse_args_from(&["--path", file_path_str]);
        assert_that!(opts).is_err().contains("not a directory");
    }

    #[test]
    fn test_cli_error() {
        let opts = super::parse_args_from(&["--no-such-arg"]);
        assert_that!(opts).is_err().contains("unrecognized option");
    }

    #[test]
    fn test_args() {
        let temp_dir = tempdir().unwrap();
        let temp_dir_str = temp_dir
            .path()
            .to_str()
            .expect("convert temp dir path to str");
        let opts = super::parse_args_from(&[
            "--path",
            temp_dir_str,
            "--dir-mode",
            "750",
            "--ignored-exts",
            "xmp,info",
        ]);
        let opts = opts.expect("parse args is successful");
        assert_that!(&opts.dir_mode).is_equal_to(Some(0o750));
        assert_that!(&opts.raw_file_mode).is_equal_to(None);
        let expected_exts = vec![OsString::from("xmp"), OsString::from("info")];
        assert_that!(opts.ignored_exts).is_equal_to(expected_exts);
    }
}
