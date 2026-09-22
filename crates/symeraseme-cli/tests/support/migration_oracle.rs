//! CLI-024: replay recorded migration inputs, output bytes and filesystem state.

use super::{Cleanup, binary, fold_root, hex_digest, run, unique_root};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

type Manifest = BTreeMap<String, Value>;

fn input_path(root: &Path, relative: &str) -> PathBuf {
    assert!(
        !relative.is_empty()
            && Path::new(relative)
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "fixture input must stay beneath its runtime root: {relative}"
    );
    root.join(relative)
}

pub(super) fn prepare(case: &Value, root: &Path) -> Manifest {
    let id = case["id"].as_str().expect("migration case id");
    fs::create_dir_all(root.join("cli").join(id).join("home")).expect("isolated migration home");
    for directory in case["input_directories"]
        .as_array()
        .expect("input directories")
    {
        fs::create_dir_all(input_path(root, directory.as_str().expect("directory")))
            .expect("create recorded migration input directory");
    }
    for (relative, content) in case["input_files"].as_object().expect("input files") {
        let path = input_path(root, relative);
        fs::create_dir_all(path.parent().expect("input parent")).expect("input parent directory");
        fs::write(
            path,
            content.as_str().expect("UTF-8 input fixture").as_bytes(),
        )
        .expect("write recorded migration input");
    }
    manifest(&root.join("cli").join(id), root)
}

pub(super) fn assert_unchanged(case: &Value, root: &Path, before: &Manifest) {
    let id = case["id"].as_str().expect("migration case id");
    assert_eq!(
        &manifest(&root.join("cli").join(id), root),
        before,
        "{id}: dry-run changed its filesystem"
    );
}

fn manifest(directory: &Path, runtime_root: &Path) -> Manifest {
    let mut entries = BTreeMap::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(parent) = pending.pop() {
        for entry in fs::read_dir(parent).expect("read migration output directory") {
            let path = entry.expect("migration output entry").path();
            let metadata = fs::symlink_metadata(&path).expect("migration output metadata");
            let relative = path
                .strip_prefix(directory)
                .expect("entry beneath root")
                .to_str()
                .expect("UTF-8 fixture path")
                .replace('\\', "/");
            let mut record = serde_json::json!({"path": relative});
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                record["mode"] = format!("0o{:o}", metadata.permissions().mode() & 0o777).into();
            }
            if metadata.is_dir() {
                record["type"] = "directory".into();
                // Directory allocation sizes vary by filesystem, unlike file
                // payload sizes. Names, types and Unix modes remain asserted.
                pending.push(path);
            } else {
                assert!(
                    metadata.is_file(),
                    "unexpected non-regular migration output: {path:?}"
                );
                let data = fold_root(
                    &fs::read(&path).expect("migration output bytes"),
                    runtime_root,
                );
                record["type"] = "file".into();
                record["size_bytes"] = data.len().into();
                record["sha256"] = hex_digest(&Sha256::digest(&data)).into();
            }
            entries.insert(relative, record);
        }
    }
    entries
}

#[test]
fn migration_fixture_paths_reject_escape() {
    let root = unique_root();
    for relative in ["", "/outside", "../outside", "cli/../../outside"] {
        assert!(std::panic::catch_unwind(|| input_path(&root, relative)).is_err());
    }
    assert_eq!(
        input_path(&root, "cli/source/config.toml"),
        root.join("cli/source/config.toml")
    );
}

#[test]
fn migration_filesystem_matches_frozen_go_backup_and_state() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../rust-tests/parity/cases/filesystem/manifests.json"
    )))
    .expect("filesystem oracle JSON");
    assert_eq!(fixture["commit"], "4e582f28");
    let cases = fixture["cases"].as_array().expect("filesystem cases");
    let matches: Vec<_> = cases
        .iter()
        .filter(|case| case["id"] == "migration")
        .collect();
    assert_eq!(matches.len(), 1, "exactly one frozen migration mutation");
    let case = matches[0];
    let root = unique_root();
    fs::create_dir_all(&root).expect("isolated runtime root");
    let _cleanup = Cleanup(root.clone());
    let case_root = root.join("filesystem/migration");
    let source = case_root.join("legacy-source");
    let home = root.join("home");
    let cwd = case_root.join("cwd");
    let capture = root.join("capture");
    for directory in [&source, &home, &cwd, &capture, &case_root.join("home")] {
        fs::create_dir_all(directory).expect("isolated fixture directory");
    }
    let config = source.join("config.toml");
    fs::write(&config, b"data_dir = 'legacy'\n").expect("recorded legacy config input");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(config, fs::Permissions::from_mode(0o644)).expect("oracle input mode");
    }
    let source_before = manifest(&source, &root);
    let argv: Vec<_> = case["operation"]
        .as_array()
        .expect("operation argv")
        .iter()
        .map(|arg| arg.as_str().expect("string argument"))
        .collect();
    assert!(binary().is_file(), "the native Rust CLI artifact exists");
    let output = run(&argv, &home, &cwd, &capture, 0);
    assert_eq!(
        output.status.code(),
        Some(0),
        "migration failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(case["process"]["exit_code"], 0);
    for (stream, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
        let folded = fold_root(bytes, &root);
        assert_eq!(
            folded.len() as u64,
            case["process"][format!("{stream}_bytes")]
                .as_u64()
                .expect("stream length"),
            "migration {stream} length"
        );
        assert_eq!(
            hex_digest(&Sha256::digest(&folded)),
            case["process"][format!("{stream}_sha256")]
                .as_str()
                .expect("stream digest"),
            "migration {stream} digest"
        );
    }
    for (name, recorded_path) in case["manifest_roots"].as_object().expect("manifest roots") {
        let relative = recorded_path
            .as_str()
            .expect("recorded root")
            .strip_prefix("<ORACLE_ROOT>/")
            .expect("isolated recorded root");
        let directory = input_path(&root, relative);
        let expected: Manifest = case["manifests"][name]
            .as_array()
            .expect("root manifest")
            .iter()
            .map(|entry| {
                let mut entry = entry.clone();
                if entry["type"] == "directory" {
                    entry.as_object_mut().expect("record").remove("size_bytes");
                }
                #[cfg(not(unix))]
                entry.as_object_mut().expect("record").remove("mode");
                (
                    entry["path"].as_str().expect("manifest path").to_owned(),
                    entry,
                )
            })
            .collect();
        assert_eq!(
            manifest(&directory, &root),
            expected,
            "migration {name} manifest"
        );
    }
    assert_eq!(
        manifest(&source, &root),
        source_before,
        "legacy source retained unchanged"
    );
}
