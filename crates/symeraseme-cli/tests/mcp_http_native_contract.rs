//! Actual Go/Rust bind failures and Windows token DACLs in disposable roots.
use std::fs;
use std::net::TcpListener;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[path = "support/mcp_http_port.rs"]
#[allow(dead_code)] // Windows-only controls use additional shared process helpers.
mod mcp_http_port;
use mcp_http_port::StartedChild;

#[path = "support/frozen_http_aux.rs"]
#[allow(dead_code)]
mod frozen_http_aux;

#[path = "support/frozen_token_acl.rs"]
#[allow(dead_code)] // Windows-only replay consumes the rest; other hosts run the reader unit tests.
mod frozen_token_acl;

const THIS_FILE: &str = "crates/symeraseme-cli/tests/mcp_http_native_contract.rs";

#[cfg(windows)]
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

#[cfg(windows)]
mod windows_acl {
    use super::*;
    use base64::Engine;
    use frozen_token_acl::{Acl, Observation};
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
            "native token ACL observation failed"
        );
        assert!(result.stderr == format!("MCP token ACL script started: {mode}\r\nMCP token ACL script completed: {mode}\r\n").as_bytes());
        serde_json::from_slice(&result.stdout).unwrap()
    }

    fn observed(value: &Value) -> Acl {
        Acl::from_json(value).unwrap()
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

    /// Fresh same-run identity: the bounded pwsh 7 Verify run re-observes this process token
    /// (inherited by the child) against the baseline. Only then is G:<PRIMARY_GROUP> expanded.
    fn verified_primary_group(root: &Path, target: &str) -> String {
        let baseline = std::env::var_os("SYMERASEME_WINDOWS_IDENTITY_BASELINE")
            .map(PathBuf::from)
            .expect(
                "SYMERASEME_WINDOWS_IDENTITY_BASELINE must name the same-run identity baseline",
            );
        assert!(
            baseline.is_absolute(),
            "identity baseline path must be absolute"
        );
        let arch = match target {
            "windows/amd64" => "X64",
            "windows/arm64" => "ARM64",
            _ => panic!("no Windows identity architecture for this target"),
        };
        assert!(
            fs::metadata(&baseline).unwrap().len() <= 16 * 1024,
            "identity baseline is too large"
        );
        let raw = fs::read(&baseline).expect("identity baseline is readable");
        assert!(raw.len() <= 16 * 1024, "identity baseline is too large");
        let value: Value = serde_json::from_slice(&raw).expect("identity baseline is JSON");
        let programs =
            PathBuf::from(std::env::var_os("ProgramFiles").expect("native ProgramFiles"));
        let pwsh = programs.join("PowerShell/7/pwsh.exe");
        let git = programs.join("Git/cmd/git.exe");
        assert!(
            pwsh.is_file() && git.is_file(),
            "native identity executables unavailable"
        );
        let mut revision_command = Command::new(git);
        private(&mut revision_command, root);
        revision_command
            .arg("-C")
            .arg(repo())
            .args(["rev-parse", "HEAD"]);
        let revision_output = capture(
            revision_command,
            root,
            "identity-source",
            Duration::from_secs(30),
        );
        assert!(revision_output.status.success());
        let revision = std::str::from_utf8(&revision_output.stdout).unwrap().trim();
        let script = repo().join("scripts/capture-windows-identity.ps1");
        let script_bytes = fs::read(&script).unwrap();
        let mut command = Command::new(pwsh);
        private(&mut command, root);
        // PowerShell needs PATHEXT to resolve the verifier's bare `git` command.
        command.env("PATHEXT", ".EXE");
        for key in ["PATH", "PSModulePath", "ProgramFiles"] {
            if let Some(path) = std::env::var_os(key) {
                command.env(key, path);
            }
        }
        command
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
            .arg(&script)
            .args([
                "-Mode",
                "Verify",
                "-ExpectedArch",
                arch,
                "-ExpectedRevision",
                revision,
                "-ArtifactPath",
            ])
            .arg(&baseline);
        let result = capture(command, root, "identity-verify", Duration::from_secs(120));
        assert!(
            result.status.success(),
            "fresh identity verification failed (exit {:?})",
            result.status.code()
        );
        assert!(
            fs::read(&baseline).unwrap() == raw,
            "identity baseline changed during verification"
        );
        assert!(
            fs::read(&script).unwrap() == script_bytes,
            "identity verifier changed"
        );
        let script_hash = frozen_token_acl::digest(&script_bytes);
        frozen_token_acl::check_identity_baseline(&value, arch, revision, &script_hash)
            .expect("independently verified current identity relationships")
    }

    /// One actual Rust run of an inherited or owner-only parent; returns every compared field.
    fn rust_case(rust: &Path, root: &Path, case: &str, protected: bool) -> Observation {
        let parent = root.join(case);
        fs::create_dir(&parent).unwrap();
        if protected {
            assert_eq!(acl(&parent, "protect", root, case)["protected"], true);
        }
        let directory = parent.join("rust");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("sentinel"), frozen_token_acl::SENTINEL).unwrap();

        let (mut child, _, old) = start(rust, &directory);
        child.kill().unwrap();
        child.wait().unwrap();
        drop(child);
        let token_file = directory.join("data/mcp_token");
        let before = acl(&token_file, "read", root, &format!("{case}-before"));
        let (mut restarted, port, new) = start(rust, &directory);
        let rotated = old != new;
        let old_accepted = accepts_token(port, &old, Instant::now() + Duration::from_secs(2));
        let new_accepted = accepts_token(port, &new, Instant::now() + Duration::from_secs(2));
        restarted.kill().unwrap();
        restarted.wait().unwrap();
        drop(restarted);
        let after = acl(&token_file, "read", root, &format!("{case}-after"));
        let directory_acl = acl(
            &directory.join("data"),
            "read",
            root,
            &format!("{case}-dir"),
        );

        // A read-only existing file rejects replacement, retaining both the
        // old token bytes and its complete security descriptor.
        let token_permissions = fs::metadata(&token_file).unwrap().permissions();
        let mut readonly = token_permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&token_file, readonly).unwrap();
        let readonly_acl = acl(&token_file, "read", root, &format!("{case}-readonly"));
        let failed = failed_start(
            rust,
            &directory,
            "127.0.0.1",
            mcp_http_port::free_port(),
            root,
            &format!("{case}-failed"),
        );
        let retained_acl = acl(&token_file, "read", root, &format!("{case}-retained"));
        let retained_token = fs::read_to_string(&token_file).unwrap() == new;
        let observation = Observation {
            name: case.to_owned(),
            parent_protected: protected,
            token_before_bytes: old.len(),
            token_after_bytes: new.len(),
            rotated,
            old_accepted,
            new_accepted,
            before: observed(&before),
            after: observed(&after),
            directory: observed(&directory_acl),
            readonly: observed(&readonly_acl),
            retained: observed(&retained_acl),
            failed_exit_code: failed.status.code(),
            failed_stdout: failed.stdout.clone(),
            failed_stderr: frozen_token_acl::normalize_runtime_root(&failed.stderr, &directory),
            retained_token,
            sentinel: fs::read(directory.join("sentinel")).unwrap(),
        };
        fs::set_permissions(&token_file, token_permissions).unwrap();
        observation
    }

    #[test]
    fn native_windows_token_dacl_and_rotation_match_go() {
        frozen_token_acl::reject_retired_env().unwrap();
        let root = tempfile::tempdir().unwrap();
        let target = frozen_http_aux::native_target().expect("native Windows target");
        let retained = frozen_token_acl::load(target)
            .expect("authenticated token ACL expectations for this target");
        assert_eq!(retained.len(), 2);
        let group = verified_primary_group(root.path(), target);
        let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
        for (expected, protected) in retained.iter().zip([false, true]) {
            assert_eq!(expected.parent_protected, protected);
            let expected = expected
                .expand_group(&group)
                .expect("identity-bound ACL expansion");
            let actual = rust_case(rust, root.path(), &expected.name, protected);
            if let Err(reason) = frozen_token_acl::compare(&expected, &actual) {
                panic!("{reason}");
            }
        }
        eprintln!(
            "Windows token ACL replay executed inherited and owner-only-parent cases against actual Rust"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn windows_token_capture_rejects_non_windows() {
    frozen_token_acl::reject_retired_env().unwrap();
}
