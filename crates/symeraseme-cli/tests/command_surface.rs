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
    const SURFACE_OPERATIONS: [&str; 10] = [
        "operate-brokers-list",
        "operate-plan-status",
        "operate-plan-tick",
        "operate-brokers-show",
        "operate-completion",
        "operate-config-show",
        "operate-help",
        "operate-render-template",
        "operate-serve",
        "operate-version",
    ];
    matches!(
        case["category"].as_str(),
        Some("help" | "unknown_flag" | "missing_argument" | "root_version" | "unknown_command")
    ) || case["category"] == "success" && PHASE_SUCCESS.contains(&case["id"].as_str().unwrap_or(""))
        || SURFACE_OPERATIONS.contains(&case["id"].as_str().unwrap_or(""))
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
    assert_eq!(cases.len(), 165);
    let selected = cases
        .iter()
        .filter(|case| is_exact_case(case))
        .collect::<Vec<_>>();
    let deferred = cases
        .iter()
        .filter(|case| !is_exact_case(case))
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 125);
    assert_eq!(deferred.len(), 40);

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
        let output = if store_backed {
            let data_dir = root.join(format!("data-{id}"));
            fs::create_dir_all(&data_dir).expect("isolated data directory");
            run_with_data_dir(&argv, &home, &cwd, &capture, index, &data_dir)
        } else {
            run(&argv, &home, &cwd, &capture, index)
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
