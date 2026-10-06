//! Actual Go/Rust bind failures and Windows token DACLs in disposable roots.
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[path = "support/mcp_http_port.rs"]
#[allow(dead_code)] // Windows-only controls use additional shared process helpers.
mod mcp_http_port;
use mcp_http_port::StartedChild;
#[path = "support/capture_http_aux.rs"]
#[allow(dead_code)] // Only the Unix process comparators retain raw HTTP bodies.
mod capture_http_aux;
#[path = "support/frozen_http_aux.rs"]
#[allow(dead_code)]
mod frozen_http_aux;

const THIS_FILE: &str = "crates/symeraseme-cli/tests/mcp_http_native_contract.rs";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn private(command: &mut Command, root: &Path) {
    fs::create_dir_all(root).unwrap();
    command
        .env_clear()
        .current_dir(root)
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("SYMERASEME_DATA_DIR", root.join("data"))
        .env("TMPDIR", root)
        .env("TMP", root)
        .env("TEMP", root);
    for key in ["CONFIG", "DATA", "STATE", "CACHE"] {
        command.env(format!("XDG_{key}_HOME"), root.join(key.to_lowercase()));
    }
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn capture(mut command: Command, logs: &Path, label: &str, budget: Duration) -> Output {
    let stdout = logs.join(format!("{label}.stdout"));
    let stderr = logs.join(format!("{label}.stderr"));
    let mut child = StartedChild::from_child(
        command
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout).unwrap())
            .stderr(fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + budget;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            let diagnostic = if fs::metadata(&stderr).unwrap().len() <= 4096 {
                fs::read_to_string(&stderr).unwrap_or_default()
            } else {
                "diagnostic exceeds limit".into()
            };
            panic!("{label} deadline: {diagnostic}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    child.wait().unwrap();
    for path in [&stdout, &stderr] {
        assert!(fs::metadata(path).unwrap().len() <= 64 * 1024);
    }
    Output {
        status,
        stdout: fs::read(stdout).unwrap(),
        stderr: fs::read(stderr).unwrap(),
    }
}

fn oracle(logs: &Path) -> PathBuf {
    let binary = logs.join("go-mcp.exe");
    let mut command = Command::new("go");
    command
        .args(["build", "-o"])
        .arg(&binary)
        .arg("./cmd/symeraseme")
        .current_dir(repo())
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off");
    let result = capture(command, logs, "go-build", Duration::from_secs(180));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    binary
}

fn failed_start(
    binary: &Path,
    root: &Path,
    host: &str,
    port: u16,
    logs: &Path,
    label: &str,
) -> Output {
    let mut command = Command::new(binary);
    private(&mut command, root);
    command.args([
        "mcp",
        "--host",
        host,
        "--port",
        &port.to_string(),
        "--allow-remote",
    ]);
    let result = capture(command, logs, label, Duration::from_secs(10));
    assert!(!result.status.success(), "{label} unexpectedly listened");
    assert!(result.stdout.is_empty());
    // Token generation precedes bind in both actual CLIs; do not normalize
    // away a profile/store failure that prevented reaching the socket API.
    assert_eq!(fs::read(root.join("data/mcp_token")).unwrap().len(), 43);
    result
}

/// Actual Go startup failure, or the recorded one with this run's port.
fn go_failed_start(
    recorded: Option<&serde_json::Value>,
    go: Option<&Path>,
    measured: &mut Vec<serde_json::Value>,
    root: &Path,
    host: &str,
    port: u16,
    case: &str,
) -> (Option<i32>, Vec<u8>) {
    let label = format!("go-{case}");
    if let Some(recorded) = recorded {
        // failed_start's actual-Go side effects, as measured.
        assert_eq!(recorded["stdout_bytes"], 0);
        assert_eq!(recorded["token_bytes"], 43);
        let (code, stderr) = frozen_http_aux::startup_failure(recorded, case, port);
        assert_ne!(code, Some(0), "{label} unexpectedly listened");
        return (code, stderr);
    }
    let result = failed_start(go.unwrap(), &root.join(&label), host, port, root, &label);
    let mut observation =
        capture_http_aux::startup_failure(case, port, result.status.code(), &result.stderr);
    observation["stdout_bytes"] = result.stdout.len().into();
    observation["token_bytes"] = fs::read(root.join(&label).join("data/mcp_token"))
        .unwrap()
        .len()
        .into();
    measured.push(observation);
    (result.status.code(), result.stderr)
}

#[test]
fn native_bind_failures_match_checked_out_go() {
    const TEST: &str = "native_bind_failures_match_checked_out_go";
    let root = tempfile::tempdir().unwrap();
    let frozen = frozen_http_aux::observation(THIS_FILE, TEST);
    if let Some(record) = &frozen {
        assert_eq!(record["cases"].as_array().unwrap().len(), 3);
    }
    let recorded = |index: usize| frozen.as_ref().map(|record| &record["cases"][index]);
    let go = frozen.is_none().then(|| oracle(root.path()));
    let mut measured = Vec::new();
    let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
    // IPv6 is a required native control, never an optional zero-case skip.
    for (index, (case, host)) in [("ipv4", "127.0.0.1"), ("ipv6", "::1")]
        .into_iter()
        .enumerate()
    {
        let listener = TcpListener::bind((host, 0)).expect("native loopback listener");
        let port = listener.local_addr().unwrap().port();
        let (go_code, go_stderr) = go_failed_start(
            recorded(index),
            go.as_deref(),
            &mut measured,
            root.path(),
            host,
            port,
            case,
        );
        let rust_result = failed_start(
            rust,
            &root.path().join(format!("rust-{case}")),
            host,
            port,
            root.path(),
            &format!("rust-{case}"),
        );
        assert_eq!(rust_result.status.code(), go_code);
        assert_eq!(
            rust_result.stderr, go_stderr,
            "actual {case} occupied-bind diagnostic"
        );
        let diagnostic = String::from_utf8(go_stderr).unwrap();
        assert!(
            diagnostic.starts_with("listen tcp ")
                && diagnostic.contains(&format!(":{port}: bind: "))
        );
        drop(listener);
    }
    let host = "192.0.2.1";
    let probe = TcpListener::bind((host, 0)).unwrap_err();
    assert_eq!(probe.kind(), std::io::ErrorKind::AddrNotAvailable);
    let port = mcp_http_port::free_port();
    let (go_code, go_stderr) = go_failed_start(
        recorded(2),
        go.as_deref(),
        &mut measured,
        root.path(),
        host,
        port,
        "unavailable",
    );
    let rust_result = failed_start(
        rust,
        &root.path().join("rust-unavailable"),
        host,
        port,
        root.path(),
        "rust-unavailable",
    );
    assert_eq!(rust_result.status.code(), go_code);
    assert_eq!(
        rust_result.stderr, go_stderr,
        "actual unavailable-address diagnostic"
    );
    assert!(
        String::from_utf8(go_stderr)
            .unwrap()
            .contains(&format!("listen tcp {host}:{port}: bind: "))
    );
    if let Some(go) = &go {
        capture_http_aux::record(go, THIS_FILE, TEST, serde_json::json!({"cases": measured}));
    }
    eprintln!("native bind differential executed IPv4, IPv6 and TEST-NET controls");
}

#[cfg(windows)]
mod windows_acl {
    use super::*;
    use base64::Engine;
    use mcp_http_port::{accepts_token, spawn_with_handoff};
    use serde_json::Value;

    fn acl(path: &Path, mode: &str, logs: &Path, label: &str) -> Value {
        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut command = Command::new(powershell);
        private(&mut command, logs);
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(repo().join("crates/symeraseme-cli/tests/support/mcp_token_acl.ps1"))
            .arg("-Path")
            .arg(path)
            .arg("-Mode")
            .arg(mode);
        let result = capture(command, logs, label, Duration::from_secs(30));
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&result.stdout).unwrap()
    }

    fn start(binary: &Path, root: &Path) -> (StartedChild, u16, String) {
        let mut port = mcp_http_port::free_port();
        let child = spawn_with_handoff(
            &mut port,
            &root.join("data/mcp_token"),
            Duration::from_secs(10),
            |port, stderr| {
                let mut command = Command::new(binary);
                private(&mut command, root);
                command
                    .args(["mcp", "--host", "127.0.0.1", "--port", &port.to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(stderr)
                    .spawn()
            },
        );
        let token = fs::read_to_string(root.join("data/mcp_token")).unwrap();
        assert_eq!(token.len(), 43);
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(&token)
                .unwrap()
                .len(),
            32
        );
        (child, port, token)
    }

    #[test]
    fn native_windows_token_dacl_and_rotation_match_go() {
        let root = tempfile::tempdir().unwrap();
        let go = oracle(root.path());
        let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
        for (case, protected) in [("inherited", false), ("owner-only-parent", true)] {
            let parent = root.path().join(case);
            fs::create_dir(&parent).unwrap();
            if protected {
                assert_eq!(
                    acl(&parent, "protect", root.path(), case)["protected"],
                    true
                );
            }
            let go_root = parent.join("go");
            let rust_root = parent.join("rust");
            for directory in [&go_root, &rust_root] {
                fs::create_dir(directory).unwrap();
                fs::write(directory.join("sentinel"), "unrelated private sentinel").unwrap();
            }
            let mut observations = Vec::new();
            for (name, binary, directory) in
                [("go", go.as_path(), &go_root), ("rust", rust, &rust_root)]
            {
                let (mut child, _, old) = start(binary, directory);
                child.kill().unwrap();
                child.wait().unwrap();
                drop(child);
                let token_file = directory.join("data/mcp_token");
                let before = acl(
                    &token_file,
                    "read",
                    root.path(),
                    &format!("{case}-{name}-before"),
                );
                let (mut restarted, port, new) = start(binary, directory);
                assert!(old != new, "{name} token did not rotate");
                assert!(!accepts_token(
                    port,
                    &old,
                    Instant::now() + Duration::from_secs(2)
                ));
                assert!(accepts_token(
                    port,
                    &new,
                    Instant::now() + Duration::from_secs(2)
                ));
                restarted.kill().unwrap();
                restarted.wait().unwrap();
                drop(restarted);
                let after = acl(
                    &token_file,
                    "read",
                    root.path(),
                    &format!("{case}-{name}-after"),
                );
                assert_eq!(before, after, "{name} rotation changed the file ACL");
                assert_eq!(after["readonly"], false);
                assert_eq!(
                    fs::read(directory.join("sentinel")).unwrap(),
                    b"unrelated private sentinel"
                );
                let directory_acl = acl(
                    &directory.join("data"),
                    "read",
                    root.path(),
                    &format!("{case}-{name}-dir"),
                );
                // A read-only existing file rejects replacement, retaining
                // both the old token bytes and its complete security descriptor.
                let original_permissions = fs::metadata(&token_file).unwrap().permissions();
                let mut permissions = original_permissions.clone();
                permissions.set_readonly(true);
                fs::set_permissions(&token_file, permissions).unwrap();
                let readonly_acl = acl(
                    &token_file,
                    "read",
                    root.path(),
                    &format!("{case}-{name}-readonly"),
                );
                let failed = failed_start(
                    binary,
                    directory,
                    "127.0.0.1",
                    mcp_http_port::free_port(),
                    root.path(),
                    &format!("{case}-{name}-failed"),
                );
                assert!(
                    String::from_utf8(failed.stderr)
                        .unwrap()
                        .starts_with("write MCP auth token: ")
                );
                assert_eq!(fs::read_to_string(&token_file).unwrap(), new);
                assert_eq!(
                    acl(
                        &token_file,
                        "read",
                        root.path(),
                        &format!("{case}-{name}-retained")
                    ),
                    readonly_acl
                );
                observations.push((before, directory_acl, readonly_acl));
                fs::set_permissions(&token_file, original_permissions).unwrap();
            }
            assert_eq!(
                observations[0], observations[1],
                "{case}: actual Go/Rust owner/group/DACL/protection/readonly parity"
            );
        }
        eprintln!(
            "Windows token ACL differential executed inherited and protected parents, rotation and read-only retention"
        );
    }
}
