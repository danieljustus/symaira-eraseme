//! Actual Go/Rust bind failures in disposable roots, plus the frozen Windows token reader controls.
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[path = "support/mcp_http_port.rs"]
#[allow(dead_code)] // The frozen readers use only part of the shared process helpers.
mod mcp_http_port;
use mcp_http_port::StartedChild;

#[path = "support/frozen_http_aux.rs"]
#[allow(dead_code)]
mod frozen_http_aux;

#[path = "support/frozen_token_acl.rs"]
#[allow(dead_code)] // Hosts run the frozen reader unit tests; the Windows replay was removed.
mod frozen_token_acl;

const THIS_FILE: &str = "crates/symeraseme-cli/tests/mcp_http_native_contract.rs";

fn private(command: &mut Command, root: &Path) {
    fs::create_dir_all(root).unwrap();
    command
        .env_clear()
        .current_dir(root)
        .env("HOME", root)
        .env("SYMERASEME_DATA_DIR", root.join("data"))
        .env("TMPDIR", root)
        .env("TMP", root)
        .env("TEMP", root);
    for key in ["CONFIG", "DATA", "STATE", "CACHE"] {
        command.env(format!("XDG_{key}_HOME"), root.join(key.to_lowercase()));
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
            panic!("native helper deadline; private diagnostics retained");
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

/// Recorded native Go startup failure with this run's port.
fn go_failed_start(recorded: &serde_json::Value, port: u16, case: &str) -> (Option<i32>, Vec<u8>) {
    let label = format!("go-{case}");
    assert_eq!(recorded["stdout_bytes"], 0);
    assert_eq!(recorded["token_bytes"], 43);
    let (code, stderr) = frozen_http_aux::startup_failure(recorded, case, port);
    assert_ne!(code, Some(0), "{label} unexpectedly listened");
    (code, stderr)
}

#[test]
fn native_bind_failures_match_checked_out_go() {
    const TEST: &str = "native_bind_failures_match_checked_out_go";
    let root = tempfile::tempdir().unwrap();
    let frozen = frozen_http_aux::observation(THIS_FILE, TEST)
        .expect("live Go is retired; recorded native bind failures are required");
    assert_eq!(frozen["cases"].as_array().unwrap().len(), 3);
    let recorded = |index: usize| &frozen["cases"][index];
    let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
    // IPv6 is a required native control, never an optional zero-case skip.
    for (index, (case, host)) in [("ipv4", "127.0.0.1"), ("ipv6", "::1")]
        .into_iter()
        .enumerate()
    {
        let listener = TcpListener::bind((host, 0)).expect("native loopback listener");
        let port = listener.local_addr().unwrap().port();
        let (go_code, go_stderr) = go_failed_start(recorded(index), port, case);
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
    let (go_code, go_stderr) = go_failed_start(recorded(2), port, "unavailable");
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
    eprintln!("native bind differential executed IPv4, IPv6 and TEST-NET controls");
}

#[test]
fn windows_token_capture_rejects_non_windows() {
    frozen_token_acl::reject_retired_env().unwrap();
}
