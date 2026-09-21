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

fn run_with_resources(
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

    let mut command = Command::new(binary());
    command
        .args(argv)
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
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
) -> ProcessOutput {
    let stdout_path = capture.join(format!("tick-status-{index}.stdout"));
    let stderr_path = capture.join(format!("tick-status-{index}.stderr"));
    let stdout = fs::File::create(&stdout_path).expect("create stdout capture");
    let stderr = fs::File::create(&stderr_path).expect("create stderr capture");

    let mut command = Command::new(binary());
    command
        .args(argv)
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("PWD", cwd)
        .env("SYMERASEME_DATA_DIR", data_dir)
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

/// Replaces the reported instant with the fixture's placeholder.
///
/// The masked value is parsed as an instant first, so masking cannot hide a
/// difference in the format Go reports.
fn mask_wall_clock(payload: &[u8]) -> Vec<u8> {
    let text = String::from_utf8(payload.to_vec()).expect("UTF-8 payload");
    let key = "\"as_of\":\"";
    let start = text.find(key).expect("the payload reports as_of") + key.len();
    let end = start + text[start..].find('"').expect("a terminated as_of value");
    let instant = &text[start..end];
    chrono::DateTime::parse_from_rfc3339(instant).expect("as_of is an RFC 3339 instant");
    // `text[..start]` already ends with the value's opening quote.
    format!("{}<TIMESTAMP>\"{}", &text[..start], &text[end + 1..]).into_bytes()
}

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

        let output = run_with_data_dir(&argv, &home, &cwd, &capture, index, &data_dir);
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
    const SURFACE_OPERATIONS: [&str; 13] = [
        "operate-brokers-list",
        "operate-plan-status",
        "operate-plan-tick",
        "operate-brokers-show",
        "operate-completion",
        "operate-config-show",
        "operate-help",
        "operate-render-template",
        "operate-serve",
        "operate-schedule-install",
        "operate-schedule-status",
        "operate-schedule-uninstall",
        "operate-version",
    ];
    // `registry list`/`validate` are replayed now that cli.rs implements them.
    // They carry the registry contract that `brokers list` cannot: no status
    // filter and no `filters` object, so the count is 1,277 rather than 1,273.
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
    assert_eq!(cases.len(), 166);
    let selected = cases
        .iter()
        .filter(|case| is_exact_case(case))
        .collect::<Vec<_>>();
    let deferred = cases
        .iter()
        .filter(|case| !is_exact_case(case))
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 133);
    assert_eq!(deferred.len(), 33);

    let root = unique_root();
    let home = root.join("home");
    let cwd = root.join("cwd");
    let capture = root.join("capture");
    fs::create_dir_all(&home).expect("isolated home");
    fs::create_dir_all(&cwd).expect("isolated cwd");
    fs::create_dir_all(&capture).expect("capture directory");
    let _cleanup = Cleanup(root.clone());

    for (index, case) in selected.iter().enumerate() {
        let argv = case["argv"]
            .as_array()
            .expect("argv")
            .iter()
            .map(|arg| arg.as_str().expect("string argv"))
            .collect::<Vec<_>>();
        let id = case["id"].as_str().unwrap_or("");
        // The store-backed operations read the event store, so they run against
        // their own data directory: the shared isolated HOME has to stay empty
        // for the assertion below.
        let store_backed = matches!(id, "operate-plan-status" | "operate-plan-tick");
        // The phase-two capture runs every schedule case in its own
        // `cli/<id>/cwd`, so the generated wrappers record that directory; the
        // replay has to run in the same layout or the folded paths differ.
        let case_cwd = if id.starts_with("operate-schedule") {
            let dir = root.join("cli").join(id).join("cwd");
            fs::create_dir_all(&dir).expect("isolated schedule cwd");
            dir
        } else {
            cwd.clone()
        };
        let output = if store_backed {
            let data_dir = root.join(format!("data-{id}"));
            fs::create_dir_all(&data_dir).expect("isolated data directory");
            run_with_data_dir(&argv, &home, &case_cwd, &capture, index, &data_dir)
        } else {
            run(&argv, &home, &case_cwd, &capture, index)
        };
        assert_eq!(
            output.status.code(),
            Some(case["exit_code"].as_i64().unwrap() as i32),
            "{id} status"
        );
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
            _ => decode_base64(case["stdout_base64"].as_str().unwrap()),
        };
        // `plan status` reports a wall clock, so its value is masked before the
        // comparison — the format itself is still asserted by the masker.
        let actual_stdout = if id == "operate-plan-status" {
            mask_wall_clock(&output.stdout)
        } else if id.starts_with("operate-schedule") {
            // The generated wrappers embed the working directory and the CLI's
            // own resolved executable path; the phase-two capture folds both to
            // `<ORACLE_ROOT>` (the binary as `<ORACLE_ROOT>/bin/symeraseme`).
            // Folding the same values here keeps the rest byte exact.
            fold_schedule_output(&output.stdout, &root, &binary())
        } else {
            output.stdout.clone()
        };
        assert_eq!(actual_stdout, expected_stdout, "{id} stdout");
        assert_eq!(
            output.stderr,
            decode_base64(case["stderr_base64"].as_str().unwrap()),
            "{id} stderr"
        );
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
    assert!(
        home_entries.is_empty(),
        "isolated home stayed clean: {home_entries:?}"
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
    fs::create_dir_all(resources.join("brokers")).expect("broker directory");
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
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    let binary = resolved_binary.to_string_lossy().to_string();
    if !binary.is_empty() {
        text = text.replace(&binary, "<ORACLE_ROOT>/bin/symeraseme");
    }
    // Only the literal root is folded. The CLI resolves its working directory
    // through symlinks, so the payload can carry `/private<literal>`; the
    // phase-two capture folded exactly the literal value too, leaving the
    // `/private` prefix in place, and matching that keeps the bytes comparable.
    text.replace(&root.to_string_lossy().to_string(), "<ORACLE_ROOT>")
        .into_bytes()
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
            *out = out.replace(&literal, placeholder);
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

/// `schedule install/uninstall/status` answer the Go oracle's recorded bytes.
///
/// PATH is cleared so no real `launchctl`, `systemctl` or `crontab` can be
/// reached: the recorded answers are then the CLI's own deterministic ones and
/// this test never touches the host's scheduler state. The volatile install root
/// is folded to `<CASE>`/`<ROOT>` on both sides, so the comparison stays byte
/// exact everywhere else.
#[test]
fn schedule_commands_match_the_go_oracle() {
    let fixture: Value = serde_json::from_str(SCHEDULE_FIXTURE).expect("schedule fixture");
    assert_eq!(fixture["schema"], "symeraseme.go-oracle.cli.v1");
    assert_eq!(
        fixture["source"],
        "cmd/symeraseme/extra_commands.go:512-594"
    );
    let cases = fixture["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 13, "the fixture lost cases");

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
            text.replace(&resolved_binary.to_string_lossy().to_string(), "<BINARY>")
                .replace(&case_root.to_string_lossy().to_string(), "<CASE>")
                .replace(&root.to_string_lossy().to_string(), "<ROOT>")
                .into_bytes()
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
