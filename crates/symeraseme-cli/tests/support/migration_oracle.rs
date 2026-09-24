//! CLI-024: replay recorded migration inputs, output bytes and filesystem state.

#[cfg(not(windows))]
use super::run;
use super::{Cleanup, binary, fold_root, hex_digest, unique_root};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

type Manifest = BTreeMap<String, Value>;
#[cfg(windows)]
type RootManifests = BTreeMap<String, Manifest>;

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
    if !directory.try_exists().expect("manifest root existence") {
        return entries;
    }
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
#[cfg(not(windows))]
fn migration_filesystem_matches_frozen_go_backup_and_state() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../rust-tests/parity/cases/filesystem/manifests.json"
    )))
    .expect("filesystem oracle JSON");
    assert_eq!(fixture["commit"], "4e582f28");
    let cases: BTreeMap<_, _> = fixture["cases"]
        .as_array()
        .expect("filesystem cases")
        .iter()
        .filter_map(|case| {
            let id = case["id"].as_str().expect("case id");
            (id == "migration" || id.starts_with("migration-")).then_some((id, case))
        })
        .collect();
    let mut expected_ids = [
        "migration",
        "migration-resume",
        "migration-manual-secrets",
        "migration-copy-secrets-rejected",
        "migration-incomplete-backup",
    ];
    expected_ids.sort_unstable();
    assert_eq!(cases.keys().copied().collect::<Vec<_>>(), expected_ids);
    for (id, case) in cases {
        replay_filesystem(id, case);
    }
}

#[cfg(not(windows))]
fn replay_filesystem(id: &str, case: &Value) {
    let root = unique_root();
    fs::create_dir_all(&root).expect("isolated runtime root");
    let _cleanup = Cleanup(root.clone());
    let case_root = input_path(&root, &format!("filesystem/{id}"));
    let source = case_root.join("legacy-source");
    let home = root.join("home");
    let cwd = case_root.join("cwd");
    let capture = root.join("capture");
    for directory in [&source, &home, &cwd, &capture, &case_root.join("home")] {
        fs::create_dir_all(directory).expect("isolated fixture directory");
    }
    if id == "migration" {
        write_input(&source.join("config.toml"), "data_dir = 'legacy'\n", 0o644);
    } else {
        for directory in case["input_directories"]
            .as_array()
            .expect("input directories")
        {
            fs::create_dir_all(input_path(&root, directory.as_str().expect("directory")))
                .expect("input directory");
        }
        for (relative, input) in case["input_files"].as_object().expect("input files") {
            let mode = u32::try_from(input["mode"].as_u64().expect("input mode"))
                .expect("file mode fits u32");
            write_input(
                &input_path(&root, relative),
                input["content"].as_str().expect("input content"),
                mode,
            );
        }
    }
    let source_before = manifest(&source, &root);
    assert!(binary().is_file(), "the native Rust CLI artifact exists");
    let mut processes: Vec<&Value> = case.get("preparation").map_or_else(Vec::new, |value| {
        value
            .as_array()
            .expect("preparation processes")
            .iter()
            .collect()
    });
    processes.push(&case["process"]);
    for (index, process) in processes.into_iter().enumerate() {
        let argv: Vec<_> = process["argv"]
            .as_array()
            .expect("operation argv")
            .iter()
            .map(|arg| arg.as_str().expect("string argument"))
            .collect();
        let output = run(&argv, &home, &cwd, &capture, index);
        assert_eq!(
            output.status.code().map(i64::from),
            process["exit_code"].as_i64(),
            "{id}: process {index}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for (stream, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
            let folded = fold_root(bytes, &root);
            assert_eq!(
                folded.len() as u64,
                process[format!("{stream}_bytes")]
                    .as_u64()
                    .expect("stream length"),
                "{id}: {index} {stream} length"
            );
            assert_eq!(
                hex_digest(&Sha256::digest(&folded)),
                process[format!("{stream}_sha256")]
                    .as_str()
                    .expect("stream digest"),
                "{id}: {index} {stream} digest: {}",
                String::from_utf8_lossy(&folded)
            );
        }
    }
    for (name, recorded_path) in case["manifest_roots"].as_object().expect("manifest roots") {
        let relative = recorded_path
            .as_str()
            .expect("recorded root")
            .strip_prefix("<ORACLE_ROOT>/")
            .expect("isolated recorded root");
        let directory = input_path(&root, relative);
        if let Some(exists) = case.get("root_exists") {
            assert_eq!(
                directory.try_exists().expect("root existence"),
                exists[name].as_bool().expect("recorded existence"),
                "{id}: {name} existence"
            );
        }
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
            "{id}: {name} manifest"
        );
    }
    assert_eq!(
        manifest(&source, &root),
        source_before,
        "{id}: legacy source retained unchanged"
    );
}

#[test]
#[cfg(not(windows))]
fn migration_filesystem_rejects_corrupted_output_and_backup() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../rust-tests/parity/cases/filesystem/manifests.json"
    )))
    .expect("filesystem oracle JSON");
    let case = fixture["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"] == "migration")
        .expect("migration case");
    // Establish a passing positive before testing the real comparator's
    // rejection paths; a broken engine cannot make these controls pass.
    replay_filesystem("migration", case);
    let mut output = case.clone();
    output["process"]["stdout_sha256"] = "0".repeat(64).into();
    let mut backup = case.clone();
    let marker = backup["manifests"]["backup"]
        .as_array_mut()
        .expect("backup")
        .iter_mut()
        .find(|entry| entry["path"] == ".complete.json")
        .expect("completion marker");
    marker["sha256"] = "0".repeat(64).into();
    let mut absent = case.clone();
    absent["root_exists"] =
        serde_json::json!({"source": true, "destination": false, "backup": true});
    for (mutated, reason) in [
        (output, "migration: 0 stdout digest"),
        (backup, "migration: backup manifest"),
        (absent, "migration: destination existence"),
    ] {
        let rejection = std::panic::catch_unwind(|| replay_filesystem("migration", &mutated))
            .expect_err("corrupt oracle must be rejected");
        let message = rejection
            .downcast_ref::<String>()
            .expect("assertion diagnostic");
        assert!(message.contains(reason), "unexpected rejection: {message}");
    }
}

#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeProcess {
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(windows)]
fn native_process(output: &super::ProcessOutput, root: &Path) -> NativeProcess {
    NativeProcess {
        status: output.status.code(),
        stdout: fold_root(&output.stdout, root),
        stderr: fold_root(&output.stderr, root),
    }
}

#[cfg(windows)]
fn assert_native_processes_equal(id: &str, go: &NativeProcess, rust: &NativeProcess) {
    assert_eq!(rust.status, go.status, "{id}: native Go/Rust exit code");
    assert_eq!(rust.stdout, go.stdout, "{id}: native Go/Rust stdout");
    assert_eq!(rust.stderr, go.stderr, "{id}: native Go/Rust stderr");
}

#[cfg(windows)]
fn assert_native_manifests_equal(id: &str, go: &RootManifests, rust: &RootManifests) {
    assert_eq!(rust, go, "{id}: native Go/Rust filesystem");
}

#[cfg(windows)]
fn run_native_migration_case(
    id: &str,
    case: &Value,
    program: &Path,
) -> (Vec<NativeProcess>, RootManifests) {
    let root = unique_root();
    fs::create_dir_all(&root).expect("isolated native migration root");
    let _cleanup = Cleanup(root.clone());
    for directory in ["home", "cwd", "capture"] {
        fs::create_dir_all(root.join(directory)).expect("native migration process directory");
    }
    let source = root.join("filesystem").join(id).join("legacy-source");
    fs::create_dir_all(&source).expect("native migration source");
    fs::create_dir_all(root.join("filesystem").join(id).join("home"))
        .expect("native migration case home");
    if id == "migration" {
        write_input(&source.join("config.toml"), "data_dir = 'legacy'\n", 0o644);
    } else {
        for directory in case["input_directories"]
            .as_array()
            .expect("input directories")
        {
            fs::create_dir_all(input_path(&root, directory.as_str().expect("directory")))
                .expect("native migration input directory");
        }
        for (relative, input) in case["input_files"].as_object().expect("input files") {
            write_input(
                &input_path(&root, relative),
                input["content"].as_str().expect("input content"),
                u32::try_from(input["mode"].as_u64().expect("input mode"))
                    .expect("file mode fits u32"),
            );
        }
    }
    let source_before = manifest(&source, &root);
    let mut processes: Vec<&Value> = case.get("preparation").map_or_else(Vec::new, |value| {
        value
            .as_array()
            .expect("preparation processes")
            .iter()
            .collect()
    });
    processes.push(&case["process"]);
    let mut outputs = Vec::with_capacity(processes.len());
    for (index, process) in processes.into_iter().enumerate() {
        let argv: Vec<_> = process["argv"]
            .as_array()
            .expect("operation argv")
            .iter()
            .map(|arg| arg.as_str().expect("string argument"))
            .collect();
        let output = super::run_program_with_resources(
            program,
            &argv,
            &root.join("home"),
            &root.join("cwd"),
            &root.join("capture"),
            index,
            None,
        );
        let output = native_process(&output, &root);
        assert_eq!(
            output.status,
            process["exit_code"].as_i64().map(|code| code as i32),
            "{id}: native process {index} exit code"
        );
        for (name, stream) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
            assert_eq!(
                !stream.is_empty(),
                process[format!("{name}_bytes")]
                    .as_u64()
                    .expect("recorded stream size")
                    > 0,
                "{id}: native process {index} {name} presence"
            );
        }
        outputs.push(output);
    }

    let mut manifests = BTreeMap::new();
    for (name, recorded_path) in case["manifest_roots"].as_object().expect("manifest roots") {
        let relative = recorded_path
            .as_str()
            .expect("recorded root")
            .strip_prefix("<ORACLE_ROOT>/")
            .expect("isolated recorded root");
        let path = input_path(&root, relative);
        manifests.insert(name.clone(), manifest(&path, &root));
        let expected: BTreeMap<_, _> = case["manifests"][name]
            .as_array()
            .expect("recorded manifest")
            .iter()
            .map(|entry| {
                (
                    entry["path"].as_str().expect("recorded path").to_owned(),
                    entry["type"].as_str().expect("recorded type").to_owned(),
                )
            })
            .collect();
        let observed: BTreeMap<_, _> = manifests[name]
            .iter()
            .map(|(path, entry)| {
                (
                    path.clone(),
                    entry["type"].as_str().expect("native type").to_owned(),
                )
            })
            .collect();
        assert_eq!(observed, expected, "{id}: {name} native manifest shape");
        for entry in case["manifests"][name]
            .as_array()
            .expect("recorded manifest")
        {
            let relative = entry["path"].as_str().expect("recorded path");
            if entry["type"] == "file"
                && !matches!(relative, ".migration-state.json" | ".complete.json")
            {
                assert_eq!(
                    manifests[name][relative]["size_bytes"], entry["size_bytes"],
                    "{id}: {name}/{relative} native size"
                );
                assert_eq!(
                    manifests[name][relative]["sha256"], entry["sha256"],
                    "{id}: {name}/{relative} native hash"
                );
            }
        }
        let expected_exists = case
            .get("root_exists")
            .map_or(!expected.is_empty(), |exists| {
                exists[name].as_bool().expect("recorded existence")
            });
        assert_eq!(
            path.try_exists().expect("native root existence"),
            expected_exists,
            "{id}: {name} existence"
        );
    }
    assert_eq!(
        manifests["source"], source_before,
        "{id}: native source changed"
    );
    (outputs, manifests)
}

#[test]
#[cfg(windows)]
fn migration_filesystem_matches_pinned_native_go() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../rust-tests/parity/cases/filesystem/manifests.json"
    )))
    .expect("filesystem oracle JSON");
    assert_eq!(fixture["commit"], "4e582f28");
    let cases: BTreeMap<_, _> = fixture["cases"]
        .as_array()
        .expect("filesystem cases")
        .iter()
        .filter_map(|case| {
            let id = case["id"].as_str().expect("case id");
            (id == "migration" || id.starts_with("migration-")).then_some((id, case))
        })
        .collect();
    let mut expected_ids = [
        "migration",
        "migration-resume",
        "migration-manual-secrets",
        "migration-copy-secrets-rejected",
        "migration-incomplete-backup",
    ];
    expected_ids.sort_unstable();
    assert_eq!(cases.keys().copied().collect::<Vec<_>>(), expected_ids);

    let oracle_root = unique_root();
    fs::create_dir_all(&oracle_root).expect("pinned Go oracle root");
    let _cleanup = Cleanup(oracle_root.clone());
    let go_binary = super::pinned_go_binary(&oracle_root, "4e582f28");
    let rust_binary = binary();
    let mut negative_control_done = false;
    for (id, case) in cases {
        let (go, go_manifests) = run_native_migration_case(id, case, &go_binary);
        let (rust, rust_manifests) = run_native_migration_case(id, case, &rust_binary);
        assert_eq!(rust.len(), go.len(), "{id}: Go/Rust process count");
        for (index, (go, rust)) in go.iter().zip(&rust).enumerate() {
            assert_native_processes_equal(&format!("{id}: process {index}"), go, rust);
        }
        assert_native_manifests_equal(id, &go_manifests, &rust_manifests);
        if id == "migration" {
            let mut corrupted_output = rust[0].clone();
            corrupted_output.stdout.push(b'!');
            let output_rejection = std::panic::catch_unwind(|| {
                assert_native_processes_equal(
                    "migration stdout negative control",
                    &go[0],
                    &corrupted_output,
                )
            });
            assert!(
                output_rejection.is_err(),
                "differential gate accepts changed stdout"
            );

            let mut corrupted_manifest = rust_manifests.clone();
            corrupted_manifest
                .get_mut("destination")
                .expect("destination manifest")
                .get_mut("config.toml")
                .expect("destination config")
                .as_object_mut()
                .expect("file record")
                .insert("sha256".into(), "0".repeat(64).into());
            let filesystem_rejection = std::panic::catch_unwind(|| {
                assert_native_manifests_equal(
                    "migration filesystem negative control",
                    &go_manifests,
                    &corrupted_manifest,
                )
            });
            assert!(
                filesystem_rejection.is_err(),
                "differential gate accepts changed file content"
            );
            negative_control_done = true;
        }
    }
    assert!(
        negative_control_done,
        "migration differential negative controls ran"
    );
}

fn write_input(path: &Path, content: &str, mode: u32) {
    let parent = path.parent().expect("input parent");
    fs::create_dir_all(parent).expect("input parent directory");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755))
            .expect("recorded input directory mode");
    }
    fs::write(path, content.as_bytes()).expect("recorded input");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("oracle input mode");
    }
    #[cfg(not(unix))]
    let _ = mode;
}
