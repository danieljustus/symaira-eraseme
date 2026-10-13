//! Opt-in native capture of the ORIGINAL Go consent ACL observations.
//! Compiled on every platform; collection requires native Windows and
//! SYMERASEME_CAPTURE_CONSENT_ACL names an absolute directory outside the
//! repository. Values are recorded raw; nothing is normalized or simulated.
use base64::Engine;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const CASES: [&str; 4] = [
    "inherited",
    "owner-only-parent",
    "readonly-directory",
    "readonly-token",
];
const SOURCE_FILES: [&str; 9] = [
    "go.mod",
    "go.sum",
    "Cargo.lock",
    "rust-tests/parity/oracle/consent-windows-acl/main.go",
    "crates/symeraseme-core/tests/consent_windows_acl.rs",
    "crates/symeraseme-core/tests/support/capture_consent_acl.rs",
    "crates/symeraseme-core/tests/support/consent_acl.ps1",
    ".github/workflows/capture-consent-retirement.yml",
    ".github/workflows/consent-windows.yml",
];
const MAX_STREAM: usize = 64 * 1024;
const MAX_FILE: usize = 64 * 1024;
const MAX_TOTAL: usize = 1024 * 1024;
const MAX_ARTIFACT: usize = 4 * 1024 * 1024;

fn digest(bytes: &[u8]) -> Value {
    json!({"bytes": bytes.len(), "sha256": hex::encode(Sha256::digest(bytes))})
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn checked(command: &mut Command) -> Vec<u8> {
    let output = command.output().expect("capture provenance command");
    assert!(output.status.success(), "capture provenance command failed");
    assert!(output.stdout.len() <= 1024 * 1024 && output.stderr.len() <= MAX_STREAM);
    output.stdout
}

fn git_head() -> String {
    let head = checked(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(ROOT),
    );
    String::from_utf8(head).unwrap().trim().to_owned()
}

fn assert_clean() {
    assert!(
        checked(
            Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(ROOT)
        )
        .is_empty(),
        "repository must be clean for native consent capture"
    );
}

fn source_closure() -> Map<String, Value> {
    let tracked = checked(
        Command::new("git")
            .args(["ls-files", "-z", "cmd", "internal"])
            .current_dir(ROOT),
    );
    let mut names: Vec<String> = tracked
        .split(|byte| *byte == 0)
        .filter(|name| name.ends_with(b".go"))
        .map(|name| String::from_utf8(name.to_vec()).unwrap())
        .collect();
    names.extend(SOURCE_FILES.iter().map(|name| (*name).to_owned()));
    names.sort();
    names.dedup();
    let mut sources = Map::new();
    for name in names {
        let bytes = fs::read(Path::new(ROOT).join(&name)).expect("source closure file");
        sources.insert(name, digest(&bytes));
    }
    sources
}

fn push(map: &mut Map<String, Value>, key: &str, value: Value) {
    assert!(
        map.insert(key.to_owned(), value).is_none(),
        "duplicate capture key"
    );
}

pub struct Recorder {
    directory: PathBuf,
    revision: String,
    target: String,
    go_info: Value,
    build_info: String,
    helper: Value,
    sources: Map<String, Value>,
    runs: Map<String, Value>,
    files: Map<String, Value>,
    acl: Map<String, Value>,
    outcomes: Map<String, Value>,
    completed: Vec<String>,
}

impl Recorder {
    /// None unless SYMERASEME_CAPTURE_CONSENT_ACL is set. Any provenance gap panics.
    pub fn from_env(helper: &Path) -> Option<Self> {
        let value = std::env::var_os("SYMERASEME_CAPTURE_CONSENT_ACL").filter(|v| !v.is_empty())?;
        let requested = PathBuf::from(value);
        assert!(
            requested.is_absolute(),
            "consent ACL capture directory must be absolute"
        );
        fs::create_dir_all(&requested).unwrap();
        let directory = requested.canonicalize().unwrap();
        assert!(
            !directory.starts_with(Path::new(ROOT).canonicalize().unwrap()),
            "consent ACL capture directory must be outside the repository"
        );
        assert!(!directory.join("consent-windows-acl.json").exists());
        assert_eq!(
            std::env::consts::OS,
            "windows",
            "consent ACL capture requires native Windows"
        );
        let arch = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => panic!("unrecorded native target"),
        };
        assert_clean();
        let revision = git_head();
        let go_info: Value = serde_json::from_slice(&checked(
            Command::new("go")
                .args([
                    "env",
                    "-json",
                    "GOVERSION",
                    "GOHOSTOS",
                    "GOHOSTARCH",
                    "GOOS",
                    "GOARCH",
                ])
                .env("GOTOOLCHAIN", "go1.26.6")
                .env("GOENV", "off")
                .env("GOWORK", "off"),
        ))
        .unwrap();
        assert_eq!(go_info["GOVERSION"], "go1.26.6");
        assert_eq!(go_info["GOHOSTOS"], "windows");
        assert_eq!(go_info["GOHOSTARCH"], arch);
        assert_eq!(go_info["GOOS"], go_info["GOHOSTOS"]);
        assert_eq!(go_info["GOARCH"], go_info["GOHOSTARCH"]);
        let build_info = String::from_utf8(checked(
            Command::new("go")
                .args(["version", "-m"])
                .arg(helper)
                .env("GOTOOLCHAIN", "go1.26.6")
                .env("GOENV", "off"),
        ))
        .unwrap();
        for expected in [
            format!("vcs.revision={revision}"),
            "vcs.modified=false".to_owned(),
            "GOOS=windows".to_owned(),
            format!("GOARCH={arch}"),
        ] {
            assert!(
                build_info.contains(&expected),
                "embedded Go build info lacks a required provenance field"
            );
        }
        let helper_bytes = fs::read(helper).unwrap();
        let sources = source_closure();
        Some(Self {
            directory,
            revision,
            target: format!("windows/{arch}"),
            go_info,
            build_info,
            helper: digest(&helper_bytes),
            sources,
            runs: Map::new(),
            files: Map::new(),
            acl: Map::new(),
            outcomes: Map::new(),
            completed: Vec::new(),
        })
    }
}

pub fn note_go_run(rec: &mut Option<Recorder>, case: &str, output: &Output) {
    let Some(rec) = rec.as_mut() else { return };
    assert!(output.stdout.len() <= MAX_STREAM && output.stderr.len() <= MAX_STREAM);
    push(
        &mut rec.runs,
        case,
        json!({
            "exit_status": output.status.code(),
            "stdout_base64": b64(&output.stdout),
            "stdout": digest(&output.stdout),
            "stderr_base64": b64(&output.stderr),
            "stderr": digest(&output.stderr),
        }),
    );
}

pub fn note_acl(rec: &mut Option<Recorder>, label: &str, output: &Output) {
    let Some(rec) = rec.as_mut() else { return };
    assert!(output.stdout.len() <= MAX_STREAM && output.stderr.len() <= MAX_STREAM);
    push(
        &mut rec.acl,
        label,
        json!({
            "exit_status": output.status.code(),
            "stdout_base64": b64(&output.stdout),
            "stdout": digest(&output.stdout),
            "stderr_base64": b64(&output.stderr),
            "stderr": digest(&output.stderr),
        }),
    );
}

pub fn note_go_files(rec: &mut Option<Recorder>, case: &str, directory: &Path, names: &[String]) {
    let Some(rec) = rec.as_mut() else { return };
    let mut entries = Map::new();
    let mut total = 0usize;
    for name in names {
        let bytes = fs::read(directory.join(name)).expect("observed Go file");
        assert!(
            bytes.len() <= MAX_FILE,
            "observed file exceeds the payload bound"
        );
        total += bytes.len();
        assert!(
            total <= MAX_TOTAL,
            "observed files exceed the payload bound"
        );
        entries.insert(
            name.clone(),
            json!({"bytes_base64": b64(&bytes), "bytes": digest(&bytes)}),
        );
    }
    push(
        &mut rec.files,
        case,
        json!({"listing": names, "entries": Value::Object(entries)}),
    );
}

pub fn note_outcome(rec: &mut Option<Recorder>, case: &str, go: &Value, rust_failed: bool) {
    let Some(rec) = rec.as_mut() else { return };
    push(
        &mut rec.outcomes,
        case,
        json!({
            "go": {"failed": go["failed"], "error_class": go["error_class"]},
            "rust": {"failed": rust_failed, "error_class": if rust_failed { "permission" } else { "ok" }},
        }),
    );
}

pub fn note_case_complete(rec: &mut Option<Recorder>, case: &str) {
    let Some(rec) = rec.as_mut() else { return };
    assert!(
        !rec.completed.iter().any(|done| done == case),
        "case completed twice"
    );
    rec.completed.push(case.to_owned());
}

/// Writes only when every case completed its assertions. Failed runs write nothing.
pub fn finish(rec: Option<Recorder>) {
    let Some(rec) = rec else { return };
    assert_clean();
    assert_eq!(
        source_closure(),
        rec.sources,
        "source closure changed during native capture"
    );
    let mut completed = rec.completed.clone();
    completed.sort();
    let mut inventory: Vec<String> = CASES.iter().map(|case| (*case).to_owned()).collect();
    inventory.sort();
    assert_eq!(
        completed, inventory,
        "every case must complete its assertions before capture is written"
    );
    let mut cases = Map::new();
    for case in &inventory {
        cases.insert(
            case.clone(),
            json!({
                "assertions_completed": true,
                "go_run": rec.runs.get(case).cloned().expect("case run recorded"),
                "go_files": rec.files.get(case).cloned().expect("case files recorded"),
                "outcome": rec.outcomes.get(case).cloned().expect("case outcome recorded"),
            }),
        );
    }
    let payload = json!({
        "schema": "symeraseme.actual-go.consent-windows-acl.v1",
        "source_revision": rec.revision,
        "source_clean": true,
        "native_target": rec.target,
        "go": {
            "version": "go1.26.6",
            "env": rec.go_info,
            "embedded_build_info": rec.build_info,
            "binary": rec.helper,
        },
        "source_files": rec.sources,
        "case_inventory": inventory,
        "actual_success": completed == inventory,
        "cases": cases,
        "acl_observations": rec.acl,
        "scope": "Original Go consent observations for four real Windows cases: raw Go stdout/stderr/status, exact file bytes and listing, owner/group/DACL/read-only readouts from the existing PowerShell helper, and token-write success or permission semantics. Isolated HOME/XDG/TEMP; no operator profile and no synthetic values.",
    });
    let encoded = serde_json::to_vec_pretty(&payload).unwrap();
    assert!(
        encoded.len() <= MAX_ARTIFACT,
        "capture payload exceeds its bound"
    );
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(rec.directory.join("consent-windows-acl.json"))
        .expect("create_new capture artifact");
    file.write_all(&encoded).unwrap();
    file.write_all(b"\n").unwrap();
}
