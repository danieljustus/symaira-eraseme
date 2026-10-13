//! Portable reader for the retained native Windows migration datasets.
//!
//! Every host validates both committed datasets, so a macOS run exercises the
//! reader and its rejection paths. It does not execute Windows Rust.

use super::frozen_review_oracle::digest;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

const DATASET: &str = "tests/fixtures/go-frozen/refresh-8973a584/windows-migration";
const SIDECAR: &str = "privacy-derivation.json";
/// Fixed anchor. Editing the sidecar alone cannot pass: the anchor is pinned here.
const SIDECAR_SHA256: &str = "1e86a6b0f88d230254421cefc18c953ade33f4e1c0362899a7c30738f6d28216";
const SCHEMA: &str = "symeraseme.actual-go.windows-migration.v1";
const PRODUCER_REVISION: &str = "8973a5843279576c3e4faf3e44f0078af667d0cf";
const GO_REQUESTED: &str = "4e582f28";
const GO_RESOLVED: &str = "4e582f284a639bdaa01260b6b8ab6000482e6c0d";
const TARGETS: [&str; 2] = ["windows-amd64", "windows-arm64"];
const FAMILIES: [&str; 2] = ["command-surface", "filesystem"];
const COMMAND_CASES: [&str; 13] = [
    "migrate-artifacts-json",
    "migrate-artifacts-text",
    "migrate-empty-json",
    "migrate-empty-text",
    "migrate-invalid-platform",
    "migrate-nested-roots",
    "migrate-scheduler-cron",
    "migrate-separate-config",
    "operate-migrate",
    "operate-poll-inbox",
    "operate-schedule-install",
    "operate-schedule-status",
    "operate-schedule-uninstall",
];
const FILESYSTEM_CASES: [&str; 5] = [
    "migration",
    "migration-copy-secrets-rejected",
    "migration-incomplete-backup",
    "migration-manual-secrets",
    "migration-resume",
];

fn dataset_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(DATASET)
}

fn inventory(family: &str) -> Result<&'static [&'static str], String> {
    match family {
        "command-surface" => Ok(&COMMAND_CASES),
        "filesystem" => Ok(&FILESYSTEM_CASES),
        other => Err(format!("unsupported family: {other}")),
    }
}

/// Rejects anything that is not a plain relative path inside the dataset.
fn contained(relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("path escapes its dataset: {relative:?}"));
    }
    Ok(path.to_path_buf())
}

fn read_checked(root: &Path, target: &str, relative: &str) -> Result<Vec<u8>, String> {
    let path = root.join(target).join(contained(relative)?);
    fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))
}

fn sidecar(root: &Path) -> Result<Value, String> {
    let bytes = fs::read(root.join(SIDECAR)).map_err(|error| error.to_string())?;
    if digest(&bytes) != SIDECAR_SHA256 {
        return Err("sidecar digest does not match the fixed anchor".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

/// Collects every `runtime_root` value inside a case, at any depth.
fn collect_runtime_roots(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(root) = map.get("runtime_root").and_then(Value::as_str) {
                out.push(root.to_owned());
            }
            for child in map.values() {
                collect_runtime_roots(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_runtime_roots(child, out);
            }
        }
        _ => {}
    }
}

/// Collects every `{file, bytes, sha256}` reference inside a family record.
fn blob_references(value: &Value, out: &mut Vec<(String, u64, String)>) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            if let Some(file) = map.get("file").and_then(Value::as_str) {
                let bytes = map
                    .get("bytes")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| format!("blob {file} lacks bytes"))?;
                let sha = map
                    .get("sha256")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("blob {file} lacks sha256"))?;
                out.push((file.to_owned(), bytes, sha.to_owned()));
            }
            for child in map.values() {
                blob_references(child, out)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                blob_references(child, out)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check(condition: bool, message: impl FnOnce() -> String) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message()) }
}

/// Validates one family record against its sidecar entry and its raw blobs.
/// Returns the record parsed from the authenticated bytes, without rereading it.
fn validate_family(
    root: &Path,
    sidecar: &Value,
    target: &str,
    family: &str,
) -> Result<Value, String> {
    check(TARGETS.contains(&target), || {
        format!("unsupported target: {target}")
    })?;
    let inventory = inventory(family)?;
    let entry = sidecar["families"]
        .as_array()
        .ok_or("sidecar families")?
        .iter()
        .find(|entry| entry["target"] == target.replace('-', "/") && entry["family"] == family)
        .ok_or_else(|| format!("sidecar has no {target}/{family}"))?;

    let bytes = read_checked(root, target, &format!("{family}.json"))?;
    check(digest(&bytes) == entry["derived_record_sha256"], || {
        format!("{target}/{family}: record digest differs from sidecar")
    })?;
    let record: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let (os, arch) = target.split_once('-').ok_or("target shape")?;

    check(record["schema"] == SCHEMA, || {
        format!("{target}/{family}: schema")
    })?;
    check(record["family"] == family, || {
        format!("{target}/{family}: family")
    })?;
    check(record["provenance"]["host"]["os"] == os, || {
        format!("{target}: host os")
    })?;
    check(record["provenance"]["host"]["arch"] == arch, || {
        format!("{target}: host arch")
    })?;
    let go = &record["provenance"]["go"];
    check(go["version"] == "go1.26.6", || {
        format!("{target}: go version")
    })?;
    check(go["revision_requested"] == GO_REQUESTED, || {
        format!("{target}: go requested")
    })?;
    check(go["revision_resolved"] == GO_RESOLVED, || {
        format!("{target}: go resolved")
    })?;
    check(go["goos"] == os && go["goarch"] == arch, || {
        format!("{target}: go platform")
    })?;
    let rust = &record["provenance"]["rust"];
    check(rust["revision"] == PRODUCER_REVISION, || {
        format!("{target}: producer revision")
    })?;
    check(rust["clean_source"] == true, || {
        format!("{target}: producer source not clean")
    })?;
    check(
        record["source_closure_before"] == record["source_closure_after"],
        || format!("{target}/{family}: source closure changed during capture"),
    )?;
    check(record["assertions_completed"] == true, || {
        format!("{target}/{family}: incomplete")
    })?;
    check(record["case_count"] == inventory.len(), || {
        format!("{target}/{family}: case count")
    })?;
    check(
        record["case_inventory"] == serde_json::to_value(inventory).unwrap(),
        || format!("{target}/{family}: case inventory differs"),
    )?;
    let cases = record["cases"].as_object().ok_or("cases")?;
    let mut keys: Vec<&str> = cases.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut expected = inventory.to_vec();
    expected.sort_unstable();
    check(keys == expected, || {
        format!("{target}/{family}: case keys differ")
    })?;
    for (id, case) in cases {
        // command-surface cases carry one case-level root; filesystem cases
        // carry one per recorded process, and every one must be normalized.
        let mut roots = Vec::new();
        collect_runtime_roots(case, &mut roots);
        check(!roots.is_empty(), || {
            format!("{target}/{id}: no runtime root")
        })?;
        check(roots.iter().all(|root| root == "<ORACLE_ROOT>"), || {
            format!("{target}/{id}: runtime root not normalized")
        })?;
    }

    let mut references = Vec::new();
    blob_references(&record["cases"], &mut references)?;
    let blobs = entry["blobs"].as_object().ok_or("sidecar blobs")?;
    for (file, bytes, sha) in &references {
        let blob = blobs
            .get(file)
            .ok_or_else(|| format!("{target}: {file} is not in the sidecar"))?;
        check(
            blob["derived_sha256"] == sha.as_str() && blob["derived_bytes"] == *bytes,
            || format!("{target}: {file} reference differs from sidecar"),
        )?;
        let raw = read_checked(root, target, file)?;
        check(raw.len() as u64 == *bytes && digest(&raw) == *sha, || {
            format!("{target}: {file} bytes or hash differ")
        })?;
    }
    Ok(record)
}

/// Validates the sidecar, all four family records and every committed blob.
/// Returns the number of files verified, including the sidecar itself.
pub fn validate_dataset(root: &Path) -> Result<usize, String> {
    let sidecar = sidecar(root)?;
    check(sidecar["publication_approved"] == false, || {
        "publication must stay unapproved".into()
    })?;
    check(sidecar["native_rust_replay_approved"] == false, || {
        "native Rust replay must stay unapproved".into()
    })?;
    let families = sidecar["families"].as_array().ok_or("sidecar families")?;
    check(families.len() == TARGETS.len() * FAMILIES.len(), || {
        format!("sidecar lists {} families", families.len())
    })?;

    let mut files = 1;
    for target in TARGETS {
        for family in FAMILIES {
            let slash_target = target.replace('-', "/");
            let entry = families
                .iter()
                .find(|entry| entry["target"] == slash_target && entry["family"] == family)
                .ok_or_else(|| format!("sidecar missing {target}/{family}"))?;
            files += 1;
            validate_family(root, &sidecar, target, family)?;
            let blobs = entry["blobs"].as_object().ok_or("sidecar blobs")?;
            for (file, blob) in blobs {
                let raw = read_checked(root, target, file)?;
                check(
                    raw.len() as u64 == blob["derived_bytes"].as_u64().unwrap_or(u64::MAX)
                        && digest(&raw) == blob["derived_sha256"],
                    || format!("{target}: sidecar blob {file} differs"),
                )?;
                files += 1;
            }
        }
    }
    Ok(files)
}

/// The capture producer is retired. Any definition of its variable, even an
/// empty one, is an explicit request this reader cannot honour, so it fails
/// instead of being silently ignored on every host.
pub const RETIRED_CAPTURE_ENV: &str = "SYMERASEME_CAPTURE_WINDOWS_MIGRATION";

pub fn reject_retired_capture(value: Option<&OsStr>) -> Result<(), String> {
    match value {
        None => Ok(()),
        Some(_) => Err(format!(
            "{RETIRED_CAPTURE_ENV} is retired: the committed Windows records are the only oracle"
        )),
    }
}

pub fn reject_retired_capture_env() -> Result<(), String> {
    reject_retired_capture(std::env::var_os(RETIRED_CAPTURE_ENV).as_deref())
}

/// Recorded target matching this host. Only Windows hosts return one; every
/// other host gets None and never runs the native comparison.
pub fn native_host_target() -> Option<&'static str> {
    if !cfg!(windows) {
        return None;
    }
    match std::env::consts::ARCH {
        "x86_64" => Some("windows-amd64"),
        "aarch64" => Some("windows-arm64"),
        _ => None,
    }
}

/// Command-surface case ids recorded from native Windows Go.
pub fn command_ids() -> &'static [&'static str] {
    &COMMAND_CASES
}

/// Status and root-folded streams of one recorded command case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsProcess {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsEntry {
    pub kind: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsRoot {
    pub exists: bool,
    pub entries: BTreeMap<String, FsEntry>,
    /// Snapshot file content: (path, bytes, sha256).
    pub files: Vec<(String, u64, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsCase {
    pub processes: Vec<FsProcess>,
    pub roots: BTreeMap<String, FsRoot>,
}

fn load_family(target: &str, family: &str) -> Result<(PathBuf, Value), String> {
    reject_retired_capture_env()?;
    let root = dataset_root();
    let sidecar = sidecar(&root)?;
    let record = validate_family(&root, &sidecar, target, family)?;
    Ok((root, record))
}

fn checked_blob(root: &Path, target: &str, reference: &Value) -> Result<Vec<u8>, String> {
    let file = reference["file"]
        .as_str()
        .ok_or("blob reference lacks file")?;
    let bytes = reference["bytes"]
        .as_u64()
        .ok_or("blob reference lacks bytes")?;
    let sha = reference["sha256"]
        .as_str()
        .ok_or("blob reference lacks sha256")?;
    let raw = read_checked(root, target, file)?;
    check(raw.len() as u64 == bytes && digest(&raw) == sha, || {
        format!("{target}: {file} bytes or hash differ")
    })?;
    Ok(raw)
}

/// Authenticated status and streams of one recorded command case. The stdout
/// normalization must match the recorded one before the bytes are used.
pub fn command_observation(target: &str, id: &str) -> Result<Observation, String> {
    let (root, record) = load_family(target, "command-surface")?;
    let case = record["cases"]
        .get(id)
        .ok_or_else(|| format!("{target}: no recorded command case {id}"))?;
    let go = &case["go"];
    let status = go["exit_code"].as_i64().ok_or("recorded exit code")?;
    let stdout_norm = if id.starts_with("operate-schedule") {
        "fold_schedule_output"
    } else {
        "fold_root"
    };
    check(case["normalization"]["stdout"] == stdout_norm, || {
        format!("{target}/{id}: stdout normalization differs")
    })?;
    check(case["normalization"]["stderr"] == "fold_root", || {
        format!("{target}/{id}: stderr normalization differs")
    })?;
    check(case["rust"]["exit_code"].as_i64() == Some(status), || {
        format!("{target}/{id}: recorded Rust status differs from Go")
    })?;
    Ok(Observation {
        status: i32::try_from(status).map_err(|error| error.to_string())?,
        stdout: checked_blob(&root, target, &go["stdout"])?,
        stderr: checked_blob(&root, target, &go["stderr"])?,
    })
}

/// Authenticated processes, manifests and snapshots of one recorded filesystem
/// case. Every manifest root must have exactly one recorded snapshot.
pub fn filesystem_observation(target: &str, id: &str) -> Result<FsCase, String> {
    let (root, record) = load_family(target, "filesystem")?;
    let case = record["cases"]
        .get(id)
        .ok_or_else(|| format!("{target}: no recorded filesystem case {id}"))?;
    let go = &case["go"];
    let mut processes = Vec::new();
    for (position, process) in go["processes"]
        .as_array()
        .ok_or("recorded processes")?
        .iter()
        .enumerate()
    {
        check(process["index"].as_u64() == Some(position as u64), || {
            format!("{target}/{id}: process index {position}")
        })?;
        processes.push(FsProcess {
            status: i32::try_from(process["exit_code"].as_i64().ok_or("process exit code")?)
                .map_err(|error| error.to_string())?,
            stdout: checked_blob(&root, target, &process["stdout"]["raw"])?,
            stderr: checked_blob(&root, target, &process["stderr"]["raw"])?,
        });
    }
    let mut roots: BTreeMap<String, FsRoot> = BTreeMap::new();
    for (name, entries) in go["manifests"].as_object().ok_or("recorded manifests")? {
        let mut map = BTreeMap::new();
        for (path, entry) in entries.as_object().ok_or("recorded manifest entries")? {
            check(entry["path"] == path.as_str(), || {
                format!("{target}/{id}/{name}: manifest path key differs")
            })?;
            map.insert(
                path.clone(),
                FsEntry {
                    kind: entry["type"]
                        .as_str()
                        .ok_or("manifest entry type")?
                        .to_owned(),
                    size: entry.get("size_bytes").and_then(Value::as_u64),
                    sha256: entry
                        .get("sha256")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
            );
        }
        roots.insert(
            name.clone(),
            FsRoot {
                exists: false,
                entries: map,
                files: Vec::new(),
            },
        );
    }
    let mut snapshotted = 0usize;
    for snapshot in go["snapshots"].as_array().ok_or("recorded snapshots")? {
        let name = snapshot["name"].as_str().ok_or("snapshot name")?;
        let entry = roots
            .get_mut(name)
            .ok_or_else(|| format!("{target}/{id}: snapshot {name} has no manifest"))?;
        entry.exists = snapshot["exists"].as_bool().ok_or("snapshot existence")?;
        snapshotted += 1;
        for file in snapshot["files"].as_array().ok_or("snapshot files")? {
            let raw = &file["raw"];
            checked_blob(&root, target, raw)?;
            entry.files.push((
                file["path"].as_str().ok_or("snapshot path")?.to_owned(),
                raw["bytes"].as_u64().ok_or("snapshot bytes")?,
                raw["sha256"].as_str().ok_or("snapshot sha256")?.to_owned(),
            ));
        }
    }
    check(snapshotted == roots.len(), || {
        format!("{target}/{id}: a manifest root has no recorded snapshot")
    })?;
    Ok(FsCase { processes, roots })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).expect("scratch dataset");
        for entry in fs::read_dir(from).expect("read dataset") {
            let entry = entry.expect("dataset entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("entry type").is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).expect("copy dataset file");
            }
        }
    }

    fn scratch_copy(label: &str) -> (super::super::Cleanup, PathBuf) {
        let root = super::super::unique_root().join(label);
        copy_dir(&dataset_root(), &root);
        (
            super::super::Cleanup(root.parent().unwrap().to_path_buf()),
            root,
        )
    }

    #[test]
    fn windows_migration_fixtures_validate_on_every_host() {
        // 135 = privacy sidecar + 2 targets * (2 family records + 65 blobs).
        assert_eq!(validate_dataset(&dataset_root()), Ok(135));
    }

    #[test]
    fn windows_migration_reader_rejects_unsupported_targets() {
        let root = dataset_root();
        let side = sidecar(&root).expect("sidecar");
        assert!(validate_family(&root, &side, "linux-amd64", "command-surface").is_err());
        assert!(validate_family(&root, &side, "windows-amd64", "unknown-family").is_err());
    }

    #[test]
    fn windows_migration_reader_rejects_path_escape() {
        for relative in ["", "/outside", "../outside", "a/../../outside"] {
            assert!(contained(relative).is_err(), "accepted {relative:?}");
        }
        assert_eq!(
            contained("command-surface/migrate-empty-json/blob-0001.bin").unwrap(),
            PathBuf::from("command-surface/migrate-empty-json/blob-0001.bin")
        );
    }

    #[test]
    fn windows_migration_reader_rejects_corrupted_blob() {
        let (_cleanup, root) = scratch_copy("corrupt-blob");
        let side = sidecar(&root).expect("sidecar");
        let blob = side["families"][0]["blobs"]
            .as_object()
            .expect("blobs")
            .iter()
            .find(|(_, blob)| blob["derived_bytes"].as_u64().unwrap_or(0) > 0)
            .map(|(file, _)| file.clone())
            .expect("a non-empty blob");
        let target = side["families"][0]["target"]
            .as_str()
            .expect("target")
            .replace('/', "-");
        let path = root.join(&target).join(&blob);
        let mut bytes = fs::read(&path).expect("blob");
        bytes[0] ^= 1;
        fs::write(&path, bytes).expect("corrupt blob");
        assert!(
            validate_dataset(&root).is_err(),
            "accepted a corrupted blob"
        );
    }

    #[test]
    fn windows_migration_reader_rejects_self_baselined_fixture() {
        // Changing a family record and re-pinning its digest in the sidecar
        // must still fail: the sidecar digest is anchored in this source file.
        let (_cleanup, root) = scratch_copy("self-baseline");
        let record_path = root.join("windows-amd64").join("command-surface.json");
        let mut record: Value =
            serde_json::from_slice(&fs::read(&record_path).expect("record")).expect("record JSON");
        record["case_count"] = 12.into();
        let edited = serde_json::to_vec(&record).expect("edited record");
        fs::write(&record_path, &edited).expect("write record");

        let mut side = sidecar_unchecked(&root);
        side["families"][0]["derived_record_sha256"] = digest(&edited).into();
        fs::write(
            root.join(SIDECAR),
            serde_json::to_vec(&side).expect("sidecar JSON"),
        )
        .expect("write sidecar");

        let rejection = sidecar(&root).expect_err("self-baselined sidecar must fail the anchor");
        assert!(
            rejection.contains("fixed anchor"),
            "unexpected: {rejection}"
        );
    }

    #[test]
    fn retired_capture_request_fails_even_when_empty() {
        assert!(reject_retired_capture(None).is_ok());
        assert!(reject_retired_capture(Some(OsStr::new(""))).is_err());
        assert!(reject_retired_capture(Some(OsStr::new("/tmp/capture"))).is_err());
    }

    #[test]
    fn every_recorded_observation_loads_for_every_target() {
        for target in TARGETS {
            for id in command_ids() {
                command_observation(target, id).expect("recorded command observation");
            }
            for id in FILESYSTEM_CASES {
                let case =
                    filesystem_observation(target, id).expect("recorded filesystem observation");
                assert_eq!(case.roots.len(), 3, "{target}/{id}: manifest roots");
            }
        }
    }

    #[test]
    fn recorded_observation_rejects_unknown_target_and_case() {
        assert!(command_observation("linux-amd64", "operate-migrate").is_err());
        assert!(command_observation("windows-amd64", "no-such-case").is_err());
        assert!(filesystem_observation("windows-amd64", "no-such-case").is_err());
    }

    #[test]
    fn native_target_is_absent_off_windows() {
        if !cfg!(windows) {
            assert_eq!(native_host_target(), None);
        }
    }

    fn sidecar_unchecked(root: &Path) -> Value {
        serde_json::from_slice(&fs::read(root.join(SIDECAR)).expect("sidecar"))
            .expect("sidecar JSON")
    }
}
