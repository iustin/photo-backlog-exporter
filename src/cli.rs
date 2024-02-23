use std::ffi::OsString;
use std::net::IpAddr;
use std::num::ParseFloatError;
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

pub fn collector_from_args(opts: CliOptions) -> crate::prometheus::PhotoBacklogCollector {
    crate::prometheus::PhotoBacklogCollector {
        scan_path: opts.path,
        ignored_exts: opts.ignored_exts,
        age_buckets: opts.age_buckets,
        owner: opts.owner,
        group: opts.group,
    }
}

#[cfg(test)]
mod tests {
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
}
