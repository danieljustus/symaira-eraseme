//! CLI-024: replay recorded migration inputs, output bytes and filesystem state.

use super::frozen_windows_migration;
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

/// Shape both sides compare: entry type plus, for files, size and digest.
/// Directories carry no size, matching the recorded manifests.
type Projected = BTreeMap<String, BTreeMap<String, (String, Option<u64>, Option<String>)>>;

/// Process output in the shape both sides compare: status plus root-folded streams.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeProcess {
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

const FILESYSTEM_IDS: [&str; 5] = [
    "migration",
    "migration-resume",
    "migration-manual-secrets",
    "migration-copy-secrets-rejected",
    "migration-incomplete-backup",
];

/// Migration-state and completion markers carry run-time values. Both sides
/// compare their presence and type only, as the retired native comparator did.
fn volatile(path: &str) -> bool {
    matches!(path, ".migration-state.json" | ".complete.json")
}

#[cfg(windows)]
fn project(manifests: &RootManifests) -> Projected {
    manifests
        .iter()
        .map(|(root, entries)| {
            let shape = entries
                .iter()
                .map(|(path, entry)| {
                    (
                        path.clone(),
                        (
                            entry["type"].as_str().expect("entry type").to_owned(),
                            entry
                                .get("size_bytes")
                                .and_then(Value::as_u64)
                                .filter(|_| !volatile(path)),
                            entry
                                .get("sha256")
                                .and_then(Value::as_str)
                                .filter(|_| !volatile(path))
                                .map(str::to_owned),
                        ),
                    )
                })
                .collect();
            (root.clone(), shape)
        })
        .collect()
}

fn recorded_processes(case: &frozen_windows_migration::FsCase) -> Vec<NativeProcess> {
    case.processes
        .iter()
        .map(|process| NativeProcess {
            status: Some(process.status),
            stdout: process.stdout.clone(),
            stderr: process.stderr.clone(),
        })
        .collect()
}

fn recorded_projection(case: &frozen_windows_migration::FsCase) -> Projected {
    case.roots
        .iter()
        .map(|(root, recorded)| {
            let shape = recorded
                .entries
                .iter()
                .map(|(path, entry)| {
                    (path.clone(), {
                        let keep = !volatile(path);
                        (
                            entry.kind.clone(),
                            entry.size.filter(|_| keep),
                            entry.sha256.clone().filter(|_| keep),
                        )
                    })
                })
                .collect();
            (root.clone(), shape)
        })
        .collect()
}

fn recorded_exists(case: &frozen_windows_migration::FsCase) -> BTreeMap<String, bool> {
    case.roots
        .iter()
        .map(|(root, recorded)| (root.clone(), recorded.exists))
        .collect()
}

fn assert_processes_equal(id: &str, recorded: &[NativeProcess], observed: &[NativeProcess]) {
    assert_eq!(observed.len(), recorded.len(), "{id}: process count");
    for (index, (want, got)) in recorded.iter().zip(observed).enumerate() {
        assert_eq!(got.status, want.status, "{id}: process {index} exit code");
        assert_eq!(got.stdout, want.stdout, "{id}: process {index} stdout");
        assert_eq!(got.stderr, want.stderr, "{id}: process {index} stderr");
    }
}

fn assert_projection_equal(id: &str, recorded: &Projected, observed: &Projected) {
    assert_eq!(observed, recorded, "{id}: filesystem manifests");
}

fn assert_snapshots_match(
    id: &str,
    recorded: &frozen_windows_migration::FsCase,
    observed: &Projected,
    exists: &BTreeMap<String, bool>,
) {
    for (root, snapshot) in &recorded.roots {
        assert_eq!(exists[root], snapshot.exists, "{id}: {root} existence");
        for (path, bytes, sha) in snapshot.files.iter().filter(|(path, _, _)| !volatile(path)) {
            let entry = observed
                .get(root)
                .and_then(|entries| entries.get(path))
                .unwrap_or_else(|| panic!("{id}: {root}/{path} missing"));
            assert_eq!(entry.1, Some(*bytes), "{id}: {root}/{path} size");
            assert_eq!(
                entry.2.as_deref(),
                Some(sha.as_str()),
                "{id}: {root}/{path} sha256"
            );
        }
    }
}

fn rejects(check: impl FnOnce() + std::panic::UnwindSafe) -> bool {
    std::panic::catch_unwind(check).is_err()
}

/// Corruption controls. Each mutated observation must be rejected by the same
/// comparator that accepted the unmutated one.
fn assert_comparator_rejects_corruption(id: &str, recorded: &frozen_windows_migration::FsCase) {
    let processes = recorded_processes(recorded);
    let projection = recorded_projection(recorded);
    let exists = recorded_exists(recorded);

    let mut bad_stdout = processes.clone();
    bad_stdout[0].stdout.push(b'!');
    assert!(
        rejects(|| assert_processes_equal(id, &processes, &bad_stdout)),
        "{id}: stdout control accepted"
    );

    let (root, path, _) = recorded
        .roots
        .iter()
        .find_map(|(root, snapshot)| {
            snapshot
                .files
                .iter()
                .find(|(path, _, _)| !volatile(path))
                .map(|(path, bytes, sha)| (root.clone(), path.clone(), (*bytes, sha.clone())))
        })
        .expect("a recorded snapshot file");
    let mut bad_file = projection.clone();
    bad_file
        .get_mut(&root)
        .and_then(|entries| entries.get_mut(&path))
        .expect("snapshot file in manifest")
        .2 = Some("0".repeat(64));
    assert!(
        rejects(|| assert_snapshots_match(id, recorded, &bad_file, &exists)),
        "{id}: file content control accepted"
    );
    assert!(
        rejects(|| assert_projection_equal(id, &projection, &bad_file)),
        "{id}: manifest control accepted"
    );

    let mut bad_exists = exists.clone();
    let flipped = bad_exists.get_mut(&root).expect("snapshot root");
    *flipped = !*flipped;
    assert!(
        rejects(|| assert_snapshots_match(id, recorded, &projection, &bad_exists)),
        "{id}: root existence control accepted"
    );
}

#[test]
fn migration_comparator_rejects_recorded_mutations_on_every_host() {
    // Comparator self-test on the retained Windows records. Recorded-against-
    // recorded equality proves nothing about Rust; only the mutations matter.
    for id in FILESYSTEM_IDS {
        let recorded = frozen_windows_migration::filesystem_observation("windows-amd64", id)
            .expect("retained Windows record");
        let processes = recorded_processes(&recorded);
        assert_processes_equal(id, &processes, &processes);
        let projection = recorded_projection(&recorded);
        assert_projection_equal(id, &projection, &projection);
        assert_snapshots_match(id, &recorded, &projection, &recorded_exists(&recorded));
        assert_comparator_rejects_corruption(id, &recorded);
    }
}

#[cfg(windows)]
fn native_process(output: &super::ProcessOutput, root: &Path) -> NativeProcess {
    NativeProcess {
        status: output.status.code(),
        stdout: fold_root(&output.stdout, root),
        stderr: fold_root(&output.stderr, root),
    }
}

/// Runs the Rust CLI through one recorded filesystem case. Nothing here runs Go.
#[cfg(windows)]
fn run_rust_case(
    id: &str,
    case: &Value,
) -> (Vec<NativeProcess>, Projected, BTreeMap<String, bool>) {
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
        let raw = super::run_program_with_resources(
            &binary(),
            &argv,
            &root.join("home"),
            &root.join("cwd"),
            &root.join("capture"),
            index,
            None,
        );
        outputs.push(native_process(&raw, &root));
    }

    let mut manifests: RootManifests = BTreeMap::new();
    let mut exists = BTreeMap::new();
    for (name, recorded_path) in case["manifest_roots"].as_object().expect("manifest roots") {
        let relative = recorded_path
            .as_str()
            .expect("recorded root")
            .strip_prefix("<ORACLE_ROOT>/")
            .expect("isolated recorded root");
        let path = input_path(&root, relative);
        exists.insert(
            name.clone(),
            path.try_exists().expect("native root existence"),
        );
        manifests.insert(name.clone(), manifest(&path, &root));
    }
    assert_eq!(
        manifests["source"], source_before,
        "{id}: native source changed"
    );
    (outputs, project(&manifests), exists)
}

#[test]
#[cfg(windows)]
fn migration_filesystem_matches_recorded_native_go() {
    let target = frozen_windows_migration::native_host_target().expect("native Windows target");
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
            FILESYSTEM_IDS.contains(&id).then_some((id, case))
        })
        .collect();
    assert_eq!(
        cases.len(),
        FILESYSTEM_IDS.len(),
        "every recorded case has an input"
    );

    for (id, case) in cases {
        let recorded = frozen_windows_migration::filesystem_observation(target, id)
            .expect("retained Windows record");
        let (processes, projection, exists) = run_rust_case(id, case);
        let expected = recorded_processes(&recorded);
        assert_processes_equal(id, &expected, &processes);
        assert_projection_equal(id, &recorded_projection(&recorded), &projection);
        assert_snapshots_match(id, &recorded, &projection, &exists);
        if id == "migration" {
            // Rust-side controls after the positive case: changed stdout must be rejected.
            let mut corrupted = processes.clone();
            corrupted[0].stdout.push(b'!');
            assert!(
                rejects(|| assert_processes_equal(id, &expected, &corrupted)),
                "{id}: Rust stdout control accepted"
            );
        }
        assert_comparator_rejects_corruption(id, &recorded);
    }
}

fn write_input(path: &Path, content: &str, mode: u32) {
    fs::create_dir_all(path.parent().expect("input parent")).expect("input parent directory");
    fs::write(path, content.as_bytes()).expect("recorded input");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("oracle input mode");
    }
    #[cfg(not(unix))]
    let _ = mode;
}
