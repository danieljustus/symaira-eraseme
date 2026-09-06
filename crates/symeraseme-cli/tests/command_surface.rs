use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
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
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "symeraseme-cli-surface-{}-{nonce}",
        std::process::id()
    ))
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
    const SURFACE_OPERATIONS: [&str; 5] = [
        "operate-completion",
        "operate-config-show",
        "operate-help",
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
    assert_eq!(selected.len(), 120);
    assert_eq!(deferred.len(), 45);

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
        let output = run(&argv, &home, &cwd, &capture, index);
        let id = case["id"].as_str().unwrap_or("");
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
        assert_eq!(output.stdout, expected_stdout, "{id} stdout");
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

    assert!(
        fs::read_dir(&home)
            .expect("read isolated home")
            .next()
            .is_none()
    );
    assert!(
        fs::read_dir(&cwd)
            .expect("read isolated cwd")
            .next()
            .is_none()
    );
}
