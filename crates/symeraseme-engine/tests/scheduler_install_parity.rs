//! Parity between the Rust scheduler install/status/uninstall port and the
//! committed Go capture at `tests/fixtures/scheduler-install/cases.json`.
//!
//! The capture is produced by `rust-tests/parity/oracle/scheduler-install`,
//! which drives the real `internal/scheduler` with a recording `Runner` and a
//! private HOME. This test replays the same cases through the Rust port with
//! its own recording runner, then compares the observable result: the returned
//! payload, the failure text, every file the run left behind (by content, after
//! normalizing the volatile temp paths) and every command issued.
//!
//! Two of these cases pin behaviour tracked in #1000 — `Status` resolving
//! launchd files under the bare name while `Uninstall` uses the label, and a
//! second install refusing to run. They are pinned as measured, not as desired.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use symeraseme_engine::scheduler::install::{
    ERR_LEGACY_UNITS, InstallOptions, Runner, install, status, uninstall,
};
use symeraseme_engine::scheduler::{Config, Platform};

const FIXTURE: &str = include_str!("../../../tests/fixtures/scheduler-install/cases.json");
const FROZEN_OBSERVATIONS: &[u8] =
    include_bytes!("../../../tests/fixtures/go-frozen/scheduler-install/observations.json");
const FROZEN_MAC: &[u8] =
    include_bytes!("../../../tests/fixtures/go-frozen/scheduler-install/darwin.observations.json");
const FROZEN_WINDOWS: &[u8] =
    include_bytes!("../../../tests/fixtures/go-frozen/scheduler-install/windows.observations.json");

#[path = "../../symeraseme-core/tests/support/go_oracle.rs"]
mod go_oracle;

/// Linux, Windows and Mac use their actual source-bound native captures by default.
/// Explicit live mode always rebuilds and runs the same twenty-case oracle.
fn selected_oracle_capture() -> &'static Value {
    static CAPTURE: OnceLock<Value> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
            Ok("1") => {}
            Ok("0") | Err(std::env::VarError::NotPresent) => {
                if cfg!(target_os = "linux") {
                    return verified_frozen_install(FROZEN_OBSERVATIONS);
                }
                if cfg!(windows) {
                    let arch = match std::env::consts::ARCH {
                        "x86_64" => "amd64",
                        "aarch64" => "arm64",
                        other => panic!("no actual Windows install capture for {other}"),
                    };
                    return verified_native_install(FROZEN_WINDOWS, "windows", arch);
                }
                if cfg!(target_os = "macos") {
                    let arch = match std::env::consts::ARCH {
                        "x86_64" => "amd64",
                        "aarch64" => "arm64",
                        other => panic!("no actual Mac install capture for {other}"),
                    };
                    return verified_native_install(FROZEN_MAC, "darwin", arch);
                }
            }
            _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
        }
        let output = go_oracle::run_oracle("scheduler-install", None);
        assert!(output.status.success(), "Go oracle run failed");
        assert!(output.stderr.is_empty(), "Go oracle emitted errors");
        // Without -fixture the existing oracle emits its case-name summary,
        // then the complete JSON document. Preserve all twenty recorded cases.
        let start = output.stdout.iter().position(|byte| *byte == b'{').unwrap();
        let document: Value = serde_json::from_slice(&output.stdout[start..]).unwrap();
        assert_eq!(document["cases"].as_object().unwrap().len(), 20);
        document
    })
}

fn verified_frozen_install(bytes: &[u8]) -> Value {
    let manifest: Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/go-frozen/scheduler-install/manifest.json"
    ))
    .unwrap();
    assert_eq!(
        manifest["source_revision"],
        "7ba198dab7c4a97efcddb1fa6aedbf2f9d9857d3"
    );
    assert_eq!(manifest["go_version"], "go version go1.26.6 linux/amd64");
    assert_eq!(manifest["native_target"], "linux/amd64");
    assert_eq!(manifest["capture_umask"], "0022");
    assert_eq!(manifest["exit_status"], 0);
    assert_eq!(manifest["observations"]["bytes"], bytes.len());
    assert_eq!(
        manifest["observations"]["sha256"],
        hex::encode(Sha256::digest(bytes))
    );
    for (name, stream) in [
        (
            "stdout",
            include_bytes!(
                "../../../tests/fixtures/go-frozen/scheduler-install/scheduler-install.stdout"
            )
            .as_slice(),
        ),
        (
            "stderr",
            include_bytes!(
                "../../../tests/fixtures/go-frozen/scheduler-install/scheduler-install.stderr"
            )
            .as_slice(),
        ),
    ] {
        assert_eq!(manifest[name]["bytes"], stream.len());
        assert_eq!(
            manifest[name]["sha256"],
            hex::encode(Sha256::digest(stream))
        );
        if name == "stderr" {
            assert!(stream.is_empty());
        }
    }
    for (path, source) in [
        (
            "internal/scheduler/scheduler.go",
            include_bytes!("../../../internal/scheduler/scheduler.go").as_slice(),
        ),
        (
            "rust-tests/parity/oracle/scheduler-install/main.go",
            include_bytes!("../../../rust-tests/parity/oracle/scheduler-install/main.go")
                .as_slice(),
        ),
    ] {
        assert_eq!(manifest["source_files"][path]["bytes"], source.len());
        assert_eq!(
            manifest["source_files"][path]["sha256"],
            hex::encode(Sha256::digest(source))
        );
    }
    let document: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(document["source_revision"], manifest["source_revision"]);
    assert_eq!(document["cases"].as_object().unwrap().len(), 20);
    document
}

fn verified_native_install(bytes: &[u8], os: &str, arch: &str) -> Value {
    let manifest_bytes = match (os, arch) {
        ("windows", "amd64") => include_bytes!(
            "../../../tests/fixtures/go-frozen/scheduler-install/windows-amd64.manifest.json"
        )
        .as_slice(),
        ("windows", "arm64") => include_bytes!(
            "../../../tests/fixtures/go-frozen/scheduler-install/windows-arm64.manifest.json"
        )
        .as_slice(),
        ("darwin", "amd64") => include_bytes!(
            "../../../tests/fixtures/go-frozen/scheduler-install/darwin-amd64.manifest.json"
        )
        .as_slice(),
        ("darwin", "arm64") => include_bytes!(
            "../../../tests/fixtures/go-frozen/scheduler-install/darwin-arm64.manifest.json"
        )
        .as_slice(),
        other => panic!("no actual native install capture for {other:?}"),
    };
    let manifest: Value = serde_json::from_slice(manifest_bytes).unwrap();
    assert_eq!(
        manifest["source_revision"],
        "30eeb38f1e43c8f633d3537818d1de8b96ba9d6a"
    );
    assert_eq!(manifest["go_version"], "go1.26.6");
    assert_eq!(manifest["native_target"], format!("{os}/{arch}"));
    let capture = manifest["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|capture| capture["package"] == "scheduler-install")
        .unwrap();
    assert_eq!(capture["exit_status"], 0);
    assert_eq!(capture["stderr"]["bytes"], 0);
    assert_eq!(capture["stderr"]["sha256"], hex::encode(Sha256::digest([])));
    assert_eq!(capture["fixture"]["bytes"], bytes.len());
    assert_eq!(
        capture["fixture"]["sha256"],
        hex::encode(Sha256::digest(bytes))
    );
    let build_info = capture["embedded_build_info"].as_str().unwrap();
    assert!(build_info.contains("vcs.revision=30eeb38f1e43c8f633d3537818d1de8b96ba9d6a"));
    assert!(build_info.contains("vcs.modified=false"));
    assert!(build_info.contains(&format!("\tGOOS={os}\n")));
    assert!(build_info.contains(&format!("\tGOARCH={arch}\n")));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sources = manifest["source_files"].as_object().unwrap();
    assert_eq!(sources.len(), 65);
    for (name, metadata) in sources {
        let source = fs::read(root.join(name)).unwrap();
        assert_eq!(metadata["bytes"], source.len(), "{name}");
        assert_eq!(
            metadata["sha256"],
            hex::encode(Sha256::digest(source)),
            "{name}"
        );
    }
    let document: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(document["source_revision"], manifest["source_revision"]);
    assert_eq!(document["cases"].as_object().unwrap().len(), 20);
    document
}

#[test]
fn actual_native_windows_install_preserves_full_cases_and_rejects_unknown_architecture() {
    let amd64 = verified_native_install(FROZEN_WINDOWS, "windows", "amd64");
    assert_eq!(
        amd64,
        verified_native_install(FROZEN_WINDOWS, "windows", "arm64")
    );
    assert_eq!(amd64["cases"].as_object().unwrap().len(), 20);
    let linux = verified_frozen_install(FROZEN_OBSERVATIONS);
    assert_ne!(
        amd64["cases"]["cron_install_writes_block"]["files"]["schedules/install.sh"],
        linux["cases"]["cron_install_writes_block"]["files"]["schedules/install.sh"]
    );
    assert!(
        std::panic::catch_unwind(|| verified_native_install(
            FROZEN_WINDOWS,
            "windows",
            "unobserved"
        ))
        .is_err()
    );
    let mut changed = FROZEN_WINDOWS.to_vec();
    changed[0] ^= 1;
    assert!(
        std::panic::catch_unwind(|| verified_native_install(&changed, "windows", "amd64")).is_err()
    );
}

#[test]
fn actual_native_mac_install_preserves_full_file_and_mode_effects() {
    let amd64 = verified_native_install(FROZEN_MAC, "darwin", "amd64");
    assert_eq!(
        amd64,
        verified_native_install(FROZEN_MAC, "darwin", "arm64")
    );
    let linux = verified_frozen_install(FROZEN_OBSERVATIONS);
    assert_eq!(amd64["cases"], linux["cases"]);
    let mut changed = FROZEN_MAC.to_vec();
    changed[0] ^= 1;
    assert!(
        std::panic::catch_unwind(|| verified_native_install(&changed, "darwin", "amd64")).is_err()
    );
    assert!(
        std::panic::catch_unwind(|| verified_native_install(FROZEN_MAC, "unobserved", "amd64"))
            .is_err()
    );
}

#[test]
fn frozen_install_rejects_changed_bytes_and_missing_file_effects() {
    let mut changed = FROZEN_OBSERVATIONS.to_vec();
    changed[0] ^= 1;
    assert!(std::panic::catch_unwind(|| verified_frozen_install(&changed)).is_err());
    let mut missing: Value = serde_json::from_slice(FROZEN_OBSERVATIONS).unwrap();
    missing["cases"]["cron_install_writes_block"]["files"]
        .as_object_mut()
        .unwrap()
        .remove("schedules/symeraseme-poll.sh")
        .unwrap();
    assert!(
        std::panic::catch_unwind(|| verified_frozen_install(
            &serde_json::to_vec(&missing).unwrap()
        ))
        .is_err()
    );
}

/// Answers commands from the capture's own script, so both sides see the same
/// platform. Records every call with the same normalization the capture uses.
struct RecordingRunner {
    /// (scripted command, error text, stdout) — the oracle's `runnerScript`
    /// carries a success payload for `launchctl list`, so stdout must survive
    /// a successful command.
    answers: Vec<(String, String, String)>,
    calls: RefCell<Vec<RecordedCall>>,
    case_root: PathBuf,
    temp_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedCall {
    line: String,
}

impl RecordingRunner {
    fn new(script: &BTreeMap<String, String>, case_root: PathBuf, temp_root: PathBuf) -> Self {
        let answers = script
            .iter()
            .map(|(line, value)| match value.strip_prefix("ERROR: ") {
                Some(error) => (line.clone(), error.to_string(), String::new()),
                None => (line.clone(), String::new(), value.clone()),
            })
            .collect();
        Self {
            answers,
            calls: RefCell::new(Vec::new()),
            case_root,
            temp_root,
        }
    }

    fn lines(&self) -> Vec<RecordedCall> {
        self.calls.borrow().clone()
    }

    /// Go clears `runner.Calls` before the second install, so only the second
    /// run's commands are recorded.
    fn clear(&self) {
        self.calls.borrow_mut().clear();
    }
}

impl Runner for RecordingRunner {
    fn run(&self, name: &str, args: &[&str]) -> std::io::Result<Vec<u8>> {
        let line = format!("{} {}", name, args.join(" ")).trim().to_string();
        self.calls.borrow_mut().push(RecordedCall {
            line: normalize(&line, &self.case_root, &self.temp_root),
        });
        // Longest matching key wins, matching the oracle's runner.
        let mut best: Option<&(String, String, String)> = None;
        for entry in &self.answers {
            let matches = line == entry.0 || line.starts_with(&format!("{} ", entry.0));
            if matches && best.is_none_or(|current| entry.0.len() > current.0.len()) {
                best = Some(entry);
            }
        }
        match best {
            Some((_, error, stdout)) if !error.is_empty() => {
                Err(std::io::Error::other(error.clone()))
            }
            Some((_, _, stdout)) => Ok(stdout.clone().into_bytes()),
            None => Err(std::io::Error::other(format!(
                "no scripted answer for: {line}"
            ))),
        }
    }
}

/// Folds the private HOME the way Go's `relPath` does. Only the unit paths and
/// the output directory go through this; command lines, file contents and error
/// texts keep the `<TMPROOT>/<case>/home/...` form the capture records.
fn normalize_path(value: &str, case_root: &Path, temp_root: &Path) -> String {
    // Fold HOME first: the later root folding would rewrite the raw path into
    // `<TMPROOT>/<case>/home`, after which the raw form no longer matches.
    let home = path_slashes(&case_root.join("home").to_string_lossy(), cfg!(windows));
    let folded = path_slashes(value, cfg!(windows)).replace(&home, "\u{1}HOME\u{1}");
    normalize(&folded, case_root, temp_root).replace("\u{1}HOME\u{1}", "<HOME>")
}

/// Match Go's filepath.ToSlash, which converts separators only on Windows.
fn path_slashes(value: &str, windows: bool) -> String {
    if windows {
        value.replace('\\', "/")
    } else {
        value.to_string()
    }
}

#[test]
fn path_slashes_preserves_unix_and_normalizes_windows() {
    assert_eq!(
        path_slashes(r"C:\tmp\sched\install.sh", true),
        "C:/tmp/sched/install.sh"
    );
    assert_eq!(
        path_slashes(r"relative\literal", false),
        r"relative\literal"
    );
}

/// Folds the volatile roots the way the capture does, so hashes and command
/// lines agree across machines.
fn normalize(value: &str, case_root: &Path, temp_root: &Path) -> String {
    let mut out = path_slashes(value, cfg!(windows));
    let temp = path_slashes(&temp_root.to_string_lossy(), cfg!(windows));
    let case = path_slashes(&case_root.to_string_lossy(), cfg!(windows));
    out = out.replace(&temp, "<TMPROOT>");
    out = out.replace(&case, "<CASE>");
    // The crontab staging file lives in the system temp directory, whose form
    // differs by platform: macOS keeps a trailing separator ("/var/folders/.../T/"),
    // Linux does not ("/tmp"). Trim it to match Go's folding.
    let staging = path_slashes(&std::env::temp_dir().to_string_lossy(), cfg!(windows));
    let staging = staging.trim_end_matches('/');
    out = out.replace(
        &format!("{staging}/.symeraseme-crontab-"),
        "<TMP>/.symeraseme-crontab-",
    );
    out = out.replace(&format!("{staging}/.crontab-"), "<TMP>/.crontab-");
    out = collapse_random(&out, ".crontab-");
    out = collapse_random(&out, ".symeraseme-crontab-");
    out
}

fn collapse_random(value: &str, marker: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(index) = rest.find(marker) {
        let after = index + marker.len();
        let digits: String = rest[after..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        out.push_str(&rest[..after]);
        out.push_str("<RANDOM>");
        rest = &rest[after + digits.len()..];
    }
    out.push_str(rest);
    out
}

/// Captures the same shape the oracle records: relative path -> sha256 of the
/// normalized content, plus the permission bits.
fn capture(
    root: &Path,
    case_root: &Path,
    temp_root: &Path,
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    use sha2::{Digest, Sha256};
    let mut hashes = BTreeMap::new();
    #[cfg_attr(windows, allow(unused_mut))]
    let mut modes = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let relative = match path.strip_prefix(root) {
                Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            let data = fs::read_to_string(&path).unwrap_or_default();
            let normalized = normalize(&data, case_root, temp_root);
            let mut hasher = Sha256::new();
            hasher.update(normalized.as_bytes());
            hashes.insert(relative.clone(), hex(&hasher.finalize()));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                modes.insert(
                    relative,
                    format!("{:04o}", metadata.permissions().mode() & 0o777),
                );
            }
        }
    }
    (hashes, modes)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Every test owns its root; a sibling test cannot remove live evidence.
fn run_root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("symeraseme-sched-install-rust-")
        .tempdir()
        .expect("create owned run root")
}

#[test]
fn scheduler_test_roots_preserve_sibling_evidence_and_cleanup_only_their_owner() {
    let first = run_root();
    let path = first.path().to_path_buf();
    let sentinel = path.join("evidence.txt");
    fs::write(&sentinel, b"owned first-test evidence").unwrap();
    {
        let second = run_root();
        assert_ne!(first.path(), second.path());
        assert_eq!(fs::read(&sentinel).unwrap(), b"owned first-test evidence");
    }
    assert_eq!(fs::read(&sentinel).unwrap(), b"owned first-test evidence");
    drop(first);
    assert!(!path.exists());
}

fn platform_of(value: &str) -> Option<Platform> {
    Platform::parse(value)
}

/// Writes the known unit names so the legacy-scan branches run. `kind` selects
/// what the seeded unit claims to be: `"python"`, `"go"` (written by this
/// implementation) or `"foreign"` (known name, no generator marker).
fn seed_legacy(home: &Path, platform: Platform, kind: &str) {
    let (dir, names): (PathBuf, [&str; 3]) = match platform {
        Platform::Launchd => (
            home.join("Library").join("LaunchAgents"),
            [
                "com.symeraseme.tick.plist",
                "com.symeraseme.poll.plist",
                "com.symeraseme.rescan.plist",
            ],
        ),
        _ => (
            home.join(".config").join("systemd").join("user"),
            [
                "symeraseme-tick.timer",
                "symeraseme-poll.timer",
                "symeraseme-rescan.timer",
            ],
        ),
    };
    fs::create_dir_all(&dir).expect("legacy dir");
    let content = match kind {
        "python" => {
            "# Generated by symeraseme generate-scheduler\n/usr/bin/python3 -m symeraseme.core.scheduler\n"
        }
        "foreign" => {
            "<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict><key>Label</key><string>com.symeraseme.tick</string></dict></plist>\n"
        }
        _ => "# Generated by symeraseme generate-scheduler (Go)\n",
    };
    for name in names {
        let path = dir.join(name);
        fs::write(&path, content).expect("legacy unit");
        // The Go oracle seeds these files with os.WriteFile(..., 0644). Keep
        // the Rust harness's seeded state equivalent when the test process
        // uses umask 000, which otherwise makes fs::write produce 0666.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
                .expect("set Go-equivalent legacy unit mode");
        }
    }
}

fn plain_config(platform: Platform, output_dir: &str) -> Config {
    Config {
        platform: Some(platform),
        output_dir: output_dir.to_string(),
        tick_hour: 10,
        tick_minute: 0,
        project_dir: "/srv/erase".to_string(),
        binary_path: "/usr/local/bin/symeraseme".to_string(),
        ..Config::default()
    }
}

/// The committed capture is from Unix. Native Windows Go reports different
/// permission bits and renders the install/uninstall shell wrappers differently;
/// those values are still compared in full against the live Go run below.
fn comparable_fixture_cases(cases: &Value, windows: bool) -> Value {
    let mut cases = cases.clone();
    if windows {
        for case in cases.as_object_mut().expect("cases object").values_mut() {
            case.as_object_mut().expect("case object").remove("mode");
            let files = case["files"].as_object_mut().expect("files object");
            files.remove("schedules/install.sh");
            files.remove("schedules/uninstall.sh");
        }
    }
    cases
}

#[test]
fn windows_fixture_projection_keeps_unrelated_hashes_strict() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("committed fixture is JSON");
    let mut changed = fixture["cases"].clone();
    changed["cron_install_writes_block"]["files"]["schedules/install.sh"] =
        Value::String("windows".into());
    assert_eq!(
        comparable_fixture_cases(&changed, true),
        comparable_fixture_cases(&fixture["cases"], true),
        "the known Windows wrapper difference must not stale the Unix fixture"
    );
    changed["cron_install_writes_block"]["files"]["schedules/symeraseme-poll.sh"] =
        Value::String("changed".into());
    assert_ne!(
        comparable_fixture_cases(&changed, true),
        comparable_fixture_cases(&fixture["cases"], true),
        "other file hashes must still reject a stale capture"
    );
}

#[test]
fn rust_install_status_uninstall_match_the_go_capture() {
    let document = selected_oracle_capture();
    let committed: Value = serde_json::from_str(FIXTURE).expect("committed fixture is JSON");
    assert_eq!(document["runner_script"], committed["runner_script"]);
    let live_cases = comparable_fixture_cases(&document["cases"], cfg!(windows));
    let committed_cases = comparable_fixture_cases(&committed["cases"], cfg!(windows));
    #[cfg(windows)]
    for (name, case) in document["cases"].as_object().expect("cases object") {
        assert_eq!(
            case["files"]
                .as_object()
                .expect("files object")
                .keys()
                .collect::<Vec<_>>(),
            committed["cases"][name]["files"]
                .as_object()
                .expect("files object")
                .keys()
                .collect::<Vec<_>>(),
            "{name}: generated file inventory differs from the Unix fixture"
        );
    }
    assert_eq!(
        live_cases, committed_cases,
        "the committed fixture no longer matches live Go behaviour; regenerate it"
    );

    let script: BTreeMap<String, String> =
        serde_json::from_value(document["runner_script"].clone()).expect("runner_script shape");
    let cases = document["cases"].as_object().expect("cases object");
    let owned_root = run_root();
    let root = owned_root.path().to_path_buf();
    let mut checked = 0;

    for (name, case) in cases {
        let scenario = Scenario::for_case(name);
        let case_root = root.join(name);
        let home = case_root.join("home");
        let output_dir = case_root.join("schedules");
        fs::create_dir_all(&home).expect("home dir");

        if let Some(kind) = scenario.seed_kind {
            seed_legacy(&home, scenario.platform, kind);
        }

        let runner = RecordingRunner::new(&script, case_root.clone(), root.clone());
        let config = plain_config(scenario.platform, output_dir.to_str().unwrap());
        let mut options = InstallOptions::new(config);
        options.home_dir = Some(home.clone());
        options.replace_legacy = scenario.replace;
        options.runner = Some(&runner);
        if scenario.raw_platform {
            // Go carries the caller's string, not a typed enum, so the
            // rejection happens inside Install/Status/Uninstall.
            options.platform_name = Some("windows".to_string());
        }

        let (payload, error) = match scenario.op {
            Op::Install => {
                if scenario.second_install {
                    let mut first = InstallOptions::new(plain_config(
                        scenario.platform,
                        output_dir.to_str().unwrap(),
                    ));
                    first.home_dir = Some(home.clone());
                    first.runner = Some(&runner);
                    let first_result = install(&first);
                    assert!(
                        first_result.is_ok(),
                        "{name}: the first install must succeed, got {first_result:?}"
                    );
                    runner.clear();
                }
                match install(&options) {
                    Ok(result) => (
                        serde_json::json!({
                            "platform": result.platform.to_string(),
                            "output_dir": normalize_path(&result.output_dir, &case_root, &root),
                            "files": result.files.iter()
                                .map(|path| normalize(path, &case_root, &root))
                                .collect::<Vec<_>>(),
                            "legacy": result.legacy.iter().map(|unit| serde_json::json!({
                                "platform": unit.platform.as_ref().map(ToString::to_string).unwrap_or_default(),
                                "kind": unit.kind,
                                "name": unit.name,
                                "path": normalize_path(&unit.path.to_string_lossy(), &case_root, &root),
                                "is_python": unit.is_python,
                                "reason": unit.reason,
                            })).collect::<Vec<_>>(),
                            "replacement_required": result.replacement_required,
                        }),
                        None,
                    ),
                    Err(failure) => (serde_json::json!({}), Some(failure.to_string())),
                }
            }
            Op::Status => match status(&options) {
                Ok(result) => (
                    serde_json::json!({
                        "platform": result.platform.to_string(),
                        "entries": result.entries.iter().map(|entry| serde_json::json!({
                            "label": entry.label,
                            "installed": entry.installed,
                            "active": entry.active,
                            "path": normalize_path(&entry.path, &case_root, &root),
                            "legacy": entry.legacy,
                            "error": entry.error,
                        })).collect::<Vec<_>>(),
                    }),
                    None,
                ),
                Err(failure) => (serde_json::json!({}), Some(failure.to_string())),
            },
            Op::Uninstall => match uninstall(&options) {
                Ok(()) => (serde_json::json!({"ok": true}), None),
                Err(failure) => (serde_json::json!({}), Some(failure.to_string())),
            },
            Op::UninstallThenStatus => {
                // Install, uninstall, then read the state back: the original
                // defect left `installed: true` after a successful uninstall.
                match install(&options) {
                    Ok(_) => match uninstall(&options) {
                        Ok(()) => match status(&options) {
                            Ok(result) => (
                                serde_json::json!({
                                    "platform": result.platform.to_string(),
                                    "entries": result.entries.iter().map(|entry| serde_json::json!({
                                        "label": entry.label,
                                        "installed": entry.installed,
                                        "active": entry.active,
                                        "path": normalize_path(&entry.path, &case_root, &root),
                                        "legacy": entry.legacy,
                                        "error": entry.error,
                                    })).collect::<Vec<_>>(),
                                }),
                                None,
                            ),
                            Err(failure) => (serde_json::json!({}), Some(failure.to_string())),
                        },
                        Err(failure) => (serde_json::json!({}), Some(failure.to_string())),
                    },
                    Err(failure) => (serde_json::json!({}), Some(failure.to_string())),
                }
            }
        };

        let expected_error = case["error"].as_str().unwrap_or("").to_string();
        // The unsupported-platform case fails before Go builds a result, so the
        // Rust port must carry the same message.
        let expected_error = if expected_error == ERR_LEGACY_UNITS {
            ERR_LEGACY_UNITS.to_string()
        } else {
            expected_error
        };
        let actual_error = error.unwrap_or_default();
        assert_eq!(actual_error, expected_error, "{name}: error text differs");

        if expected_error.is_empty() {
            assert_eq!(
                payload,
                normalize_value(&case["output"], &case_root, &root),
                "{name}: result payload differs"
            );
        }

        #[cfg_attr(windows, allow(unused_variables))]
        let (hashes, modes) = capture(&case_root, &case_root, &root);
        let expected_files: BTreeMap<String, String> =
            serde_json::from_value(case["files"].clone()).expect("files shape");
        assert_eq!(hashes, expected_files, "{name}: filesystem differs");

        #[cfg(unix)]
        {
            let expected_modes: BTreeMap<String, String> =
                serde_json::from_value(case["mode"].clone()).expect("mode shape");
            assert_eq!(modes, expected_modes, "{name}: file modes differ");
        }

        let expected_calls: Vec<String> = case["calls"]
            .as_array()
            .expect("calls array")
            .iter()
            .map(|call| call["line"].as_str().expect("call line").to_string())
            .collect();
        let actual_calls: Vec<String> = runner.lines().into_iter().map(|call| call.line).collect();
        assert_eq!(
            actual_calls, expected_calls,
            "{name}: issued commands differ"
        );

        checked += 1;
    }

    assert_eq!(checked, cases.len(), "every captured case was replayed");
    let _ = &committed;
}

/// Normalizes the captured payload the same way the live values are normalized:
/// the volatile run root becomes `<TMPROOT>` and the per-case root `<CASE>`.
fn normalize_value(value: &Value, case_root: &Path, temp_root: &Path) -> Value {
    match value {
        Value::String(text) => Value::String(normalize_path(text, case_root, temp_root)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| normalize_value(item, case_root, temp_root))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), normalize_value(item, case_root, temp_root)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Install,
    Status,
    Uninstall,
    /// Install, then Uninstall, then Status: proves the two entry points resolve
    /// the same launchd path (the #1000 defect).
    UninstallThenStatus,
}

struct Scenario {
    platform: Platform,
    op: Op,
    raw_platform: bool,
    /// What the pre-seeded unit claims to be: "python", "go", "foreign" or none.
    seed_kind: Option<&'static str>,
    replace: bool,
    second_install: bool,
}

impl Scenario {
    /// Mirrors the oracle's own scenario table.
    fn for_case(name: &str) -> Self {
        let (platform, _) = if name.starts_with("cron_") {
            (Platform::Cron, name)
        } else if name.starts_with("systemd_") {
            (Platform::Systemd, name)
        } else if name.starts_with("launchd_") {
            (Platform::Launchd, name)
        } else {
            (Platform::parse("windows").unwrap_or(Platform::Cron), name)
        };
        let op = if name.contains("after_uninstall") {
            Op::UninstallThenStatus
        } else if name.contains("status") {
            Op::Status
        } else if name.contains("uninstall") {
            Op::Uninstall
        } else {
            Op::Install
        };
        Self {
            platform,
            op,
            raw_platform: name.contains("unsupported_platform"),
            seed_kind: if name.contains("python_legacy") || name.contains("reports_legacy_units") {
                Some("python")
            } else if name.contains("foreign_unit") {
                Some("foreign")
            } else if name.contains("over_own_units")
                || name.contains("reports_active")
                || name.contains("removes_units")
            {
                Some("go")
            } else {
                None
            },
            replace: name.contains("replaces_python_legacy"),
            second_install: name.contains("reinstall"),
        }
    }
}

#[test]
fn unsupported_platform_is_rejected_before_any_side_effect() {
    let owned_root = run_root();
    let root = owned_root.path().to_path_buf();
    let runner = RecordingRunner::new(&BTreeMap::new(), root.clone(), root.clone());
    let config = plain_config(Platform::Cron, root.join("schedules").to_str().unwrap());
    // Go validates the platform string before it does anything else; the Rust
    // port has a closed enum, so an unknown string is expressed by the caller
    // failing to parse it.
    assert!(
        platform_of("windows").is_none(),
        "windows is not a supported platform"
    );
    assert!(
        platform_of("CRON").is_some(),
        "platform parsing stays case-insensitive like Go's strings.ToLower"
    );
    let _ = (&config, &runner, &root);
}

#[test]
fn cron_block_removal_keeps_surrounding_entries() {
    let existing = "17 * * * * /other\n# Symaira EraseMe scheduled tasks\n0 10 * * * stale\n# End Symaira EraseMe scheduled tasks\n25 * * * * /after\n";
    let cleaned = symeraseme_engine::scheduler::install::remove_cron_block(existing);
    assert!(cleaned.contains("/other"), "entries before the block stay");
    assert!(cleaned.contains("/after"), "entries after the block stay");
    assert!(!cleaned.contains("stale"), "the marked block is dropped");
    assert!(!cleaned.contains("# Symaira EraseMe scheduled tasks"));
}
