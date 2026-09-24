//! Native Linux regression for byte-preserving whole-source backups.
//! macOS APFS rejects these names (EILSEQ); a host pass is not Linux evidence.
#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::ffi::OsStringExt;
use symeraseme_engine::migration::{self, Options};

#[test]
fn backup_preserves_distinct_non_utf8_names() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("config.toml"), b"legacy fixture").unwrap();
    let names = [
        std::ffi::OsString::from_vec(b"raw-\xff".to_vec()),
        std::ffi::OsString::from("raw-\u{fffd}"),
    ];
    for (index, name) in names.iter().enumerate() {
        let directory = source.join(name);
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join(name), format!("distinct content {index}")).unwrap();
    }
    let options = Options {
        source_root: source.to_str().unwrap().to_owned(),
        destination_root: root.path().join("destination").to_str().unwrap().to_owned(),
        platform: "cron".to_owned(),
        ..Default::default()
    };
    let (report, error) = migration::run(&options);
    assert!(error.is_none(), "{error:?}");
    let report = report.unwrap();
    assert!(report.complete);
    let backup = std::path::Path::new(&report.backup_dir).join("source");
    for (index, name) in names.iter().enumerate() {
        let expected = format!("distinct content {index}").into_bytes();
        assert_eq!(fs::read(backup.join(name).join(name)).unwrap(), expected);
        assert_eq!(fs::read(source.join(name).join(name)).unwrap(), expected);
    }
}
