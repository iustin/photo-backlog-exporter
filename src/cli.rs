use std::ffi::OsString;

use std::net::IpAddr;
use std::path::PathBuf;

use crate::{parse_exts, parse_weeks};

use gumdrop::Options;

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

    #[options(help = "Owner expected for all files")]
    pub owner: Option<u32>,

    #[options(help = "Group expected for all files")]
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
}