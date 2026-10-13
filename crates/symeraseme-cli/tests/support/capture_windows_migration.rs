//! Opt-in native Windows recorder for the pinned Go migration comparators.
//!
//! Enabled only by SYMERASEME_CAPTURE_WINDOWS_MIGRATION=<absolute directory
//! outside the repository>. Raw Go streams, exit codes and filesystem snapshot
//! bytes are written as blob files before any folding or assertion runs. Blobs
//! and family manifests are create-only and are never committed by the tests.
#![cfg_attr(not(windows), allow(dead_code))]

use super::{ProcessOutput, fold_root, hex_digest};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const ENV: &str = "SYMERASEME_CAPTURE_WINDOWS_MIGRATION";
const SCHEMA: &str = "symeraseme.actual-go.windows-migration.v1";
const GO_VERSION: &str = "go1.26.6";
const GO_REVISION: &str = "4e582f28";
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const MAX_BLOB_BYTES: usize = 8 * 1024 * 1024;
const MAX_SNAPSHOT_FILES: usize = 256;
/// Rust-side comparator, helper and input files whose digests bind the
/// producer revision. Each path is read from the clean working tree.
const PRODUCER_FILES: [&str; 9] = [
    "crates/symeraseme-cli/tests/command_surface.rs",
    "crates/symeraseme-cli/tests/support/migration_oracle.rs",
    "crates/symeraseme-cli/tests/support/capture_windows_migration.rs",
    "crates/symeraseme-cli/Cargo.toml",
    "rust-tests/parity/cases/cli/behavior.json",
    "rust-tests/parity/cases/cli/surface.json",
    "rust-tests/parity/cases/filesystem/manifests.json",
    "Cargo.lock",
    "rust-toolchain.toml",
];

fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => "unsupported",
    }
}

/// The native gate: Go must target the host Windows OS and architecture.
pub fn native_target(
    go_os: &str,
    go_arch: &str,
    host_os: &str,
    host_machine: &str,
) -> Result<(), String> {
    if go_os != "windows" || host_os != "windows" {
        return Err(format!("not native Windows: GOOS={go_os} host={host_os}"));
    }
    if go_arch != host_machine || !matches!(host_machine, "amd64" | "arm64") {
        return Err(format!(
            "not a native Windows architecture: GOARCH={go_arch} host={host_machine}"
        ));
    }
    Ok(())
}

/// Resolves the opt-in directory. Setting the variable on a non-native host
/// is an error, never a silent skip.
pub fn opt_in_dir(value: Option<&OsStr>) -> Result<Option<PathBuf>, String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    native_target("windows", host_arch(), std::env::consts::OS, host_arch())?;
    let directory = PathBuf::from(value);
    if !directory.is_absolute() {
        return Err("capture directory must be absolute".to_owned());
    }
    fs::create_dir_all(&directory).map_err(|error| format!("create capture directory: {error}"))?;
    let directory = directory
        .canonicalize()
        .map_err(|error| format!("canonicalize capture directory: {error}"))?;
    let repository = Path::new(ROOT)
        .canonicalize()
        .map_err(|error| format!("canonicalize repository: {error}"))?;
    if directory.starts_with(&repository) {
        return Err("capture directory must be outside the repository".to_owned());
    }
    Ok(Some(directory))
}

/// Executable on every platform. Opt-in off Windows must fail here, not pass
/// with zero recorded cases.
#[test]
fn windows_migration_capture_opt_in_is_native_windows_only() {
    if let Err(message) = opt_in_dir(std::env::var_os(ENV).as_deref()) {
        panic!("{ENV}: {message}");
    }
}

#[test]
fn windows_migration_capture_native_gate_rejects_nonnative() {
    assert!(native_target("linux", "amd64", "linux", "amd64").is_err());
    assert!(native_target("darwin", "arm64", "darwin", "arm64").is_err());
    assert!(native_target("windows", "arm64", "windows", "amd64").is_err());
    assert!(native_target("windows", "386", "windows", "386").is_err());
    assert!(native_target("windows", "amd64", "windows", "amd64").is_ok());
}

fn digest(bytes: &[u8]) -> String {
    hex_digest(&Sha256::digest(bytes))
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(ROOT)
        .output()
        .expect("provenance git command");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout)
        .expect("UTF-8 git output")
        .trim()
        .to_owned()
}

fn command_text(command: &mut Command) -> String {
    let output = command.output().expect("provenance command");
    assert!(output.status.success(), "provenance command failed");
    String::from_utf8(output.stdout).expect("UTF-8 provenance output")
}

/// Walks the extracted Go source. Symlinks fail closed; the closure digest
/// covers every relative path and file digest, including go.mod and go.sum.
fn source_closure(root: &Path) -> (String, Vec<Value>) {
    fn collect(base: &Path, directory: &Path, out: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(directory).expect("source closure directory") {
            let path = entry.expect("source closure entry").path();
            let metadata = fs::symlink_metadata(&path).expect("source closure metadata");
            assert!(
                !metadata.file_type().is_symlink(),
                "source closure rejects symlinks: {path:?}"
            );
            if metadata.is_dir() {
                collect(base, &path, out);
            } else {
                assert!(metadata.is_file(), "source closure rejects non-files");
                let relative = path
                    .strip_prefix(base)
                    .expect("beneath source root")
                    .to_str()
                    .expect("UTF-8 source path")
                    .replace('\\', "/");
                out.push((relative, digest(&fs::read(&path).expect("source bytes"))));
            }
        }
    }
    let mut files = Vec::new();
    collect(root, root, &mut files);
    files.sort();
    let mut closure = Sha256::new();
    for (relative, sha) in &files {
        closure.update(format!("{relative}\t{sha}\n").as_bytes());
    }
    let listing = files
        .iter()
        .map(|(path, sha)| json!({"path": path, "sha256": sha}))
        .collect();
    (hex_digest(&closure.finalize()), listing)
}

pub(crate) struct CommandCase<'a> {
    pub(crate) id: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) go: &'a ProcessOutput,
    pub(crate) rust: &'a ProcessOutput,
    pub(crate) expected_stdout: &'a [u8],
    pub(crate) actual_stdout: &'a [u8],
}

/// Names the normalization applied to the stdout comparison, mirroring the
/// comparator's predicates in command_surface.rs.
fn normalization_label(id: &str) -> &'static str {
    if matches!(
        id,
        "operate-plan-status"
            | "status-json"
            | "operate-status"
            | "dashboard-json"
            | "operate-dashboard"
            | "calendar-json"
            | "operate-calendar"
    ) {
        "mask_wall_clock+fold_root"
    } else if id.starts_with("operate-schedule") {
        "fold_schedule_output"
    } else {
        "fold_root"
    }
}

pub(crate) struct Recorder {
    dir: PathBuf,
    blobs: usize,
    go_binary: PathBuf,
    go_source: PathBuf,
    binary_sha256: String,
    closure_before: String,
    provenance: Value,
    commands: BTreeMap<String, Value>,
    filesystems: BTreeMap<String, Value>,
}

impl Recorder {
    /// Returns None when the variable is unset. Provenance is bound from the
    /// actual Go binary bytes, its build info, the extracted source closure
    /// and the clean Rust producer tree; nothing is assumed from HEAD.
    pub(crate) fn from_env(go_binary: &Path, go_source: &Path, rust_binary: &Path) -> Option<Self> {
        let dir = opt_in_dir(std::env::var_os(ENV).as_deref())
            .unwrap_or_else(|error| panic!("{ENV}: {error}"))?;
        let info: Value = serde_json::from_slice(
            &Command::new("go")
                .args([
                    "env",
                    "-json",
                    "GOVERSION",
                    "GOOS",
                    "GOARCH",
                    "GOHOSTOS",
                    "GOHOSTARCH",
                ])
                .env("GOTOOLCHAIN", GO_VERSION)
                .env("GOENV", "off")
                .env("GOWORK", "off")
                .output()
                .expect("go env")
                .stdout,
        )
        .expect("go env JSON");
        assert_eq!(info["GOVERSION"], GO_VERSION, "pinned Go toolchain");
        assert_eq!(info["GOHOSTOS"], info["GOOS"], "Go host OS must be native");
        assert_eq!(
            info["GOHOSTARCH"], info["GOARCH"],
            "Go host arch must be native"
        );
        let goos = info["GOOS"].as_str().expect("GOOS");
        let goarch = info["GOARCH"].as_str().expect("GOARCH");
        native_target(goos, goarch, std::env::consts::OS, host_arch())
            .unwrap_or_else(|error| panic!("{error}"));

        let build_info = command_text(
            Command::new("go")
                .args(["version", "-m"])
                .arg(go_binary)
                .env("GOTOOLCHAIN", GO_VERSION),
        );
        assert!(
            build_info.contains(&format!("GOOS={goos}"))
                && build_info.contains(&format!("GOARCH={goarch}")),
            "actual Go binary build info must match the native target"
        );
        let commit_spec = format!("{GO_REVISION}^{{commit}}");
        let resolved = git(&["rev-parse", "--verify", commit_spec.as_str()]);
        assert!(resolved.starts_with(GO_REVISION), "pinned Go revision");

        assert!(
            go_source.join("go.mod").is_file() && go_source.join("go.sum").is_file(),
            "extracted Go source must include go.mod and go.sum"
        );
        let (closure_before, closure_files) = source_closure(go_source);
        let go_bytes = fs::read(go_binary).expect("Go binary bytes");

        assert!(
            git(&["status", "--porcelain"]).is_empty(),
            "Rust producer source must be clean"
        );
        let rust_head = git(&["rev-parse", "HEAD"]);
        let rust_bytes = fs::read(rust_binary).expect("Rust binary bytes");
        let producer: BTreeMap<String, String> = PRODUCER_FILES
            .iter()
            .map(|name| {
                let bytes = fs::read(Path::new(ROOT).join(name)).expect("producer input");
                ((*name).to_owned(), digest(&bytes))
            })
            .collect();
        let toolchain = command_text(
            Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into())).arg("-Vv"),
        );

        let provenance = json!({
            "host": {"os": std::env::consts::OS, "arch": host_arch()},
            "go": {
                "version": GO_VERSION,
                "revision_requested": GO_REVISION,
                "revision_resolved": resolved,
                "goos": goos,
                "goarch": goarch,
                "build_info": build_info,
                "binary": {"bytes": go_bytes.len(), "sha256": digest(&go_bytes)},
                "source_closure": {"sha256": closure_before, "files": closure_files},
            },
            "rust": {
                "revision": rust_head,
                "clean_source": true,
                "toolchain": toolchain,
                "binary": {"bytes": rust_bytes.len(), "sha256": digest(&rust_bytes)},
                "producer_inputs": producer,
            },
        });
        Some(Self {
            dir,
            blobs: 0,
            go_binary: go_binary.to_path_buf(),
            go_source: go_source.to_path_buf(),
            binary_sha256: digest(&go_bytes),
            closure_before,
            provenance,
            commands: BTreeMap::new(),
            filesystems: BTreeMap::new(),
        })
    }

    /// Writes one raw blob, create-only, and returns its digest record.
    fn blob(&mut self, family: &str, id: &str, bytes: &[u8]) -> Value {
        assert!(
            bytes.len() <= MAX_BLOB_BYTES,
            "{family}/{id}: capture exceeds the bounded blob limit"
        );
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
            "unsafe capture id: {id}"
        );
        self.blobs += 1;
        let relative = format!("{family}/{id}/blob-{:04}.bin", self.blobs);
        let path = self.dir.join(&relative);
        fs::create_dir_all(path.parent().expect("blob parent")).expect("blob directory");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("create raw blob without overwrite");
        file.write_all(bytes).expect("write raw blob");
        json!({"file": relative, "bytes": bytes.len(), "sha256": digest(bytes)})
    }

    /// Raw Go-side stream of one process, plus its folded digest.
    pub(crate) fn raw_process(
        &mut self,
        id: &str,
        index: usize,
        root: &Path,
        output: &ProcessOutput,
    ) -> Value {
        let stdout = self.blob("filesystem", id, &output.stdout);
        let stderr = self.blob("filesystem", id, &output.stderr);
        json!({
            "index": index,
            "exit_code": output.status.code(),
            "stdout": {
                "raw": stdout,
                "folded_sha256": digest(&fold_root(&output.stdout, root)),
                "normalization": "fold_root",
            },
            "stderr": {
                "raw": stderr,
                "folded_sha256": digest(&fold_root(&output.stderr, root)),
                "normalization": "fold_root",
            },
        })
    }

    /// Byte snapshot of one manifest root, taken before the case root is
    /// cleaned up. Symlinks and non-regular files fail closed.
    pub(crate) fn snapshot(&mut self, id: &str, name: &str, directory: &Path) -> Value {
        if !directory.try_exists().expect("snapshot root existence") {
            return json!({"name": name, "exists": false, "files": []});
        }
        let mut found: Vec<(String, PathBuf)> = Vec::new();
        let mut pending = vec![directory.to_path_buf()];
        while let Some(current) = pending.pop() {
            for entry in fs::read_dir(&current).expect("snapshot directory") {
                let path = entry.expect("snapshot entry").path();
                let metadata = fs::symlink_metadata(&path).expect("snapshot metadata");
                assert!(
                    !metadata.file_type().is_symlink(),
                    "snapshot rejects symlinks: {path:?}"
                );
                if metadata.is_dir() {
                    pending.push(path);
                } else {
                    assert!(metadata.is_file(), "snapshot rejects non-regular files");
                    let relative = path
                        .strip_prefix(directory)
                        .expect("snapshot beneath root")
                        .to_str()
                        .expect("UTF-8 snapshot path")
                        .replace('\\', "/");
                    found.push((relative, path));
                }
            }
        }
        assert!(
            found.len() <= MAX_SNAPSHOT_FILES,
            "{id}/{name}: snapshot exceeds the bounded file limit"
        );
        found.sort();
        let mut files = Vec::with_capacity(found.len());
        for (relative, path) in found {
            let bytes = fs::read(&path).expect("snapshot bytes");
            let blob = self.blob("filesystem", id, &bytes);
            files.push(json!({"path": relative, "raw": blob}));
        }
        json!({"name": name, "exists": true, "files": files})
    }

    pub(crate) fn record_command(&mut self, case: CommandCase<'_>) {
        assert!(
            !self.commands.contains_key(case.id),
            "{}: duplicate command capture",
            case.id
        );
        let go_stdout = self.blob("command-surface", case.id, &case.go.stdout);
        let go_stderr = self.blob("command-surface", case.id, &case.go.stderr);
        let entry = json!({
            "go": {
                "exit_code": case.go.status.code(),
                "stdout": go_stdout,
                "stderr": go_stderr,
            },
            "rust": {
                "exit_code": case.rust.status.code(),
                "stdout_folded_sha256": digest(&fold_root(&case.rust.stdout, case.root)),
                "stderr_folded_sha256": digest(&fold_root(&case.rust.stderr, case.root)),
            },
            "normalization": {
                "stdout": normalization_label(case.id),
                "stderr": "fold_root",
                "go_stdout_folded_sha256": digest(&fold_root(&case.go.stdout, case.root)),
                "expected_stdout_folded_sha256": digest(&fold_root(case.expected_stdout, case.root)),
                "actual_stdout_folded_sha256": digest(&fold_root(case.actual_stdout, case.root)),
                "go_stderr_folded_sha256": digest(&fold_root(&case.go.stderr, case.root)),
            },
            "snapshot": "not recorded: Rust and Go share one case root in the command family",
        });
        self.commands.insert(case.id.to_owned(), entry);
    }

    /// Go side: raw blobs, written before any comparison. Rust side: folded
    /// digests only, labelled with the normalization applied.
    pub(crate) fn process(
        &mut self,
        side: &str,
        id: &str,
        index: usize,
        root: &Path,
        output: &ProcessOutput,
    ) -> Value {
        if side == "go" {
            return self.raw_process(id, index, root, output);
        }
        json!({
            "index": index,
            "exit_code": output.status.code(),
            "stdout_folded_sha256": digest(&fold_root(&output.stdout, root)),
            "stderr_folded_sha256": digest(&fold_root(&output.stderr, root)),
            "normalization": "fold_root",
        })
    }

    pub(crate) fn record_side(&mut self, id: &str, side: &str, value: Value) {
        let entry = self
            .filesystems
            .entry(id.to_owned())
            .or_insert_with(|| json!({}));
        assert!(
            entry.get(side).is_none(),
            "{id}/{side}: duplicate filesystem capture"
        );
        entry[side] = value;
    }

    /// Writes the family manifest only after every required case passed.
    /// Refuses to overwrite and re-checks the Go binary and source closure.
    pub(crate) fn finish_family(&self, family: &str, expected: usize) {
        let (closure_after, closure_files) = source_closure(&self.go_source);
        assert_eq!(
            closure_after, self.closure_before,
            "pinned Go source closure changed during capture"
        );
        assert_eq!(
            digest(&fs::read(&self.go_binary).expect("Go binary bytes")),
            self.binary_sha256,
            "Go binary changed during capture"
        );
        let cases = match family {
            "command-surface" => &self.commands,
            "filesystem" => &self.filesystems,
            other => panic!("unknown capture family: {other}"),
        };
        assert_eq!(cases.len(), expected, "{family}: recorded case inventory");
        let body = json!({
            "schema": SCHEMA,
            "family": family,
            "provenance": self.provenance,
            "source_closure_before": self.closure_before,
            "source_closure_after": closure_after,
            "source_files_after": closure_files.len(),
            "case_count": expected,
            "cases": cases,
        });
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.dir.join(format!("{family}.json")))
            .expect("create family manifest without overwrite");
        file.write_all(
            serde_json::to_string_pretty(&body)
                .expect("family JSON")
                .as_bytes(),
        )
        .expect("write family manifest");
        file.write_all(b"\n").expect("terminate family manifest");
    }
}
