#[path = "support/migration_oracle.rs"]
mod migration_oracle;

use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BEHAVIOR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../rust-tests/parity/cases/cli/behavior.json"
));
const SURFACE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../rust-tests/parity/cases/cli/surface.json"
));
const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

fn decode_base64(input: &str) -> Vec<u8> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut count = 0u8;
    let mut output = Vec::new();
    for byte in input.bytes().filter(|byte| *byte != b'=') {
        let value = alphabet
            .iter()
            .position(|candidate| *candidate == byte)
            .expect("valid base64 fixture") as u32;
        bits = (bits << 6) | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            output.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    output
}

fn binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_symeraseme-rust")
        .or_else(|| std::env::var_os("CARGO_BIN_EXE_symeraseme_rust"))
        .map(PathBuf::from)
        .expect("Cargo provides the CLI binary path")
}

#[derive(Debug)]
struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run(argv: &[&str], home: &Path, cwd: &Path, capture: &Path, index: usize) -> ProcessOutput {
    run_with_resources(argv, home, cwd, capture, index, None)
}

/// The capture normalized the runtime root to `<ORACLE_ROOT>` in argv, stdout
/// and stderr; a replay has to feed the real root back in, otherwise a path
/// case replays with a literal placeholder the oracle never saw.
fn substitute_oracle_root(argv: &[&str], home: &Path) -> Vec<String> {
    let root = home
        .parent()
        .map(|root| root.to_string_lossy().into_owned())
        .unwrap_or_default();
    argv.iter()
        .map(|arg| {
            if root.is_empty() {
                (*arg).to_owned()
            } else {
                arg.replace("<ORACLE_ROOT>", &root)
            }
        })
        .collect()
}

/// The byte-level inverse of the capture's normalization: fold the runtime
/// root back to `<ORACLE_ROOT>` so outputs compare against the recorded bytes.
fn fold_root(bytes: &[u8], root: &Path) -> Vec<u8> {
    let needle = root.to_string_lossy().into_owned();
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return bytes.to_vec();
    }
    let mut folded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(needle) {
            folded.extend_from_slice(b"<ORACLE_ROOT>");
            index += needle.len();
        } else {
            folded.push(bytes[index]);
            index += 1;
        }
    }
    folded
}

/// Fold only a known volatile path, in raw text and in a JSON-escaped string.
fn fold_path(value: &str, path: &Path, placeholder: &str) -> String {
    let literal = path.to_string_lossy();
    if literal.is_empty() {
        return value.to_owned();
    }
    value
        .replace(&literal.replace('\\', "\\\\"), placeholder)
        .replace(literal.as_ref(), placeholder)
}

#[test]
fn fold_path_only_replaces_the_recorded_root() {
    let root = Path::new(r"C:\isolated\case");
    assert_eq!(
        fold_path(
            r#"{"path":"C:\\isolated\\case\\data","other":"D:\\keep"}"#,
            root,
            "<CASE>"
        ),
        r#"{"path":"<CASE>\\data","other":"D:\\keep"}"#
    );
    assert_eq!(
        fold_path(r"C:\isolated\case\data D:\keep", root, "<CASE>"),
        r"<CASE>\data D:\keep"
    );
}

fn run_with_resources(
    argv: &[&str],
    home: &Path,
    cwd: &Path,
    capture: &Path,
    index: usize,
    resources: Option<&Path>,
) -> ProcessOutput {
    run_program_with_resources(&binary(), argv, home, cwd, capture, index, resources)
}

fn run_program_with_resources(
    program: &Path,
    argv: &[&str],
    home: &Path,
    cwd: &Path,
    capture: &Path,
    index: usize,
    resources: Option<&Path>,
) -> ProcessOutput {
    let stdout_path = capture.join(format!("{index}.stdout"));
    let stderr_path = capture.join(format!("{index}.stderr"));
    let stdout = fs::File::create(&stdout_path).expect("create stdout capture");
    let stderr = fs::File::create(&stderr_path).expect("create stderr capture");

    let mut command = Command::new(program);
    command
        .args(substitute_oracle_root(argv, home))
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("PWD", cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if let Some(resources) = resources {
        command.env("SYMERASEME_RESOURCES", resources);
    }
    if let Some(profile_path) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile_path);
    }
    configure_process_group(&mut command);
    let mut child = command.spawn().expect("bounded CLI subprocess starts");
    let started = Instant::now();

    let status = loop {
        if capture_exceeded(&stdout_path, &stderr_path) {
            terminate_bounded(&mut child).expect("oversized CLI process cleanup");
            panic!("CLI output exceeded the bounded capture limit for {argv:?}");
        }
        if let Some(status) = child.try_wait().expect("poll CLI process") {
            break status;
        }
        if started.elapsed() >= PROCESS_TIMEOUT {
            terminate_bounded(&mut child).expect("timed-out CLI process cleanup");
            panic!("CLI process exceeded the bounded timeout for {argv:?}");
        }
        thread::sleep(Duration::from_millis(5));
    };

    ProcessOutput {
        status,
        stdout: read_bounded(&stdout_path),
        stderr: read_bounded(&stderr_path),
    }
}

/// Build the frozen CLI's pinned Go source for the current host. OS error
/// messages must come from a native oracle, not from the Unix fixture.
#[cfg(windows)]
fn pinned_go_binary(root: &Path, revision: &str) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let archive = root.join("go-oracle.tar");
    let source = root.join("go-oracle");
    fs::create_dir_all(&source).expect("isolated pinned oracle source");
    let archived = Command::new("git")
        .current_dir(&repo)
        .args(["archive", "--format=tar", "-o"])
        .arg(&archive)
        .arg(revision)
        .output()
        .expect("archive pinned Go source");
    assert!(
        archived.status.success(),
        "pinned Go source unavailable: {}",
        String::from_utf8_lossy(&archived.stderr)
    );
    let extracted = Command::new("tar")
        .current_dir(root)
        .arg("-xf")
        .arg("go-oracle.tar")
        .arg("-C")
        .arg("go-oracle")
        .output()
        .expect("extract pinned Go source");
    assert!(
        extracted.status.success(),
        "pinned Go source extraction failed: {}",
        String::from_utf8_lossy(&extracted.stderr)
    );
    let program = root.join("symeraseme-go.exe");
    let built = Command::new("go")
        .current_dir(&source)
        .env("GOWORK", "off")
        .env("GOENV", "off")
        .env("GOTOOLCHAIN", "go1.26.6")
        .arg("build")
        .arg("-o")
        .arg(&program)
        .arg("./cmd/symeraseme")
        .output()
        .expect("build pinned Go CLI");
    assert!(
        built.status.success(),
        "pinned Go CLI build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    program
}

fn capture_exceeded(stdout_path: &Path, stderr_path: &Path) -> bool {
    [stdout_path, stderr_path]
        .iter()
        .filter_map(|path| fs::metadata(path).ok())
        .any(|metadata| metadata.len() > MAX_OUTPUT_BYTES)
}

fn read_bounded(path: &Path) -> Vec<u8> {
    let mut file = fs::File::open(path).expect("open capture");
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .expect("read bounded capture");
    assert!(
        bytes.len() as u64 <= MAX_OUTPUT_BYTES,
        "capture exceeded limit"
    );
    bytes
}

fn terminate_bounded(child: &mut Child) -> std::io::Result<()> {
    let tree_result = kill_process_tree(child);
    let direct_result = child.kill();
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    while Instant::now() < deadline {
        if child.try_wait()?.is_some() {
            if let Err(error) = tree_result {
                return Err(std::io::Error::other(format!(
                    "CLI process-tree cleanup failed: {error}"
                )));
            }
            if let Err(error) = direct_result {
                let already_exited = matches!(
                    error.kind(),
                    std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
                ) || error.raw_os_error() == Some(3);
                if !already_exited {
                    return Err(std::io::Error::other(format!(
                        "CLI direct-child cleanup failed: {error}"
                    )));
                }
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "CLI process cleanup exceeded timeout",
    ))
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn configure_process_group(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn kill_process_tree(child: &Child) -> std::io::Result<()> {
    let group = format!("-{}", child.id());
    let status = Command::new("kill").args(["-KILL", &group]).status()?;
    if status.success() || status.code() == Some(1) {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "kill process group exited {status}"
        )))
    }
}

#[cfg(windows)]
fn kill_process_tree(child: &Child) -> std::io::Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .status()?;
    if status.success() || status.code() == Some(128) {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "taskkill process tree exited {status}"
        )))
    }
}

#[cfg(not(any(unix, windows)))]
fn kill_process_tree(_child: &Child) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "process-tree cleanup unsupported on this platform",
    ))
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn unique_root() -> PathBuf {
    static ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let sequence = ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "symeraseme-cli-surface-{}-{nonce}-{sequence}",
        std::process::id(),
    ))
}

const TICK_STATUS_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/cli-tick-status/cases.json"
));
const TICK_STATUS_SEED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/cli-tick-status/seed.sql"
));

/// Runs one case with an explicit data directory, mirroring the Go oracle.
fn run_with_data_dir(
    argv: &[&str],
    home: &Path,
    cwd: &Path,
    capture: &Path,
    index: usize,
    data_dir: &Path,
    extra_env: &[(&str, &str)],
) -> ProcessOutput {
    let stdout_path = capture.join(format!("tick-status-{index}.stdout"));
    let stderr_path = capture.join(format!("tick-status-{index}.stderr"));
    let stdout = fs::File::create(&stdout_path).expect("create stdout capture");
    let stderr = fs::File::create(&stderr_path).expect("create stderr capture");

    let mut command = Command::new(binary());
    command
        .args(substitute_oracle_root(argv, home))
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("PWD", cwd)
        .env("SYMERASEME_DATA_DIR", data_dir)
        .envs(extra_env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    // An instrumented child writes its profile where llvm-cov expects it; without
    // this the coverage run leaves `.profraw` files in the working directory.
    if let Some(profile_path) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile_path);
    }
    configure_process_group(&mut command);
    let mut child = command.spawn().expect("bounded CLI subprocess starts");
    let started = Instant::now();

    let status = loop {
        if capture_exceeded(&stdout_path, &stderr_path) {
            terminate_bounded(&mut child).expect("oversized CLI process cleanup");
            panic!("CLI output exceeded the bounded capture limit for {argv:?}");
        }
        if let Some(status) = child.try_wait().expect("poll CLI process") {
            break status;
        }
        if started.elapsed() >= PROCESS_TIMEOUT {
            terminate_bounded(&mut child).expect("timed-out CLI process cleanup");
            panic!("CLI process exceeded the bounded timeout for {argv:?}");
        }
        thread::sleep(Duration::from_millis(5));
    };

    ProcessOutput {
        status,
        stdout: read_bounded(&stdout_path),
        stderr: read_bounded(&stderr_path),
    }
}

/// Lists a directory's entries so a leak names the file it left behind.
fn directory_entries(directory: &Path, context: &str) -> Vec<String> {
    fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{context}: {error}"))
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// Applies the frozen rows both implementations read.
fn seed_store(database: &Path) {
    let store = symeraseme_core::storage::Store::open(database).expect("open the seeded store");
    store
        .connection()
        .execute_batch(TICK_STATUS_SEED)
        .expect("apply the frozen rows");
}

/// Replaces the reported instants with the fixture's placeholder.
///
/// Only the fields the oracle itself folds are masked, and each one is parsed as
/// an instant before it is replaced, so masking cannot hide a difference in the
/// format Go reports. Go's layout is `2006-01-02T15:04:05.999999-07:00`, so a
/// UTC instant ends in `+00:00` rather than `Z`.
fn mask_wall_clock(payload: &[u8]) -> Vec<u8> {
    let mut text = String::from_utf8(payload.to_vec()).expect("UTF-8 payload");
    for key in ["as_of", "generated_at", "horizon_until"] {
        let prefix = format!("\"{key}\":\"");
        // The search resumes past the folded value: the prefix survives the
        // replacement, so restarting from the front would find it again.
        let mut search_from = 0;
        while let Some(relative) = text[search_from..].find(&prefix) {
            let start = search_from + relative + prefix.len();
            let end = start + text[start..].find('"').expect("a terminated instant");
            let instant = &text[start..end];
            chrono::DateTime::parse_from_rfc3339(instant)
                .unwrap_or_else(|_| panic!("{key} is an RFC 3339 instant"));
            text = format!("{}<TIMESTAMP>\"{}", &text[..start], &text[end + 1..]);
            search_from = start + "<TIMESTAMP>\"".len();
        }
    }
    text.into_bytes()
}

/// `dashboard`, `calendar`, `requests list` and `manual-tasks list` are thin
/// wrappers over an MCP tool (`mcp.ContractHandler()`), which is why each pair of
/// ids carries the same recorded bytes. They are listed here because the
/// selection predicate and the replay body both need them. The `generate-*`
/// commands are the same shape: thin wrappers whose text mode prints only
/// `success`. `review` and `run-web-form` join them for the same reason.
const CONTRACT_TOOL_OPERATIONS: [&str; 19] = [
    "dashboard-json",
    "operate-dashboard",
    "calendar-json",
    "operate-calendar",
    "requests-list-json",
    "operate-requests-list",
    "manual-tasks-list-json",
    "operate-manual-tasks-list",
    "operate-manual-tasks-show",
    "operate-manual-tasks-complete",
    "operate-manual-tasks-cleanup",
    "operate-events-show",
    "operate-grant",
    "grant-dry-run",
    "operate-generate-dashboard",
    "operate-generate-report",
    "operate-generate-scheduler",
    "operate-review",
    "operate-run-web-form",
];

/// `plan status` and `plan tick` answer the Go oracle's recorded bytes.
///
/// Both commands read the store, so every case runs against its own isolated
/// store seeded from the frozen rows the Go oracle applies too: `plan tick`
/// without `--dry-run` mutates that store, and shared state would make the cases
/// order-dependent.
#[test]
fn source_bound_status_and_tick_match_the_go_oracle() {
    let fixture: Value = serde_json::from_str(TICK_STATUS_FIXTURE).expect("tick status fixture");
    assert_eq!(fixture["schema"], "symeraseme.go-oracle.cli.v1");
    assert_eq!(fixture["source"], "cmd/symeraseme/real_commands.go:198-245");
    assert_eq!(fixture["seed"], "tests/fixtures/cli-tick-status/seed.sql");
    let cases = fixture["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 8, "the fixture lost cases");

    let root = unique_root();
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root.clone());

    for (index, case) in cases.iter().enumerate() {
        let id = case["id"].as_str().expect("case id");
        let argv = case["argv"]
            .as_array()
            .expect("argv")
            .iter()
            .map(|arg| arg.as_str().expect("string argv"))
            .collect::<Vec<_>>();
        let home = root.join(format!("home-{id}"));
        let data_dir = root.join(format!("data-{id}"));
        fs::create_dir_all(&home).expect("isolated home");
        fs::create_dir_all(&data_dir).expect("isolated data directory");
        seed_store(&data_dir.join("symeraseme.db"));

        let output = run_with_data_dir(&argv, &home, &cwd, &capture, index, &data_dir, &[]);
        assert_eq!(
            output.status.code(),
            Some(case["exit_code"].as_i64().expect("exit code") as i32),
            "{id} exit code"
        );
        let expected_stdout = decode_base64(case["stdout_base64"].as_str().expect("stdout"));
        let actual_stdout = match case["normalized_fields"].as_array() {
            Some(fields) if fields.iter().any(|field| field == "as_of") => {
                mask_wall_clock(&output.stdout)
            }
            _ => output.stdout.clone(),
        };
        assert_eq!(actual_stdout, expected_stdout, "{id} stdout");
        assert_eq!(
            output.stderr,
            decode_base64(case["stderr_base64"].as_str().expect("stderr")),
            "{id} stderr"
        );
    }
}

fn count_nodes(node: &Value) -> usize {
    1 + node
        .get("children")
        .and_then(Value::as_array)
        .map(|children| children.iter().map(count_nodes).sum())
        .unwrap_or(0)
}

fn is_exact_case(case: &Value) -> bool {
    const PHASE_SUCCESS: [&str; 8] = [
        "version",
        "version-json",
        "config-show",
        "config-show-json",
        "completion-bash",
        "completion-zsh",
        "completion-fish",
        "completion-powershell",
    ];
    const SURFACE_OPERATIONS: [&str; 20] = [
        "operate-brokers-list",
        "operate-plan-status",
        "operate-plan-tick",
        "operate-brokers-show",
        "operate-completion",
        "operate-config-show",
        "operate-help",
        "operate-render-template",
        "operate-serve",
        // `mcp --stdio` serves until stdin reaches EOF; the recorded case runs
        // against a null stdin, so stdout and stderr stay empty and exit is 0.
        "operate-mcp",
        // `auto-confirm` is a thin wrapper over the `auto_confirm` tool; the
        // recorded case has no inbox reply for request 1, so the result is the
        // no-reply struct with DryRun true and the exit is 1.
        "operate-auto-confirm",
        // `migrate` records only validateRoots: a missing source directory,
        // folded back to <ORACLE_ROOT>, on stderr with exit 1.
        "operate-migrate",
        "operate-schedule-install",
        "operate-schedule-status",
        "operate-schedule-uninstall",
        "operate-version",
        // `poll-inbox` is the real MCP handler over the production IMAP dialer;
        // this recorded CLI case pins its surfaced connection error bytes.
        "operate-poll-inbox",
        "operate-poll-inbox-invalid-since",
        "operate-classify-reply",
        "operate-generate-rebuttal",
    ];
    // `registry list`/`validate` are replayed now that cli.rs implements them.
    // They carry the registry contract that `brokers list` cannot: no status
    // filter and no `filters` object, so the count is 1,277 rather than 1,273.
    // `status` and `plan status` share Go's body; the recorded bytes for
    // `status-json`, `operate-status` and `operate-plan-status` are identical,
    // and bare `tick` shares `tickCommandWith` with `plan tick`.
    // `init-profile`/`show-profile` are replayed now that cli.rs implements
    // them: the recorded write lands in the case's own data directory and the
    // recorded read finds no profile at all.
    const PROFILE_OPERATIONS: [&str; 2] = ["operate-init-profile", "operate-show-profile"];
    // `plan create`/`show`/`execute` are replayed now that cli.rs implements
    // them. Each runs against its own data directory, which is why `show` and
    // `execute` report an empty plan although `create` planned one request.
    const PLAN_OPERATIONS: [&str; 3] = [
        "operate-plan-create",
        "operate-plan-show",
        "operate-plan-execute",
    ];
    const STATUS_OPERATIONS: [&str; 3] = ["status-json", "operate-status", "operate-tick"];
    const REGISTRY_OPERATIONS: [&str; 5] = [
        "registry-list",
        "registry-list-json",
        "registry-validate",
        "operate-registry-list",
        "operate-registry-validate",
    ];
    matches!(
        case["category"].as_str(),
        Some("help" | "unknown_flag" | "missing_argument" | "root_version" | "unknown_command")
    ) || case["category"] == "success" && PHASE_SUCCESS.contains(&case["id"].as_str().unwrap_or(""))
        || SURFACE_OPERATIONS.contains(&case["id"].as_str().unwrap_or(""))
        || REGISTRY_OPERATIONS.contains(&case["id"].as_str().unwrap_or(""))
        || STATUS_OPERATIONS.contains(&case["id"].as_str().unwrap_or(""))
        || PROFILE_OPERATIONS.contains(&case["id"].as_str().unwrap_or(""))
        || PLAN_OPERATIONS.contains(&case["id"].as_str().unwrap_or(""))
        || CONTRACT_TOOL_OPERATIONS.contains(&case["id"].as_str().unwrap_or(""))
        || case["category"] == "migration"
}

#[test]
fn frozen_command_surface_matches_phase_two_contract() {
    let surface: Value = serde_json::from_str(SURFACE).expect("surface JSON");
    assert_eq!(count_nodes(&surface["root"]), 51);
    let top_level = surface["root"]["children"].as_array().expect("top level");
    assert_eq!(top_level.len(), 31);
    assert!(
        top_level
            .iter()
            .any(|node| node["name"] == "serve" && node["hidden"] == true)
    );

    let behavior: Value = serde_json::from_str(BEHAVIOR).expect("behavior JSON");
    let cases = behavior["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 175);
    let selected = cases
        .iter()
        .filter(|case| is_exact_case(case))
        .collect::<Vec<_>>();
    let deferred = cases
        .iter()
        .filter(|case| !is_exact_case(case))
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 175);
    assert_eq!(deferred.len(), 0);

    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root.clone());

    #[cfg(windows)]
    let go_binary = {
        assert_eq!(behavior["commit"], "4e582f28", "pinned CLI oracle commit");
        pinned_go_binary(&root, "4e582f28")
    };

    for (index, case) in selected.iter().enumerate() {
        let argv = case["argv"]
            .as_array()
            .expect("argv")
            .iter()
            .map(|arg| arg.as_str().expect("string argv"))
            .collect::<Vec<_>>();
        let id = case["id"].as_str().unwrap_or("");
        let migration_before =
            (case["category"] == "migration").then(|| migration_oracle::prepare(case, &root));
        // The store-backed operations read the event store, so they run against
        // their own data directory: the shared isolated HOME has to stay empty
        // for the assertion below.
        let store_backed = matches!(
            id,
            "operate-plan-status"
                | "operate-plan-tick"
                | "status-json"
                | "operate-status"
                | "operate-tick"
                | "operate-plan-create"
                | "operate-plan-show"
                | "operate-plan-execute"
                | "operate-auto-confirm"
        ) || CONTRACT_TOOL_OPERATIONS.contains(&id);
        // The phase-two capture runs every schedule case in its own
        // `cli/<id>/cwd`, so the generated wrappers record that directory; the
        // replay has to run in the same layout or the folded paths differ.
        // `generate-dashboard` always names a file (`report.html` by
        // default), so it gets the same isolated layout — and the shared
        // cwd stays empty for the assertion below.
        let case_cwd = if id.starts_with("operate-schedule") || id == "operate-generate-dashboard" {
            let dir = root.join("cli").join(id).join("cwd");
            fs::create_dir_all(&dir).expect("isolated schedule cwd");
            dir
        } else {
            cwd.clone()
        };
        let output = if id == "operate-init-profile" {
            // The capture wrote the profile into `cli/<id>/data`, so the
            // recorded path only reproduces from the same layout. The master
            // key comes from the environment, ahead of any OS keychain.
            let data_dir = root.join("cli").join(id).join("data");
            fs::create_dir_all(&data_dir).expect("isolated profile data directory");
            run_with_data_dir(
                &argv,
                &home,
                &case_cwd,
                &capture,
                index,
                &data_dir,
                &[(
                    "SYMERASEME_IDENTITY_MASTER_KEY",
                    "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
                )],
            )
        } else if store_backed {
            let data_dir = root.join(format!("data-{id}"));
            fs::create_dir_all(&data_dir).expect("isolated data directory");
            run_with_data_dir(&argv, &home, &case_cwd, &capture, index, &data_dir, &[])
        } else {
            run(&argv, &home, &case_cwd, &capture, index)
        };
        assert_eq!(
            output.status.code(),
            Some(case["exit_code"].as_i64().unwrap() as i32),
            "{id} status"
        );
        #[cfg(windows)]
        let native_go = if id == "operate-migrate" || id.starts_with("operate-schedule") {
            let go = run_program_with_resources(
                &go_binary,
                &argv,
                &home,
                &case_cwd,
                &capture,
                index + cases.len(),
                None,
            );
            assert_eq!(
                go.status.code(),
                output.status.code(),
                "{id} native Go exit"
            );
            Some(go)
        } else {
            None
        };
        let expected_stdout = match id {
            "root-version" => {
                format!("symeraseme version {}\n", env!("CARGO_PKG_VERSION")).into_bytes()
            }
            "version" => format!("symeraseme {}\n", env!("CARGO_PKG_VERSION")).into_bytes(),
            "version-json" | "operate-version" => format!(
                "{{\"tool\":\"symeraseme\",\"version\":\"{}\",\"schema_version\":1}}\n",
                env!("CARGO_PKG_VERSION")
            )
            .into_bytes(),
            "config-show" => b"data_dir=~/.local/share/symeraseme\nport=8000\n".to_vec(),
            "config-show-json" | "operate-config-show" => b"{\"config\":{\"data_dir\":\"~/.local/share/symeraseme\",\"db_dir\":\"\",\"encrypt_db\":false,\"port\":8000,\"allow_remote\":false},\"success\":true}\n".to_vec(),
            #[cfg(windows)]
            "operate-init-profile" => format!(
                "{{\"profile_path\":{},\"success\":true}}\n",
                serde_json::to_string(
                    &root.join("cli")
                        .join(id)
                        .join("data")
                        .join("identity.encrypted")
                        .to_string_lossy()
                )
                .expect("native Go filepath.Join profile path")
            )
            .into_bytes(),
            #[cfg(windows)]
            id if id.starts_with("operate-schedule") => fold_schedule_output(
                &native_go.as_ref().expect("native schedule oracle").stdout,
                &root,
                &go_binary,
            ),
            _ => decode_base64(case["stdout_base64"].as_str().unwrap()),
        };
        // `plan status` reports a wall clock, so its value is masked before the
        // comparison — the format itself is still asserted by the masker.
        let actual_stdout =
            if matches!(id, "operate-plan-status" | "status-json" | "operate-status")
                || matches!(
                    id,
                    "dashboard-json" | "operate-dashboard" | "calendar-json" | "operate-calendar"
                )
            {
                mask_wall_clock(&output.stdout)
            } else if id == "operate-init-profile" && !cfg!(windows) {
                String::from_utf8_lossy(&output.stdout)
                    .replace(&root.to_string_lossy().to_string(), "<ORACLE_ROOT>")
                    .into_bytes()
            } else if id.starts_with("operate-schedule") {
                // The generated wrappers embed the working directory and the CLI's
                // own resolved executable path; the phase-two capture folds both to
                // `<ORACLE_ROOT>` (the binary as `<ORACLE_ROOT>/bin/symeraseme`).
                // Folding the same values here keeps the rest byte exact.
                fold_schedule_output(&output.stdout, &root, &binary())
            } else {
                output.stdout.clone()
            };
        assert_eq!(
            fold_root(&actual_stdout, &root),
            expected_stdout,
            "{id} stdout"
        );
        #[cfg(windows)]
        let expected_stderr = if let Some(go) = &native_go {
            if id == "operate-migrate" {
                assert!(go.stdout.is_empty(), "native Go migrate stdout");
            }
            fold_root(&go.stderr, &root)
        } else {
            decode_base64(case["stderr_base64"].as_str().unwrap())
        };
        #[cfg(not(windows))]
        let expected_stderr = decode_base64(case["stderr_base64"].as_str().unwrap());
        assert_eq!(
            fold_root(&output.stderr, &root),
            expected_stderr,
            "{id} stderr"
        );
        if let Some(before) = migration_before {
            migration_oracle::assert_unchanged(case, &root, &before);
        }
    }

    for (offset, case) in deferred.iter().enumerate() {
        let argv = case["argv"]
            .as_array()
            .expect("argv")
            .iter()
            .map(|arg| arg.as_str().expect("string argv"))
            .collect::<Vec<_>>();
        let output = run(&argv, &home, &cwd, &capture, selected.len() + offset);
        let id = case["id"].as_str().unwrap_or("");
        assert_ne!(output.status.code(), Some(0), "{id} must fail closed");
        assert!(!output.stderr.is_empty(), "{id} must explain deferral");
        assert!(
            !output
                .stderr
                .windows(home.as_os_str().len())
                .any(|window| { window == home.as_os_str().to_string_lossy().as_bytes() }),
            "{id} leaked isolated HOME"
        );
    }

    let home_entries = directory_entries(&home, "read isolated home");
    assert_eq!(
        home_entries,
        [".local"],
        "only the Go-compatible default event-store directory was created"
    );
    let cwd_entries = directory_entries(&cwd, "read isolated cwd");
    assert!(
        cwd_entries.is_empty(),
        "isolated cwd stayed clean: {cwd_entries:?}"
    );
}

#[test]
fn brokers_operations_match_go_text_filters_and_errors() {
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root);

    let output = run(&["brokers", "list"], &home, &cwd, &capture, 0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"1273 broker(s)\n");
    assert!(output.stderr.is_empty());

    let output = run(
        &["brokers", "list", "--law", "GDPR"],
        &home,
        &cwd,
        &capture,
        1,
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"138 broker(s)\n");
    assert!(output.stderr.is_empty());

    let output = run(
        &["brokers", "list", "ignored-extra"],
        &home,
        &cwd,
        &capture,
        2,
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"1273 broker(s)\n");
    assert!(output.stderr.is_empty());

    let output = run(
        &["brokers", "show", "0ptimus-analytics-us"],
        &home,
        &cwd,
        &capture,
        3,
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"0ptimus Analytics (0ptimus-analytics-us)\n");
    assert!(output.stderr.is_empty());

    let output = run(&["brokers", "show", "not-there"], &home, &cwd, &capture, 4);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"broker \"not-there\" not found\n");

    let output = run(&["brokers", "show"], &home, &cwd, &capture, 5);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stderr, b"accepts 1 arg(s), received 0\n");

    let output = run(&["brokers", "show", "one", "two"], &home, &cwd, &capture, 6);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stderr, b"accepts 1 arg(s), received 2\n");

    let output = run(
        &["brokers", "list", "--output", "yaml"],
        &home,
        &cwd,
        &capture,
        7,
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stderr,
        b"invalid output format \"yaml\": use text or json\n"
    );
}

#[test]
fn brokers_list_honors_resources_override() {
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    let resources = root.join("resources");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    fs::create_dir_all(resources.join("brokers/us")).expect("broker directory");
    fs::create_dir_all(resources.join("schemas")).expect("schema directory");
    fs::write(
        resources.join("manifest.json"),
        br#"{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}"#,
    )
    .expect("registry manifest");
    fs::write(
        resources.join("schemas/broker.schema.json"),
        br#"{"schema_version":1}"#,
    )
    .expect("broker schema");
    let _cleanup = Cleanup(root);

    let output = run_with_resources(
        &["brokers", "list"],
        &home,
        &cwd,
        &capture,
        8,
        Some(&resources),
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"0 broker(s)\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn brokers_json_operations_match_source_bound_goldens() {
    let behavior: Value = serde_json::from_str(BEHAVIOR).expect("behavior JSON");
    let cases = behavior["cases"].as_array().expect("behavior cases");
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root);

    for (index, id) in ["operate-brokers-list", "operate-brokers-show"]
        .iter()
        .enumerate()
    {
        let case = cases
            .iter()
            .find(|case| case["id"].as_str() == Some(*id))
            .unwrap_or_else(|| panic!("missing behavior case {id}"));
        let argv = case["argv"]
            .as_array()
            .expect("behavior argv")
            .iter()
            .map(|arg| arg.as_str().expect("string behavior argv"))
            .collect::<Vec<_>>();
        let output = run(&argv, &home, &cwd, &capture, 20 + index);

        assert_eq!(
            output.status.code(),
            case["exit_code"].as_i64().map(|code| code as i32),
            "{id} exit status"
        );
        assert_eq!(
            output.stdout,
            decode_base64(case["stdout_base64"].as_str().expect("stdout golden")),
            "{id} stdout"
        );
        assert_eq!(
            output.stderr,
            decode_base64(case["stderr_base64"].as_str().expect("stderr golden")),
            "{id} stderr"
        );
    }
}

#[test]
fn review_positional_and_path_aliases_match_the_live_go_cli() {
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root.clone());
    let input = cwd.join("review.txt");
    let original = b"Alice Example <alice@example.invalid>\n";
    fs::write(&input, original).expect("review input");

    let go_binary = root.join(if cfg!(windows) {
        "symeraseme-go.exe"
    } else {
        "symeraseme-go"
    });
    let build = Command::new("go")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .env("GOWORK", "off")
        .env("GOENV", "off")
        .env("GOTOOLCHAIN", "go1.26.6")
        .args(["build", "-o"])
        .arg(&go_binary)
        .arg("./cmd/symeraseme")
        .output()
        .expect("build live Go CLI");
    assert!(
        build.status.success(),
        "Go CLI build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let cases: &[&[&str]] = &[
        &["review"],
        &["review", "--path", "review.txt", "--output", "json"],
        &[
            "review",
            "review.txt",
            "--path",
            "missing.txt",
            "--output",
            "json",
        ],
        &["review", "missing.txt", "--path", "review.txt"],
        &["review", "review.txt"],
        &["--output", "json", "review", "review.txt"],
        &["review", "--path", "missing.txt", "--output", "json"],
        &["review", "review.txt", "--output", "yaml"],
    ];
    for (index, argv) in cases.iter().enumerate() {
        let go =
            run_program_with_resources(&go_binary, argv, &home, &cwd, &capture, index * 2, None);
        let rust =
            run_program_with_resources(&binary(), argv, &home, &cwd, &capture, index * 2 + 1, None);
        assert_eq!(rust.status.code(), go.status.code(), "{argv:?} status");
        assert_eq!(rust.stdout, go.stdout, "{argv:?} stdout");
        assert_eq!(rust.stderr, go.stderr, "{argv:?} stderr");
        if matches!(index, 1 | 2 | 4 | 5) {
            assert!(go.status.success(), "{argv:?} did not exercise redaction");
            assert!(
                !go.stdout
                    .windows(b"alice@example.invalid".len())
                    .any(|window| { window == b"alice@example.invalid" }),
                "{argv:?} exposed the input address"
            );
        }
    }
    assert_eq!(fs::read(input).expect("review input remains"), original);
}

#[test]
fn render_template_unknown_name_matches_go_error() {
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root);

    let output = run(
        &["render-template", "laws/not-there"],
        &home,
        &cwd,
        &capture,
        0,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"templating: unknown template \"not-there\"\n"
    );
}

#[test]
fn render_template_does_not_double_strip_final_go_names() {
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root);

    let valid_prefix = run(
        &["render-template", "laws/laws/gdpr-art17.en.md.j2"],
        &home,
        &cwd,
        &capture,
        0,
    );
    assert_eq!(valid_prefix.status.code(), Some(1));
    assert!(valid_prefix.stdout.is_empty());
    assert_eq!(
        valid_prefix.stderr,
        b"templating: unknown template \"laws/gdpr-art17.en.md.j2\"\n"
    );

    let unknown_prefix = run(
        &["render-template", "laws/laws/not-there"],
        &home,
        &cwd,
        &capture,
        1,
    );
    assert_eq!(unknown_prefix.status.code(), Some(1));
    assert!(unknown_prefix.stdout.is_empty());
    assert_eq!(
        unknown_prefix.stderr,
        b"templating: unknown template \"laws/not-there\"\n"
    );
}

#[test]
fn render_template_accepts_go_double_prefix_normalization() {
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root);

    let canonical = run(
        &["render-template", "laws/gdpr-art17.en.md.j2"],
        &home,
        &cwd,
        &capture,
        0,
    );
    let double_prefix = run(
        &["render-template", "laws/templates/gdpr-art17.en.md.j2"],
        &home,
        &cwd,
        &capture,
        1,
    );
    assert_eq!(canonical.status.code(), Some(0));
    assert_eq!(double_prefix.status.code(), Some(0));
    assert_eq!(double_prefix.stdout, canonical.stdout);
    assert_eq!(double_prefix.stderr, canonical.stderr);
}

const SCHEDULE_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/cli-schedule/cases.json"
));

/// Writes the native units a case starts with, matching the oracle's seed flags.
///
/// The oracle seeds by case, so the predicates here must name the same cases:
/// `seedLabeledUnits` (python content, label names) for the refusal and the
/// uninstall cases, `seedStatusNamedUnits` (bare names) for the status cases,
/// and nothing for the rest.
fn seed_schedule_case(home: &Path, id: &str) {
    let dir = home.join("Library").join("LaunchAgents");
    let (names, content): (&[&str], &str) = if id.contains("refuses-python-legacy")
        || id.contains("replaces-python-legacy")
        || id.contains("uninstall-launchd-without-launchctl")
    {
        (
            &[
                "com.symeraseme.tick.plist",
                "com.symeraseme.poll.plist",
                "com.symeraseme.rescan.plist",
            ],
            "# Generated by symeraseme generate-scheduler\n/usr/bin/python3 -m symeraseme.core.scheduler\n",
        )
    } else if id.starts_with("status-launchd") {
        (
            &[
                "symeraseme-tick.plist",
                "symeraseme-poll.plist",
                "symeraseme-rescan.plist",
            ],
            "# Generated by symeraseme generate-scheduler (Go)\n",
        )
    } else {
        return;
    };
    fs::create_dir_all(&dir).expect("seed directory");
    for name in names {
        fs::write(dir.join(name), content).expect("seed unit");
    }
}

/// Folds the two machine-specific values a generated schedule embeds: the
/// runtime root (in both its literal and its canonical form, since macOS
/// resolves `/var/...` to `/private/var/...`) and the CLI's own resolved
/// executable path.
fn fold_schedule_output(bytes: &[u8], root: &Path, resolved_binary: &Path) -> Vec<u8> {
    let text = fold_path(
        &String::from_utf8_lossy(bytes),
        resolved_binary,
        "<ORACLE_ROOT>/bin/symeraseme",
    );
    // Only the literal root is folded. The CLI resolves its working directory
    // through symlinks, so the payload can carry `/private<literal>`; the
    // phase-two capture folded exactly the literal value too, leaving the
    // `/private` prefix in place, and matching that keeps the bytes comparable.
    fold_path(&text, root, "<ORACLE_ROOT>").into_bytes()
}

/// Renders a digest as lowercase hex, matching Go's `%x`.
fn hex_digest(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Folds the volatile values a generated file can embed: the CLI's own resolved
/// executable path and the case root, in both its literal and its canonical form
/// (the CLI resolves its working directory through symlinks, so `/var/...`
/// surfaces as `/private/var/...`).
fn fold_volatile(value: &str, root: &Path, case_root: &Path, resolved_binary: &Path) -> String {
    let mut out = value.to_string();
    let fold = |needle: &Path, placeholder: &str, out: &mut String| {
        let literal = needle.to_string_lossy().to_string();
        if !literal.is_empty() {
            *out = fold_path(out, needle, placeholder);
        }
        if let Ok(canonical) = needle.canonicalize() {
            let canonical = canonical.to_string_lossy().to_string();
            if !canonical.is_empty() && canonical != literal {
                *out = out.replace(&canonical, placeholder);
            }
        }
    };
    fold(resolved_binary, "<BINARY>", &mut out);
    fold(case_root, "<CASE>", &mut out);
    fold(root, "<ROOT>", &mut out);
    out
}

/// Hashes every regular file under a root the way the oracle does: the oracle
/// folds the volatile paths before hashing, so this must fold identically or the
/// digests cannot agree.
fn hash_tree(
    root: &Path,
    case_root: &Path,
    resolved_binary: &Path,
) -> std::collections::BTreeMap<String, String> {
    use sha2::{Digest, Sha256};
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).into_iter().flatten().flatten() {
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
            let relative = path
                .strip_prefix(root)
                .expect("path under root")
                .to_string_lossy()
                .replace('\\', "/");
            let data = fs::read(&path).unwrap_or_default();
            let folded = fold_volatile(
                &String::from_utf8_lossy(&data),
                root,
                case_root,
                resolved_binary,
            );
            let mut hasher = Sha256::new();
            hasher.update(folded.as_bytes());
            out.insert(relative, hex_digest(&hasher.finalize()));
        }
    }
    out
}

/// The frozen schedule bytes were recorded on Unix. On Windows, execute the
/// same checked-in Go oracle rather than comparing against Unix path syntax.
#[cfg(windows)]
fn windows_schedule_fixture() -> Value {
    let root = unique_root();
    fs::create_dir_all(&root).expect("isolated oracle capture root");
    let _cleanup = Cleanup(root.clone());
    let stdout_path = root.join("go-schedule.stdout");
    let stderr_path = root.join("go-schedule.stderr");
    let stdout = fs::File::create(&stdout_path).expect("create Go stdout capture");
    let stderr = fs::File::create(&stderr_path).expect("create Go stderr capture");
    let mut command = Command::new("go");
    command
        .args(["run", "./rust-tests/parity/oracle/cli-schedule"])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .env("TMP", &root)
        .env("TEMP", &root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    configure_process_group(&mut command);
    let mut child = command.spawn().expect("pinned Go oracle starts");
    let started = Instant::now();
    let status = loop {
        if capture_exceeded(&stdout_path, &stderr_path) {
            terminate_bounded(&mut child).expect("oversized Go oracle cleanup");
            panic!("Go schedule oracle exceeded the capture limit");
        }
        if let Some(status) = child.try_wait().expect("poll Go oracle") {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(180) {
            terminate_bounded(&mut child).expect("timed-out Go oracle cleanup");
            panic!("Go schedule oracle exceeded 180 seconds");
        }
        thread::sleep(Duration::from_millis(25));
    };
    assert!(
        status.success(),
        "Go schedule oracle failed: {}",
        String::from_utf8_lossy(&read_bounded(&stderr_path))
    );
    serde_json::from_slice(&read_bounded(&stdout_path)).expect("Windows Go schedule fixture")
}

/// `schedule install/uninstall/status` answer the Go oracle's recorded bytes.
///
/// PATH is cleared so no real `launchctl`, `systemctl` or `crontab` can be
/// reached: the recorded answers are then the CLI's own deterministic ones and
/// this test never touches the host's scheduler state. The volatile install root
/// is folded to `<CASE>`/`<ROOT>` on both sides, so the comparison stays byte
/// exact everywhere else.
#[test]
fn schedule_commands_match_the_go_oracle() {
    #[cfg(windows)]
    let fixture = windows_schedule_fixture();
    #[cfg(not(windows))]
    let fixture: Value = serde_json::from_str(SCHEDULE_FIXTURE).expect("schedule fixture");
    assert_eq!(fixture["schema"], "symeraseme.go-oracle.cli.v1");
    assert_eq!(
        fixture["source"],
        "cmd/symeraseme/extra_commands.go:512-594"
    );
    let cases = fixture["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 13, "the fixture lost cases");
    #[cfg(windows)]
    {
        let frozen: Value = serde_json::from_str(SCHEDULE_FIXTURE).expect("frozen case inventory");
        let ids = |fixture: &Value| {
            fixture["cases"]
                .as_array()
                .expect("cases")
                .iter()
                .map(|case| case["id"].as_str().expect("case id").to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&fixture), ids(&frozen), "Windows oracle case inventory");
    }

    let root = unique_root();
    let capture = root.join("capture");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root.clone());

    let mut checked = 0;
    for (index, case) in cases.iter().enumerate() {
        let id = case["id"].as_str().expect("case id");
        let argv = case["argv"]
            .as_array()
            .expect("argv")
            .iter()
            .map(|value| value.as_str().expect("argv entry"))
            .collect::<Vec<_>>();
        let case_root = root.join(format!("case-{id}"));
        let home = case_root.join("home");
        fs::create_dir_all(&home).expect("isolated home");
        seed_schedule_case(&home, id);

        // The CLI derives its default project directory from the working
        // directory, so the case runs inside its own root exactly like the Go
        // oracle does; that keeps the generated wrappers comparable.
        let output = run(&argv, &home, &case_root, &capture, index);
        let expected_stdout = decode_base64(case["stdout_base64"].as_str().expect("stdout"));
        let expected_stderr = decode_base64(case["stderr_base64"].as_str().expect("stderr"));
        let expected_code = case["exit_code"].as_i64().expect("exit code");

        // The CLI embeds its own resolved executable path, which is
        // machine-specific by design; it is folded to a named placeholder here
        // and its format is asserted separately below. Everything else is
        // compared byte for byte.
        let resolved_binary = binary();
        let fold = |bytes: &[u8]| -> Vec<u8> {
            let text = String::from_utf8_lossy(bytes).into_owned();
            let text = fold_path(&text, &resolved_binary, "<BINARY>");
            let text = fold_path(&text, &case_root, "<CASE>");
            fold_path(&text, &root, "<ROOT>").into_bytes()
        };
        assert_eq!(
            fold(&output.stdout),
            expected_stdout,
            "{id}: stdout differs"
        );
        assert_eq!(
            fold(&output.stderr),
            expected_stderr,
            "{id}: stderr differs"
        );
        assert_eq!(
            output.status.code(),
            Some(expected_code as i32),
            "{id}: exit code differs"
        );

        // The masked binary path is not simply trusted: it must still be an
        // absolute path naming the CLI, so masking cannot hide a broken value.
        if case["normalized_fields"]
            .as_array()
            .is_some_and(|fields| fields.iter().any(|field| field == "binary_path"))
        {
            assert!(
                resolved_binary.is_absolute(),
                "{id}: the resolved binary path is absolute"
            );
            assert!(
                resolved_binary
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("symeraseme")),
                "{id}: the resolved binary path names the CLI"
            );
            let expected_stdout_text = String::from_utf8_lossy(&expected_stdout).into_owned();
            assert!(
                expected_stdout_text.contains("<BINARY>"),
                "{id}: the capture carries the masked binary path"
            );
        }

        let expected_files: std::collections::BTreeMap<String, String> =
            serde_json::from_value(case["files"].clone()).expect("files shape");
        assert_eq!(
            hash_tree(&case_root, &case_root, &resolved_binary),
            expected_files,
            "{id}: the written files differ"
        );
        checked += 1;
    }
    assert_eq!(checked, cases.len(), "every case was replayed");
}

#[test]
fn init_profile_round_trips_through_show_profile() {
    const KEY: [(&str, &str); 1] = [(
        "SYMERASEME_IDENTITY_MASTER_KEY",
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
    )];
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    let data_dir = root.join("data");
    for directory in [&home, &cwd, &capture, &data_dir] {
        fs::create_dir_all(directory).expect("isolated directory");
    }
    let _cleanup = Cleanup(root.clone());

    let output = run_with_data_dir(
        &[
            "init-profile",
            "--full-name",
            " Oracle User ",
            "--email",
            " oracle@example.invalid ",
        ],
        &home,
        &cwd,
        &capture,
        0,
        &data_dir,
        &KEY,
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        format!(
            "identity profile saved at {}\n",
            data_dir.join("identity.encrypted").display()
        )
        .into_bytes()
    );

    let output = run_with_data_dir(
        &["show-profile", "--output", "json"],
        &home,
        &cwd,
        &capture,
        1,
        &data_dir,
        &KEY,
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        b"{\"full_name\":\"Oracle User\",\"name_variants\":null,\"date_of_birth\":null,\"addresses\":[],\"email_addresses\":[\"oracle@example.invalid\"],\"phone_numbers\":null,\"jurisdictions\":null}\n"
    );

    let output = run_with_data_dir(
        &["init-profile", "--full-name", "Oracle User"],
        &home,
        &cwd,
        &capture,
        2,
        &data_dir,
        &KEY,
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stderr, b"--full-name and --email are required\n");

    let output = run_with_data_dir(
        &[
            "init-profile",
            "--full-name",
            "Oracle User",
            "--email",
            "Oracle User <oracle@example.invalid>",
        ],
        &home,
        &cwd,
        &capture,
        3,
        &data_dir,
        &KEY,
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stderr, b"invalid email address\n");
}

/// Seeds one manual task and one artifact file, then replays the populated
/// paths of `manual-tasks show`, `complete` and `cleanup`.
///
/// The recorded corpus only exercises the absent task and the absent artifact
/// directory, so the populated branches of Go's `HandleShow`, `HandleComplete`
/// and `HandleCleanup` are covered here instead.
#[test]
fn manual_tasks_populated_paths_match_the_go_bodies() {
    use chrono::{DateTime, Utc};

    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    let data_dir = root.join("data");
    let tasks_dir = data_dir.join("manual_tasks");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    fs::create_dir_all(&tasks_dir).expect("isolated artifact directory");
    let _cleanup = Cleanup(root.clone());

    let now = "2026-01-02T03:04:05Z"
        .parse::<DateTime<Utc>>()
        .expect("fixed instant");
    {
        let store = symeraseme_core::storage::Store::open(data_dir.join("symeraseme.db"))
            .expect("open the seeded store");
        symeraseme_core::manualtasks::create(
            &store,
            &symeraseme_core::manualtasks::CreateOpts {
                broker_id: "acme".to_owned(),
                broker_name: "Acme Data".to_owned(),
                form_url: "https://acme.example/optout".to_owned(),
                reason: "captcha_failed".to_owned(),
                ..symeraseme_core::manualtasks::CreateOpts::default()
            },
            None,
            now,
        )
        .expect("seed a manual task");
    }
    fs::write(tasks_dir.join("task-1.png"), b"artifact").expect("seed an artifact");

    let show = run_with_data_dir(
        &["manual-tasks", "show", "1", "--output", "json"],
        &home,
        &cwd,
        &capture,
        900,
        &data_dir,
        &[],
    );
    assert_eq!(show.status.code(), Some(0), "show status");
    assert!(show.stderr.is_empty(), "show stderr");
    let payload: Value = serde_json::from_slice(&show.stdout).expect("show payload");
    assert_eq!(payload["success"], true);
    assert_eq!(payload["id"], 1);
    assert_eq!(payload["broker_name"], "Acme Data");
    assert_eq!(payload["status"], "pending");
    // Go's SQL driver renders the `TIMESTAMP` columns as RFC3339 in UTC.
    let created_at = payload["created_at"].as_str().expect("created_at");
    assert!(
        created_at.ends_with('Z') && created_at.parse::<chrono::DateTime<chrono::Utc>>().is_ok(),
        "created_at is an RFC3339 instant: {created_at}"
    );
    let message = payload["message"].as_str().expect("show message");
    assert_eq!(
        message,
        format!(
            "Manual task #1:\n  Broker:     Acme Data (acme)\n  URL:        https://acme.example/optout\n  Reason:     captcha_failed\n  Status:     pending\n  Created:    {created_at}\n\nInstructions:\nThe CAPTCHA solver failed for Acme Data's opt-out form. Please visit the URL below and complete the CAPTCHA manually."
        ),
        "show message"
    );

    // Go's `manual-tasks show` reads `--task-id` when no positional is given.
    let show_flag = run_with_data_dir(
        &["manual-tasks", "show", "--task-id", "1", "--output", "json"],
        &home,
        &cwd,
        &capture,
        901,
        &data_dir,
        &[],
    );
    assert_eq!(show_flag.stdout, show.stdout, "show --task-id stdout");

    // Go's `intArgument` rejects a non-numeric positional before the handler.
    let invalid = run_with_data_dir(
        &["manual-tasks", "show", "abc", "--output", "json"],
        &home,
        &cwd,
        &capture,
        902,
        &data_dir,
        &[],
    );
    assert_eq!(invalid.status.code(), Some(1), "invalid task ID status");
    assert_eq!(invalid.stdout, b"", "invalid task ID stdout");
    assert_eq!(
        invalid.stderr, b"invalid task ID \"abc\"\n",
        "invalid task ID stderr"
    );

    let complete = run_with_data_dir(
        &[
            "manual-tasks",
            "complete",
            "1",
            "--notes",
            "done by hand",
            "--output",
            "json",
        ],
        &home,
        &cwd,
        &capture,
        903,
        &data_dir,
        &[],
    );
    assert_eq!(complete.status.code(), Some(0), "complete status");
    assert_eq!(
        complete.stdout,
        b"{\"message\":\"Manual task #1 marked as completed.\",\"success\":true,\"task_id\":1}\n",
        "complete stdout"
    );

    // The completion is persisted: `show` now reports it with the notes.
    let after: Value = serde_json::from_slice(
        &run_with_data_dir(
            &["manual-tasks", "show", "1", "--output", "json"],
            &home,
            &cwd,
            &capture,
            904,
            &data_dir,
            &[],
        )
        .stdout,
    )
    .expect("show payload after completion");
    assert_eq!(after["status"], "completed");
    assert_eq!(after["notes"], "done by hand");
    let completed_at = after["completed_at"].as_str().expect("completed_at");
    let after_message = after["message"].as_str().expect("message");
    assert!(
        after_message.contains(&format!("\n  Completed:  {completed_at}\n")),
        "completed show message: {after_message}"
    );
    assert!(
        after_message.ends_with("\n\nNotes: done by hand"),
        "completed show message: {after_message}"
    );

    // `--dry-run` counts the artifact without removing it.
    let dry_run = run_with_data_dir(
        &["manual-tasks", "cleanup", "--dry-run", "--output", "json"],
        &home,
        &cwd,
        &capture,
        905,
        &data_dir,
        &[],
    );
    assert_eq!(dry_run.status.code(), Some(0), "cleanup dry-run status");
    assert_eq!(
        dry_run.stdout,
        format!(
            "{{\"dry_run\":true,\"message\":{},\"removed\":0,\"skipped\":1,\"success\":true}}\n",
            serde_json::to_string(&format!(
                "Would remove 1 artifact(s) from {}. Use --yes to confirm.",
                tasks_dir.display()
            ))
            .expect("JSON message")
        )
        .into_bytes(),
        "cleanup dry-run stdout"
    );
    assert!(
        tasks_dir.join("task-1.png").exists(),
        "dry run kept the file"
    );

    let removed = run_with_data_dir(
        &["manual-tasks", "cleanup", "--output", "json"],
        &home,
        &cwd,
        &capture,
        906,
        &data_dir,
        &[],
    );
    assert_eq!(
        removed.stdout,
        format!(
            "{{\"dry_run\":false,\"message\":{},\"removed\":1,\"skipped\":0,\"success\":true}}\n",
            serde_json::to_string(&format!(
                "Removed 1 artifact(s) from {}.",
                tasks_dir.display()
            ))
            .expect("JSON message")
        )
        .into_bytes(),
        "cleanup stdout"
    );
    assert!(
        !tasks_dir.join("task-1.png").exists(),
        "cleanup removed the file"
    );

    // Go's text mode prints the contract's `success` line for all three.
    for argv in [
        vec!["manual-tasks", "show", "1"],
        vec!["manual-tasks", "complete", "1"],
        vec!["manual-tasks", "cleanup", "--dry-run"],
    ] {
        let text = run_with_data_dir(&argv, &home, &cwd, &capture, 907, &data_dir, &[]);
        assert_eq!(text.stdout, b"success\n", "{argv:?} text stdout");
        assert_eq!(text.status.code(), Some(0), "{argv:?} text status");
    }
}

/// `manual-tasks show` renders the `TIMESTAMP` columns the way Go's SQL driver
/// does, for every layout that driver accepts.
///
/// Each expectation was taken from the built Go binary reading the same stored
/// value. Two of the driver's layouts carry a numeric zone offset and differ
/// only in their `T`/space separator, Go's `time.Parse` also accepts `Z` where
/// a layout writes `-07:00`, and a parsed offset survives into the rendered
/// value — only the naive layouts report `Z`.
#[test]
fn manual_task_timestamps_match_the_go_sql_driver() {
    use chrono::{DateTime, Utc};

    const CASES: [(&str, &str); 16] = [
        ("2026-09-21 18:39:46+02:00", "2026-09-21T18:39:46+02:00"),
        ("2026-09-21T18:39:46+02:00", "2026-09-21T18:39:46+02:00"),
        ("2026-09-21T18:39:46Z", "2026-09-21T18:39:46Z"),
        ("2026-09-21 18:39:46Z", "2026-09-21T18:39:46Z"),
        ("2026-09-21 18:39:46+00:00", "2026-09-21T18:39:46Z"),
        ("2026-09-21 18:39:46.5+02:00", "2026-09-21T18:39:46.5+02:00"),
        (
            "2026-09-21T18:39:46.500000000+02:00",
            "2026-09-21T18:39:46.5+02:00",
        ),
        ("2026-09-21 18:39:46-05:30", "2026-09-21T18:39:46-05:30"),
        ("2026-09-21 18:39:46.123", "2026-09-21T18:39:46.123Z"),
        ("2026-09-21 18:39:46", "2026-09-21T18:39:46Z"),
        ("2026-09-21T18:39:46", "2026-09-21T18:39:46Z"),
        ("2026-09-21 18:39", "2026-09-21T18:39:00Z"),
        ("2026-09-21T18:39", "2026-09-21T18:39:00Z"),
        ("2026-09-21", "2026-09-21T00:00:00Z"),
        // Neither the short offset nor a non-instant matches a layout, so Go
        // hands the stored text back untouched.
        ("2026-09-21T18:39:46+02", "2026-09-21T18:39:46+02"),
        ("not a time", "not a time"),
    ];

    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    let data_dir = root.join("data");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    fs::create_dir_all(&data_dir).expect("isolated data directory");
    let _cleanup = Cleanup(root.clone());

    let database = data_dir.join("symeraseme.db");
    let now = "2026-01-02T03:04:05Z"
        .parse::<DateTime<Utc>>()
        .expect("fixed instant");
    let store = symeraseme_core::storage::Store::open(database).expect("open the seeded store");
    symeraseme_core::manualtasks::create(
        &store,
        &symeraseme_core::manualtasks::CreateOpts {
            broker_id: "acme".to_owned(),
            broker_name: "Acme Data".to_owned(),
            reason: "captcha_failed".to_owned(),
            ..symeraseme_core::manualtasks::CreateOpts::default()
        },
        None,
        now,
    )
    .expect("seed a manual task");

    for (index, (stored, expected)) in CASES.iter().enumerate() {
        store
            .connection()
            .execute(
                "UPDATE manual_tasks SET created_at = ? WHERE id = 1",
                [stored],
            )
            .expect("store the timestamp");
        let output = run_with_data_dir(
            &["manual-tasks", "show", "1", "--output", "json"],
            &home,
            &cwd,
            &capture,
            920 + index,
            &data_dir,
            &[],
        );
        let payload: Value = serde_json::from_slice(&output.stdout).expect("show payload");
        assert_eq!(payload["created_at"], *expected, "stored {stored:?}");
        assert!(
            payload["message"]
                .as_str()
                .expect("message")
                .contains(&format!("\n  Created:    {expected}\n")),
            "stored {stored:?} message"
        );
    }
}

/// A two-broker registry: one email-first broker and one web-form-first broker.
fn plan_resources(root: &Path) -> std::path::PathBuf {
    let resources = root.join("resources");
    fs::create_dir_all(resources.join("brokers/us")).expect("broker directory");
    fs::create_dir_all(resources.join("schemas")).expect("schema directory");
    fs::write(
        resources.join("manifest.json"),
        br#"{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}"#,
    )
    .expect("registry manifest");
    fs::write(
        resources.join("schemas/broker.schema.json"),
        br#"{"schema_version":1}"#,
    )
    .expect("broker schema");
    fs::write(
        resources.join("brokers/us/aa-mail-us.yaml"),
        b"id: aa-mail-us\nname: AA Mail\nwebsite: https://aa.example\ncategory: other\njurisdictions:\n- US\nlaws:\n- CCPA\ndata_sensitivity: 3\npriority: medium\nopt_out:\n- type: email\n  endpoint: privacy@aa.example\n  template: ccpa-deletion\n  locale: en\n  expected_response_days: 45\nadded_date: '2025-05-21'\nstatus: active\n".as_slice(),
    )
    .expect("email broker");
    fs::write(
        resources.join("brokers/us/zz-form-us.yaml"),
        b"id: zz-form-us\nname: ZZ Form\nwebsite: https://zz.example\ncategory: other\njurisdictions:\n- US\nlaws:\n- CCPA\ndata_sensitivity: 3\npriority: medium\nopt_out:\n- type: web_form\n  url: https://zz.example/optout\n  form_spec:\n    steps:\n    - goto: https://zz.example/optout\nadded_date: '2025-05-21'\nstatus: active\n".as_slice(),
    )
    .expect("web form broker");
    resources
}

/// The populated `plan create`/`show`/`execute` paths, which the oracle records
/// only in their empty state.
///
/// One isolated store carries the whole sequence, because the commands are
/// written to build on each other: `create` plans, `show` reads the plan back
/// and `execute` consumes it. The registry is a two-broker override so the email
/// and web-form branches are both exercised with known values.
#[test]
fn plan_populated_paths_match_the_go_bodies() {
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    let data_dir = root.join("data");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    fs::create_dir_all(&data_dir).expect("isolated data directory");
    let resources = plan_resources(&root);
    let _cleanup = Cleanup(root.clone());

    let resources = resources.to_string_lossy().to_string();
    let environment: Vec<(&str, &str)> = vec![
        ("SYMERASEME_RESOURCES", resources.as_str()),
        (
            "SYMERASEME_IDENTITY_MASTER_KEY",
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        ),
    ];
    let mut step = 0;
    let mut plan = |argv: &[&str]| {
        step += 1;
        run_with_data_dir(
            argv,
            &home,
            &cwd,
            &capture,
            900 + step,
            &data_dir,
            &environment,
        )
    };

    let output = plan(&[
        "init-profile",
        "--full-name",
        "Jane Doe",
        "--email",
        "jane@example.com",
    ]);
    assert_eq!(output.status.code(), Some(0), "init-profile");

    // `create` plans both brokers: the email channel resolves a template, the
    // web-form channel resolves none.
    let created = json_stdout(&plan(&[
        "--output",
        "json",
        "plan",
        "create",
        "--campaign",
        "pop",
        "--max",
        "5",
    ]));
    assert_eq!(created["total_brokers"], 2);
    assert_eq!(created["matched"], 2);
    assert_eq!(created["planned"], 2);
    let requests = created["requests"].as_array().expect("planned requests");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["broker_id"], "aa-mail-us");
    assert_eq!(requests[0]["channel"], "email");
    assert_eq!(requests[0]["template"], "ccpa-deletion.en.md.j2");
    assert_eq!(requests[1]["broker_id"], "zz-form-us");
    assert_eq!(requests[1]["channel"], "web_form");
    assert_eq!(requests[1]["template"], "");

    // `--max` bounds the planned requests, never the match count.
    let capped = json_stdout(&plan(&[
        "--output",
        "json",
        "plan",
        "create",
        "--campaign",
        "capped",
        "--max",
        "1",
    ]));
    assert_eq!(capped["matched"], 2);
    assert_eq!(capped["planned"], 1);

    // A filter that matches nothing keeps Go's nil slice: `null`, not `[]`.
    let empty = json_stdout(&plan(&[
        "--output",
        "json",
        "plan",
        "create",
        "--campaign",
        "empty",
        "--category",
        "people-search",
    ]));
    assert_eq!(empty["planned"], 0);
    assert_eq!(empty["requests"], Value::Null);

    // `show` reads the plan back, filtered by campaign and by status.
    let shown = json_stdout(&plan(&[
        "--output",
        "json",
        "plan",
        "show",
        "--campaign",
        "pop",
    ]));
    assert_eq!(shown["campaign_id"], "pop");
    assert_eq!(shown["total"], 2);
    let rows = shown["requests"].as_array().expect("shown requests");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["broker_id"], "aa-mail-us");
    assert_eq!(rows[0]["current_status"], "PLANNED");
    let filtered = json_stdout(&plan(&[
        "--output",
        "json",
        "plan",
        "show",
        "--campaign",
        "pop",
        "--status",
        "SENT",
    ]));
    assert_eq!(filtered["total"], 0);
    assert_eq!(filtered["requests"], Value::Null);
    let all = json_stdout(&plan(&["--output", "json", "plan", "show"]));
    assert_eq!(all["campaign_id"], "all");
    assert_eq!(all["total"], 3);

    // `execute --dry-run` previews both channels. The email branch renders Go's
    // placeholder body, because `realPlanCommand` injects no renderer.
    let executed = json_stdout(&plan(&[
        "--output",
        "json",
        "plan",
        "execute",
        "--campaign",
        "pop",
        "--dry-run",
    ]));
    assert_eq!(executed["campaign_id"], "pop");
    assert_eq!(executed["total_planned"], 2);
    assert_eq!(executed["batch_size"], 2);
    let results = executed["results"].as_array().expect("execute results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["success"], true);
    assert_eq!(results[0]["dry_run"], true);
    assert_eq!(results[0]["to"], "privacy@aa.example");
    // Go reads `req["broker_id"]` into `brokerName`, so the id is what reaches
    // the subject line and the rendered body.
    assert_eq!(results[0]["subject"], "Data Deletion Request — aa-mail-us");
    assert_eq!(
        results[0]["body"],
        "[template ccpa-deletion.en.md.j2 — aa-mail-us / Jane Doe]"
    );
    assert_eq!(results[1]["success"], true);
    assert_eq!(results[1]["broker_id"], "zz-form-us");
    assert_eq!(results[1]["url"], "https://zz.example/optout");
    // The allowlist drops the adapter's `steps` field on the way out.
    assert_eq!(results[1]["steps"], Value::Null);

    // The web-form branch records its outcome even in a dry run, which is what
    // takes that request out of the next batch; the email branch records
    // nothing, so it stays PLANNED.
    let after = json_stdout(&plan(&[
        "--output",
        "json",
        "plan",
        "show",
        "--campaign",
        "pop",
    ]));
    let rows = after["requests"]
        .as_array()
        .expect("requests after execute");
    assert_eq!(rows[0]["current_status"], "PLANNED");
    // The shared projection turns a SENT event into `AWAITING_ACK`.
    assert_eq!(rows[1]["current_status"], "AWAITING_ACK");

    // `--batch-size` bounds one batch without changing the planned total.
    let batched = json_stdout(&plan(&[
        "--output",
        "json",
        "plan",
        "execute",
        "--campaign",
        "capped",
        "--dry-run",
        "--batch-size",
        "0",
    ]));
    assert_eq!(batched["total_planned"], 1);
    assert_eq!(batched["batch_size"], 1);

    // An unknown campaign executes an empty batch, and `results` stays `[]`.
    let unknown = json_stdout(&plan(&[
        "--output",
        "json",
        "plan",
        "execute",
        "--campaign",
        "missing",
        "--dry-run",
    ]));
    assert_eq!(unknown["total_planned"], 0);
    assert_eq!(unknown["results"], Value::Array(Vec::new()));

    // Without a consent token a live run stops at the gate, before any adapter
    // is built — which is why no test here can reach a non-dry-run execution.
    let denied = plan(&["--output", "json", "plan", "execute", "--campaign", "pop"]);
    assert_eq!(denied.status.code(), Some(1), "consent gate exit code");
    assert_eq!(denied.stdout, b"", "consent gate stdout");
    assert_eq!(denied.stderr, b"identity: consent denied\n");

    // `--campaign` is required by both commands that declare it as mandatory.
    let missing = plan(&["plan", "create"]);
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(missing.stderr, b"--campaign is required\n");
}

/// The JSON document one successful command wrote to stdout.
fn json_stdout(output: &ProcessOutput) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout is one JSON document")
}

/// One manual task, with every column set to a fixed value so the whole payload
/// is deterministic.
const MANUAL_TASK_SEED: &str = "INSERT INTO manual_tasks \
     (id,request_id,broker_id,broker_name,form_url,reason,instructions,screenshot_path,\
      html_snapshot_path,form_fields_json,status,created_at,completed_at,notes) \
     VALUES (1,NULL,'acme','Acme Data','https://example.test/form','captcha','do it',\
      '/s.png','/s.html','{}','pending','2026-01-02T03:04:05Z',NULL,'note');";

/// The bytes the Go binary writes for that row, measured against an isolated
/// store. The recorded corpus case `operate-manual-tasks-list` runs on an empty
/// store, so only a populated list pins the key order of the nested task
/// objects: Go marshals them from the `manualtasks.ManualTask` struct and so
/// emits its declaration order, while the surrounding `Result` goes through a
/// Go map and is sorted.
const MANUAL_TASKS_LIST_GOLDEN: &str = concat!(
    r#"{"message":"Manual tasks (1):\n  #1 [pending] Acme Data (captcha) @ 2026-01-02T03:04:05Z","#,
    r#""success":true,"tasks":[{"id":1,"request_id":null,"broker_id":"acme","#,
    r#""broker_name":"Acme Data","form_url":"https://example.test/form","reason":"captcha","#,
    r#""instructions":"do it","screenshot_path":"/s.png","html_snapshot_path":"/s.html","#,
    r#""form_fields_json":"{}","status":"pending","created_at":"2026-01-02T03:04:05Z","#,
    r#""completed_at":null,"notes":"note"}]}"#,
    "\n"
);

#[test]
fn manual_tasks_list_matches_go_on_a_populated_store() {
    let root = unique_root();
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    let home = root.join("home");
    let data_dir = root.join("data");
    for directory in [&cwd, &capture, &home, &data_dir] {
        fs::create_dir_all(directory).expect("isolated directory");
    }
    let _cleanup = Cleanup(root.clone());

    let store = symeraseme_core::storage::Store::open(data_dir.join("symeraseme.db"))
        .expect("open the seeded store");
    store
        .connection()
        .execute_batch(MANUAL_TASK_SEED)
        .expect("seed one manual task");
    drop(store);

    let output = run_with_data_dir(
        &["manual-tasks", "list", "--output", "json"],
        &home,
        &cwd,
        &capture,
        0,
        &data_dir,
        &[],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        MANUAL_TASKS_LIST_GOLDEN
    );
}

/// `events show` and `grant` on the paths the oracle corpus cannot reach.
///
/// The recorded cases are all absent/dry-run states, so the populated event
/// list and the real issue/revoke/list token paths are asserted here. Both run
/// against an isolated HOME and `SYMERASEME_DATA_DIR`: the consent tokens are
/// files under that data directory and never reach a keychain.
#[test]
fn events_and_grant_populated_paths_match_the_go_bodies() {
    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    let data_dir = root.join("data");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    fs::create_dir_all(&data_dir).expect("isolated data directory");
    let _cleanup = Cleanup(root.clone());
    seed_store(&data_dir.join("symeraseme.db"));

    let go = |argv: &[&str], index: usize| {
        run_with_data_dir(argv, &home, &cwd, &capture, index, &data_dir, &[])
    };

    // Go marshals `[]eventstore.Event` itself, so the exported struct field
    // order survives and `<` is HTML-escaped by `json.Marshal`.
    let events = go(&["events", "show", "1", "--output", "json"], 920);
    assert_eq!(events.status.code(), Some(0), "events show status");
    assert!(events.stderr.is_empty(), "events show stderr");
    assert_eq!(
        events.stdout,
        concat!(
            r#"[{"ID":1,"RequestID":1,"OccurredAt":"2026-08-07T08:59:00Z","RecordedAt":"2026-08-07T08:59:01Z","EventType":"PLANNED","Payload":{},"Source":"system"},"#,
            r#"{"ID":2,"RequestID":1,"OccurredAt":"2026-08-07T09:00:00Z","RecordedAt":"2026-08-07T09:00:02Z","EventType":"SENT","Payload":{"message_id":"\u003csent-1@example.com\u003e"},"Source":"user"}]"#,
            "\n"
        )
        .as_bytes(),
        "events show stdout"
    );

    // `--after-event-id` drops the earlier event.
    let after = go(
        &[
            "events",
            "show",
            "1",
            "--after-event-id",
            "1",
            "--output",
            "json",
        ],
        921,
    );
    assert_eq!(
        after.stdout,
        concat!(
            r#"[{"ID":2,"RequestID":1,"OccurredAt":"2026-08-07T09:00:00Z","RecordedAt":"2026-08-07T09:00:02Z","EventType":"SENT","Payload":{"message_id":"\u003csent-1@example.com\u003e"},"Source":"user"}]"#,
            "\n"
        )
        .as_bytes(),
        "events show --after-event-id stdout"
    );

    // Go's `events show` reads `--request-id` when no positional is given.
    assert_eq!(
        go(
            &["events", "show", "--request-id", "1", "--output", "json"],
            922
        )
        .stdout,
        events.stdout,
        "events show --request-id stdout"
    );
    // A request with no events keeps Go's nil slice.
    assert_eq!(
        go(&["events", "show", "9", "--output", "json"], 923).stdout,
        b"null\n",
        "events show unknown request stdout"
    );
    // Text mode never renders the list.
    assert_eq!(
        go(&["events", "show", "1"], 924).stdout,
        b"success\n",
        "events show text stdout"
    );
    // Go's `intArgument` rejects a non-numeric positional before the handler.
    let invalid = go(&["events", "show", "abc", "--output", "json"], 925);
    assert_eq!(invalid.status.code(), Some(1), "invalid request ID status");
    assert_eq!(
        invalid.stderr, b"invalid request ID \"abc\"\n",
        "invalid request ID stderr"
    );

    // `grant` without `--dry-run` issues a token into the data directory.
    let issued = go(&["grant", "--output", "json"], 926);
    assert_eq!(issued.status.code(), Some(0), "grant issue status");
    let issued: Value = serde_json::from_slice(&issued.stdout).expect("grant issue payload");
    assert_eq!(issued["success"], true);
    let first = issued["token"].as_str().expect("issued token").to_owned();
    assert!(!first.is_empty(), "grant issues a token value");
    let consent = directory_entries(&data_dir, "read isolated data directory")
        .into_iter()
        .filter(|name| name.starts_with("consent_"))
        .count();
    assert_eq!(consent, 1, "the token is stored in the isolated data dir");

    // The positional overrides `--command`, and `--ttl` reaches the record.
    let second = go(
        &["grant", "send-removal", "--ttl", "60", "--output", "json"],
        927,
    );
    let second: Value = serde_json::from_slice(&second.stdout).expect("grant issue payload");
    let second = second["token"].as_str().expect("issued token").to_owned();

    // `--list` is Go's second name for `--list-tokens`, and Go marshals
    // `[]ConsentToken` itself, so its struct field order survives.
    let listed = go(&["grant", "--list", "--output", "json"], 928);
    let body = String::from_utf8(listed.stdout.clone()).expect("UTF-8 listing");
    assert!(
        body.starts_with("{\"count\":2,\"success\":true,\"tokens\":["),
        "{body}"
    );
    let listed: Value = serde_json::from_slice(&listed.stdout).expect("grant list payload");
    let tokens = listed["tokens"].as_array().expect("tokens");
    // Both tokens are issued in the same second, and Go sorts on issued-at
    // alone, so only the membership is contractual here.
    let mut commands = tokens
        .iter()
        .map(|token| token["command"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    commands.sort_unstable();
    assert_eq!(commands, ["execute", "send-removal"], "listed commands");
    let short = tokens
        .iter()
        .find(|token| token["command"] == "send-removal")
        .expect("the positional reached the record");
    assert_eq!(
        short["expires_at"].as_i64().expect("expires_at")
            - short["issued_at"].as_i64().expect("issued_at"),
        60,
        "--ttl reaches the record"
    );
    // Text mode never renders the listing.
    assert_eq!(
        go(&["grant", "--list-tokens"], 929).stdout,
        b"success\n",
        "grant list text stdout"
    );

    // A single revoke reports one removal; an unknown token fails closed with
    // Go's own wording rather than the identity package's.
    assert_eq!(
        go(&["grant", "--revoke", &first, "--output", "json"], 930).stdout,
        b"{\"revoked\":1,\"success\":true}\n",
        "grant revoke stdout"
    );
    let missing = go(&["grant", "--revoke", &first, "--output", "json"], 931);
    assert_eq!(
        missing.status.code(),
        Some(1),
        "grant revoke missing status"
    );
    assert_eq!(missing.stdout, b"", "grant revoke missing stdout");
    assert_eq!(
        missing.stderr, b"consent token not found\n",
        "grant revoke missing stderr"
    );

    // `--revoke-all` clears the rest, and the empty listing is Go's nil slice.
    assert_eq!(
        go(&["grant", "--revoke-all", "--output", "json"], 932).stdout,
        b"{\"revoke_all\":true,\"revoked\":1,\"success\":true}\n",
        "grant revoke-all stdout"
    );
    assert_eq!(
        go(&["grant", "--list-tokens", "--output", "json"], 933).stdout,
        b"{\"count\":0,\"success\":true,\"tokens\":null}\n",
        "grant empty list stdout"
    );
    assert!(!second.is_empty(), "the second token was issued");

    let home_entries = directory_entries(&home, "read isolated home");
    assert!(
        home_entries.is_empty(),
        "isolated home stayed clean: {home_entries:?}"
    );
}
