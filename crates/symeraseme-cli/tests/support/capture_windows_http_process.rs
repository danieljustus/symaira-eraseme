//! Opt-in, source-bound native Go observations for the Windows HTTP process comparators.
//! Enabled only by `SYMERASEME_CAPTURE_WINDOWS_HTTP=<absolute directory outside the checkout>`.
//! Without it every method is inert. Values come from operations the original test already
//! performs and are recorded before the original assertions run. Go is the truth.
use base64::Engine;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const ENV: &str = "SYMERASEME_CAPTURE_WINDOWS_HTTP";
const SCHEMA: &str = "symeraseme.actual-go.windows-http-process.v1";
const GO_VERSION: &str = "go1.26.6";
const RECORD_LIMIT: usize = 4 * 1024 * 1024;
const BYTES_LIMIT: usize = 1024 * 1024;
/// Prefix an isolated console child prints before each measured JSON value.
pub const CHILD_MARKER: &str = "SYMERASEME_WINDOWS_HTTP_OBSERVATION ";
pub const CASES: [&str; 7] = [
    "native_windows_http_matches_checked_out_go",
    "native_windows_disconnect_reaps_host_agent_like_go",
    "native_windows_pathext_availability_matches_live_go",
    "native_windows_disconnect_cancels_provider_request_like_go",
    "native_windows_slow_header_timeout_matches_go",
    "native_windows_ctrl_break_shutdown_matches_go",
    "native_windows_ctrl_c_shutdown_matches_go",
];
const SOURCE_ROOTS: [&str; 6] = [
    "cmd",
    "internal",
    "regdata.go",
    "registry",
    "rust-tests/parity/oracle/native-host-agent",
    "rust-tests/parity/oracle/provider-cancel",
];
const GENERATOR_INPUTS: [&str; 10] = [
    ".github/workflows/capture-windows-http-retirement.yml",
    "crates/symeraseme-cli/tests/mcp_http_windows_process.rs",
    "crates/symeraseme-cli/tests/support/capture_windows_http_process.rs",
    "crates/symeraseme-cli/tests/support/mcp_http_port.rs",
    "crates/symeraseme-cli/tests/support/native_host_agent.rs",
    "crates/symeraseme-cli/Cargo.toml",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "tests/fixtures/provider-cancel/http.json",
];

/// One comparator's observation record. Disabled captures are inert.
pub struct Capture {
    test: &'static str,
    directory: Option<PathBuf>,
    record: Map<String, Value>,
    forbidden: Vec<String>,
    observations: Map<String, Value>,
    withheld: Cell<bool>,
    finished: bool,
}

impl Capture {
    /// `binaries` lists (role, path). Roles starting with `go_` must be Go executables; their
    /// embedded build information is read back. `private` paths must never appear in a record.
    pub fn new(test: &'static str, binaries: &[(&str, &Path)], private: &[&Path]) -> Self {
        assert!(CASES.contains(&test), "unknown capture case");
        let directory = capture_directory();
        let mut forbidden = Vec::new();
        let mut record = Map::new();
        if directory.is_some() {
            for path in private {
                add_forbidden(&mut forbidden, &path.to_string_lossy());
                if let Ok(canonical) = path.canonicalize() {
                    add_forbidden(&mut forbidden, &canonical.to_string_lossy());
                }
            }
            for name in ["USERPROFILE", "HOME", "TEMP", "TMP", "COMPUTERNAME"] {
                if let Some(value) = std::env::var_os(name) {
                    add_forbidden(&mut forbidden, &value.to_string_lossy());
                }
            }
            let checkout = Path::new(ROOT).canonicalize().unwrap();
            add_forbidden(&mut forbidden, &checkout.to_string_lossy());
            record = provenance(binaries);
        }
        Self {
            test,
            directory,
            record,
            forbidden,
            observations: Map::new(),
            withheld: Cell::new(false),
            finished: false,
        }
    }

    /// True when no private value occurs in `raw`. A violation withholds the value rather than
    /// panicking, so the original assertions still decide the test verdict.
    fn clean(&self, raw: &[u8]) -> bool {
        !self
            .forbidden
            .iter()
            .any(|needle| contains(raw, needle.as_bytes()))
    }

    pub fn push(&mut self, category: &str, value: Value) {
        if self.directory.is_none() {
            return;
        }
        let value = if self.clean(value.to_string().as_bytes()) {
            value
        } else {
            self.withheld.set(true);
            json!({"withheld": "private-value"})
        };
        self.observations
            .entry(category.to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .push(value);
    }

    /// Actual status and body. Refuses a body that echoes the bearer token.
    pub fn reply(&self, reply: &(u16, Vec<u8>), token: Option<&str>) -> Value {
        if self.directory.is_none() {
            return Value::Null;
        }
        let echoed = token.is_some_and(|token| contains(&reply.1, token.as_bytes()));
        if echoed || !self.clean(&reply.1) {
            self.withheld.set(true);
            return json!({"status": reply.0, "body": {"withheld": "private-value"}});
        }
        json!({"status": reply.0, "body": measured(&reply.1)})
    }

    pub fn bytes(&self, raw: &[u8]) -> Value {
        if self.directory.is_none() {
            return Value::Null;
        }
        if self.clean(raw) {
            measured(raw)
        } else {
            self.withheld.set(true);
            json!({"withheld": "private-value"})
        }
    }

    /// How a slow-header connection actually closed, and the measured elapsed milliseconds.
    pub fn closure(&self, result: &Result<usize, std::io::ErrorKind>, elapsed: Duration) -> Value {
        let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        match result {
            Ok(received) => {
                json!({"result": "eof", "response_bytes": received, "elapsed_ms": elapsed_ms})
            }
            Err(kind) => {
                json!({"result": "error", "error_kind": format!("{kind:?}"), "elapsed_ms": elapsed_ms})
            }
        }
    }

    /// Adopts values printed by the isolated console child. The child never builds a `Capture`.
    pub fn absorb_console_children(&mut self, log: &str) {
        if self.directory.is_none() {
            return;
        }
        for line in log.lines() {
            if let Some(index) = line.find(CHILD_MARKER) {
                let value: Value = serde_json::from_str(line[index + CHILD_MARKER.len()..].trim())
                    .expect("console child observation is JSON");
                self.push("console_children", value);
            }
        }
    }

    /// Call only after every original assertion of the test has passed.
    pub fn finish(mut self) {
        assert!(
            !self.withheld.get(),
            "capture withheld private observations"
        );
        self.finished = true;
        if self.directory.is_some() {
            self.write("completed")
                .expect("write Windows HTTP process capture");
        }
    }

    fn write(&self, outcome: &str) -> std::io::Result<()> {
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        let mut record = self.record.clone();
        record.insert("schema".into(), json!(SCHEMA));
        record.insert("test".into(), json!(self.test));
        record.insert("outcome".into(), json!(outcome));
        record.insert(
            "observations".into(),
            Value::Object(self.observations.clone()),
        );
        let encoded = serde_json::to_vec_pretty(&Value::Object(record))?;
        if encoded.len() > RECORD_LIMIT {
            return Err(std::io::Error::other("capture record exceeds its bound"));
        }
        if self
            .forbidden
            .iter()
            .any(|needle| contains(&encoded, needle.as_bytes()))
        {
            return Err(std::io::Error::other(
                "capture refused: record contains a private path",
            ));
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("{}.json", self.test)))?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // A failed test keeps its measured prefix. Nothing here may panic while unwinding.
        if self.directory.is_some()
            && !self.finished
            && let Err(error) = self.write("not_completed")
        {
            eprintln!("Windows HTTP process capture not written: {}", error.kind());
        }
    }
}

/// Emitted by the isolated console child, which inherits the variable but records nothing itself.
pub fn child_observation(value: Value) {
    if std::env::var_os(ENV).is_some_and(|value| !value.is_empty()) {
        println!("{CHILD_MARKER}{value}");
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Adds a value in both raw and JSON-escaped form, so a Windows path cannot slip through.
fn add_forbidden(forbidden: &mut Vec<String>, value: &str) {
    if value.len() < 3 {
        return;
    }
    let escaped = serde_json::to_string(value).unwrap();
    let escaped = escaped[1..escaped.len() - 1].to_owned();
    for needle in [value.to_owned(), escaped] {
        if !forbidden.contains(&needle) {
            forbidden.push(needle);
        }
    }
}

fn capture_directory() -> Option<PathBuf> {
    let value = std::env::var_os(ENV).filter(|value| !value.is_empty())?;
    let directory = PathBuf::from(value);
    assert!(directory.is_absolute(), "{ENV} must be an absolute path");
    let checkout = Path::new(ROOT).canonicalize().unwrap();
    // Validate before creating anything, including when the directory is absent.
    let resolved = if directory.exists() {
        directory.canonicalize().unwrap()
    } else {
        directory
            .parent()
            .expect("capture parent")
            .canonicalize()
            .expect("capture parent must already exist")
            .join(directory.file_name().expect("capture directory name"))
    };
    assert!(
        !resolved.starts_with(&checkout),
        "{ENV} must be outside the checkout"
    );
    std::fs::create_dir_all(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    assert!(
        !directory.starts_with(checkout),
        "{ENV} must be outside the checkout"
    );
    Some(directory)
}

fn checked(command: &mut Command) -> Vec<u8> {
    let root = tempfile::tempdir().expect("private provenance output");
    let stdout = root.path().join("stdout");
    let stderr = root.path().join("stderr");
    let mut child = command
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&stdout).unwrap())
        .stderr(std::fs::File::create(&stderr).unwrap())
        .spawn()
        .expect("capture provenance command");
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait().unwrap();
            panic!("capture provenance deadline exceeded");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    child.wait().unwrap();
    assert!(status.success(), "capture provenance command failed");
    assert!(std::fs::metadata(&stdout).unwrap().len() <= BYTES_LIMIT as u64);
    assert!(std::fs::metadata(&stderr).unwrap().len() <= 65536);
    std::fs::read(stdout).unwrap()
}

fn digest(raw: &[u8]) -> Value {
    let sha256: String = Sha256::digest(raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({"bytes": raw.len(), "sha256": sha256})
}

fn file_digest(path: &Path) -> Value {
    digest(&std::fs::read(path).unwrap())
}

fn measured(raw: &[u8]) -> Value {
    assert!(
        raw.len() <= BYTES_LIMIT,
        "measured bytes exceed the capture bound"
    );
    let mut value = digest(raw);
    value["base64"] = base64::engine::general_purpose::STANDARD.encode(raw).into();
    value
}

fn native_target() -> (&'static str, &'static str) {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        "linux" => "linux",
        other => panic!("unrecorded native OS {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => panic!("unrecorded native architecture {other}"),
    };
    (os, arch)
}

fn go_env() -> Value {
    let info: Value = serde_json::from_slice(&checked(
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
            .env("GOTOOLCHAIN", GO_VERSION)
            .env("GOENV", "off")
            .env("GOWORK", "off")
            .env("GOPROXY", "off")
            .env("GOSUMDB", "off"),
    ))
    .unwrap();
    assert_eq!(info["GOVERSION"], GO_VERSION);
    assert_eq!(info["GOHOSTOS"], info["GOOS"]);
    assert_eq!(info["GOHOSTARCH"], info["GOARCH"]);
    info
}

/// Embedded build information of an actual Go executable. Only the executable-path heading
/// is omitted; the compiler version is kept as a field, and the module/VCS/GOOS/GOARCH lines verbatim.
fn buildinfo(binary: &Path, revision: &str, os: &str, arch: &str) -> Value {
    let text = String::from_utf8(checked(
        Command::new("go")
            .args(["version", "-m"])
            .arg(binary)
            .env("GOTOOLCHAIN", GO_VERSION)
            .env("GOENV", "off")
            .env("GOWORK", "off"),
    ))
    .unwrap()
    .replace("\r\n", "\n");
    let (heading, fields) = text.split_once('\n').expect("go version -m heading");
    let version = heading
        .rsplit_once(':')
        .map(|(_, version)| version.trim())
        .expect("heading names the executable");
    assert_eq!(version, GO_VERSION, "embedded Go toolchain");
    let checkout = Path::new(ROOT).canonicalize().unwrap();
    assert!(
        !fields.contains(binary.to_string_lossy().as_ref())
            && !fields.contains(checkout.to_string_lossy().as_ref()),
        "build information must not expose a local absolute path"
    );
    for expected in [
        format!("vcs.revision={revision}"),
        "vcs.modified=false".to_owned(),
        format!("GOOS={os}"),
        format!("GOARCH={arch}"),
    ] {
        assert!(
            fields.contains(&expected),
            "embedded build information lacks {expected}"
        );
    }
    json!({"embedded_go_version": version, "buildinfo_without_executable_heading": fields})
}

fn checkout_autocrlf() -> &'static str {
    let output = Command::new("git")
        .args(["config", "--get", "core.autocrlf"])
        .current_dir(ROOT)
        .output()
        .unwrap();
    assert!(matches!(output.status.code(), Some(0 | 1)));
    assert!(output.stdout.len() <= 32 && output.stderr.is_empty());
    match String::from_utf8(output.stdout).unwrap().trim() {
        "" | "false" => "false",
        "true" => "true",
        "input" => "input",
        _ => panic!("unrecorded checkout newline policy"),
    }
}

fn provenance(binaries: &[(&str, &Path)]) -> Map<String, Value> {
    assert!(
        checked(
            Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(ROOT)
        )
        .is_empty(),
        "capture requires a clean committed checkout"
    );
    let revision = String::from_utf8(checked(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(ROOT),
    ))
    .unwrap()
    .trim()
    .to_owned();
    let env = go_env();
    let (os, arch) = native_target();
    assert_eq!(env["GOHOSTOS"], os);
    assert_eq!(env["GOHOSTARCH"], arch);

    let mut binary_records = Map::new();
    for (role, path) in binaries {
        assert!(!binary_records.contains_key(*role), "duplicate binary role");
        let mut entry = file_digest(path);
        if role.starts_with("go_") {
            entry["embedded_build_info"] = buildinfo(path, &revision, os, arch);
        }
        binary_records.insert((*role).to_owned(), entry);
    }

    let mut sources = Map::new();
    let tracked = checked(
        Command::new("git")
            .args(["ls-files", "-z", "--"])
            .args(SOURCE_ROOTS)
            .current_dir(ROOT),
    );
    for name in tracked.split(|byte| *byte == 0).filter(|name| {
        name.ends_with(b".go")
            || name.starts_with(b"registry/")
            || name.starts_with(b"internal/templating/templates/")
            || *name == b"internal/mcp/tools.json"
    }) {
        let name = std::str::from_utf8(name).unwrap();
        sources.insert(name.to_owned(), file_digest(&Path::new(ROOT).join(name)));
    }
    for name in ["go.mod", "go.sum"] {
        sources.insert(name.to_owned(), file_digest(&Path::new(ROOT).join(name)));
    }
    let generators: Map<String, Value> = GENERATOR_INPUTS
        .into_iter()
        .map(|name| (name.to_owned(), file_digest(&Path::new(ROOT).join(name))))
        .collect();
    let rust_version =
        String::from_utf8(checked(Command::new("rustc").arg("-V").current_dir(ROOT)))
            .unwrap()
            .trim()
            .to_owned();

    let mut record = Map::new();
    record.insert(
        "case_type".into(),
        json!("native_windows_process_comparator"),
    );
    record.insert("case_version".into(), json!(1));
    record.insert("case_inventory".into(), json!(CASES));
    record.insert("source_revision".into(), json!(revision));
    record.insert("native_target".into(), json!(format!("{os}/{arch}")));
    record.insert("go_version".into(), json!(GO_VERSION));
    record.insert("go_env".into(), env);
    record.insert("rust_version".into(), json!(rust_version));
    record.insert("checkout_autocrlf".into(), json!(checkout_autocrlf()));
    record.insert(
        ".gitattributes".into(),
        file_digest(&Path::new(ROOT).join(".gitattributes")),
    );
    record.insert("source_files".into(), Value::Object(sources));
    record.insert("generator_inputs".into(), Value::Object(generators));
    record.insert("binaries".into(), Value::Object(binary_records));
    record.insert(
        "console_child_propagation".into(),
        json!("The isolated console child is the Rust test harness. It inherits the capture variable but never constructs a Capture; it prints CHILD_MARKER lines that the parent absorbs. Its server children call env_clear and receive no capture variable. Only the parent hashes the Go oracle it passes to the child."),
    );
    record.insert(
        "excluded_ephemeral".into(),
        json!([
            "process ids and pid-file contents",
            "ephemeral TCP ports and mcp tokens (token echo is refused)",
            "temporary and checkout absolute paths (refused by the private-path guard)",
            "the PATHEXT lookup path (only found, available, path_nonempty and path_mode are kept)",
            "server process stderr and console log text other than CHILD_MARKER lines"
        ]),
    );
    record.insert(
        "scope".into(),
        json!("Go-side observations of the original auxiliary-process comparators: HTTP statuses and bodies, PATHEXT found/available, provider request line, closure and generator fixture bytes, slow-header closure and elapsed milliseconds, console exit status and port closure. Each value is recorded before its original assertion; those assertions run unchanged."),
    );
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    // These records deliberately lack native provenance and are never acceptance fixtures.
    fn synthetic(directory: &Path) -> Capture {
        let mut forbidden = Vec::new();
        add_forbidden(&mut forbidden, r"C:\private\runner");
        Capture {
            test: CASES[0],
            directory: Some(directory.to_owned()),
            record: Map::from_iter([("synthetic_control".into(), json!(true))]),
            forbidden,
            observations: Map::new(),
            withheld: Cell::new(false),
            finished: false,
        }
    }

    #[test]
    fn capture_privacy_failure_cannot_be_completed() {
        for mode in ["json-path", "raw-path", "token-echo"] {
            let root = tempfile::tempdir().unwrap();
            let outcome = std::panic::catch_unwind(|| {
                let mut capture = synthetic(root.path());
                let value = match mode {
                    "json-path" => json!({"path": r"C:\private\runner\secret"}),
                    "raw-path" => capture.bytes(br"C:\private\runner\secret"),
                    _ => capture.reply(
                        &(200, b"synthetic-bearer".to_vec()),
                        Some("synthetic-bearer"),
                    ),
                };
                capture.push("control", value);
                capture.finish();
            });
            assert!(outcome.is_err());
            let raw = std::fs::read(root.path().join(format!("{}.json", CASES[0]))).unwrap();
            let record: Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(record["outcome"], "not_completed");
            assert!(!contains(&raw, b"synthetic-bearer"));
            assert!(!contains(&raw, b"runner"));
        }
    }

    #[test]
    fn capture_retains_exact_bytes_and_partial_results_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let mut capture = synthetic(root.path());
        let raw = b"measured\r\n\xff";
        let measured = capture.bytes(raw);
        capture.push("control", measured);
        capture.finish();
        let path = root.path().join(format!("{}.json", CASES[0]));
        let before = std::fs::read(&path).unwrap();
        let record: Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(record["outcome"], "completed");
        assert_eq!(record["observations"]["control"][0]["bytes"], raw.len());
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(
                    record["observations"]["control"][0]["base64"]
                        .as_str()
                        .unwrap()
                )
                .unwrap(),
            raw
        );
        assert!(std::panic::catch_unwind(|| synthetic(root.path()).finish()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);

        let partial = tempfile::tempdir().unwrap();
        let mut capture = synthetic(partial.path());
        capture.push("control", json!({"observed": false}));
        drop(capture);
        let record: Value = serde_json::from_slice(
            &std::fs::read(partial.path().join(format!("{}.json", CASES[0]))).unwrap(),
        )
        .unwrap();
        assert_eq!(record["outcome"], "not_completed");
        assert_eq!(record["observations"]["control"][0]["observed"], false);
    }

    #[test]
    fn provenance_command_preserves_real_exit_failure() {
        assert!(checked(Command::new("git").arg("--version")).starts_with(b"git version "));
        assert!(
            std::panic::catch_unwind(|| {
                checked(Command::new("git").arg("--invalid-capture-control-option"))
            })
            .is_err()
        );
    }

    #[test]
    fn capture_directory_rejects_checkout_before_writing() {
        const CHILD: &str = "SYMERASEME_CAPTURE_DIRECTORY_CONTROL";
        if std::env::var_os(CHILD).is_some() {
            assert!(std::panic::catch_unwind(capture_directory).is_err());
            return;
        }
        let unique = tempfile::tempdir().unwrap();
        let denied = Path::new(ROOT).join(unique.path().file_name().unwrap());
        assert!(!denied.exists());
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "capture_windows_http_process::tests::capture_directory_rejects_checkout_before_writing",
            ])
            .env(CHILD, "1")
            .env(ENV, &denied)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        assert!(!denied.exists());
    }
}
