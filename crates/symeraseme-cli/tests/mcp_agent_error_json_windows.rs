//! Native executable parity for malformed host-agent stderr. The portable
//! portion checks the helper and Go observation on other hosts.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Deserialize;
#[cfg(windows)]
use std::io::Write;
#[cfg(windows)]
use std::process::Stdio;
#[cfg(windows)]
use symeraseme_core::storage::Store;
#[cfg(windows)]
use symeraseme_core::storage::repository::Repository;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const BAD_STDERR: &[u8] = b"before\xf0\x80\x80after\xffend";
const MAX_CAPTURE: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct Observation {
    schema: String,
    platform: String,
    agent_stderr_bytes_base64: String,
    request_base64: String,
    response_base64: String,
}

struct Captured {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn capture(mut command: Command, root: &Path, name: &str, timeout: Duration) -> Captured {
    let stdout_path = root.join(format!("{name}.stdout"));
    let stderr_path = root.join(format!("{name}.stderr"));
    command
        .stdout(File::create(&stdout_path).expect("stdout file"))
        .stderr(File::create(&stderr_path).expect("stderr file"));
    let mut child = command.spawn().expect("spawn child");
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name} exceeded {timeout:?}");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let read = |path: &Path| {
        assert!(fs::metadata(path).expect("capture metadata").len() <= MAX_CAPTURE);
        fs::read(path).expect("capture bytes")
    };
    Captured {
        status,
        stdout: read(&stdout_path),
        stderr: read(&stderr_path),
    }
}

fn helper(root: &Path) -> PathBuf {
    let executable = root.join(if cfg!(windows) {
        "mcp-agent-error-json-windows.exe"
    } else {
        "mcp-agent-error-json-windows"
    });
    let mut command = Command::new("go");
    command
        .args(["build", "-o"])
        .arg(&executable)
        .arg("./rust-tests/parity/oracle/mcp-agent-error-json-windows")
        .current_dir(ROOT)
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .env("GOCACHE", root.join("gocache"));
    let output = capture(command, root, "go-build", Duration::from_secs(180));
    assert!(
        output.status.success(),
        "Go helper build: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

fn isolated(command: &mut Command, root: &Path, bin: &Path, data: &Path) {
    let home = root.join("home");
    let temp = root.join("tmp");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&temp).expect("temp");
    command
        .env_clear()
        .current_dir(root)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("TMP", &temp)
        .env("TEMP", &temp)
        .env("TMPDIR", &temp)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("PATH", bin)
        .env("SYMERASEME_DATA_DIR", data)
        .env("SYMERASEME_LLM_PROVIDER", "agent")
        .env("SYMERASEME_AGENT_BACKEND", "claude")
        .env("TERM", "dumb");
    #[cfg(windows)]
    {
        // Windows process loading and Go's LookPath need only these OS keys.
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command.env("PATHEXT", ".EXE");
    }
}

fn decode(value: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .expect("base64 observation")
}

fn compare_bytes(actual: &[u8], expected: &[u8]) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "MCP response bytes differ: actual {} bytes, expected {} bytes",
            actual.len(),
            expected.len()
        ))
    }
}

#[test]
fn native_helper_and_go_contract_observation() {
    let root = tempfile::tempdir().expect("test root");
    let executable = helper(root.path());
    let agent = root.path().join(if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    });
    fs::copy(&executable, &agent).expect("native fake agent copy");

    let mut sanity_command = Command::new(&agent);
    isolated(
        &mut sanity_command,
        root.path(),
        root.path(),
        &root.path().join("sanity-data"),
    );
    let sanity = capture(
        sanity_command,
        root.path(),
        "fake-agent",
        Duration::from_secs(5),
    );
    assert_eq!(sanity.status.code(), Some(23));
    assert!(sanity.stdout.is_empty());
    assert_eq!(sanity.stderr, BAD_STDERR, "fake agent emitted raw bytes");

    let go_root = root.path().join("go");
    let go_bin = go_root.join("bin");
    let go_data = go_root.join("data");
    fs::create_dir_all(&go_bin).expect("Go bin");
    fs::create_dir_all(&go_data).expect("Go data");
    fs::copy(&agent, go_bin.join(agent.file_name().unwrap())).expect("Go fake agent");
    let mut command = Command::new(&executable);
    command.arg("--oracle");
    isolated(&mut command, &go_root, &go_bin, &go_data);
    let go = capture(command, root.path(), "go-oracle", Duration::from_secs(30));
    assert!(
        go.status.success(),
        "Go ContractHandler oracle: {}",
        String::from_utf8_lossy(&go.stderr)
    );
    assert!(go.stderr.is_empty());
    let observed: Observation = serde_json::from_slice(&go.stdout).expect("Go observation");
    assert_eq!(
        observed.schema,
        "symeraseme.go-oracle.mcp-agent-error-json-windows.v1"
    );
    let go_os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    assert!(observed.platform.starts_with(&format!("{go_os}/")));
    assert_eq!(decode(&observed.agent_stderr_bytes_base64), BAD_STDERR);
    let expected = decode(&observed.response_base64);
    let request = decode(&observed.request_base64);
    assert!(!expected.is_empty(), "Go emitted an MCP response");
    assert!(request.ends_with(b"\n"), "complete MCP request frame");
    let response: serde_json::Value =
        serde_json::from_slice(&expected).expect("Go MCP response JSON");
    assert_eq!(response["error"]["code"], -32603);
    let content = response["error"]["message"]
        .as_str()
        .expect("Go MCP error text");
    for piece in ["before", "after", "end", "\u{fffd}"] {
        assert!(content.contains(piece), "Go response lost stderr evidence");
    }

    // Negative control: the byte comparator must reject a one-byte mutation.
    let mut tampered = expected.clone();
    tampered[0] ^= 1;
    assert!(compare_bytes(&tampered, &expected).is_err());
    assert!(compare_bytes(&expected, &expected).is_ok());

    #[cfg(windows)]
    {
        let rust_root = root.path().join("rust");
        let rust_bin = rust_root.join("bin");
        let rust_data = rust_root.join("data");
        fs::create_dir_all(&rust_bin).expect("Rust bin");
        fs::create_dir_all(&rust_data).expect("Rust data");
        fs::copy(&agent, rust_bin.join("claude.exe")).expect("Rust fake agent");
        seed(&rust_data);
        let mut command = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
        command.args(["mcp", "--stdio"]);
        isolated(&mut command, &rust_root, &rust_bin, &rust_data);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(File::create(root.path().join("rust.stdout")).unwrap())
            .stderr(File::create(root.path().join("rust.stderr")).unwrap())
            .spawn()
            .expect("spawn Rust MCP process");
        child
            .stdin
            .take()
            .expect("MCP stdin")
            .write_all(&request)
            .expect("write MCP request");
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll Rust MCP") {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Rust MCP exceeded 30 seconds");
            }
            thread::sleep(Duration::from_millis(20));
        };
        let read = |name: &str| {
            let path = root.path().join(name);
            assert!(fs::metadata(&path).unwrap().len() <= MAX_CAPTURE);
            fs::read(path).unwrap()
        };
        let rust_stderr = read("rust.stderr");
        assert!(
            status.success(),
            "Rust MCP status {status}: {}",
            String::from_utf8_lossy(&rust_stderr)
        );
        assert!(rust_stderr.is_empty(), "MCP stdio stderr stays clean");
        compare_bytes(&read("rust.stdout"), &expected).expect("native Go/Rust MCP byte parity");
    }
}

#[cfg(windows)]
fn seed(data: &Path) {
    let store = Store::open(data.join("symeraseme.db")).expect("seed store open");
    let request_id = Repository::new(&store)
        .create_removal_request(
            "oracle-broker",
            "email",
            "oracle-campaign",
            "DE",
            "gdpr-art17.de.md.j2",
            "",
        )
        .expect("seed removal request");
    store
        .connection()
        .execute(
            "INSERT INTO inbox_replies (request_id, message_id, thread_id, from_addr, subject, snippet) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (request_id, "oracle-message", "oracle-thread", "privacy@example.invalid", "We need your current address", "Your current address does not match our records."),
        )
        .expect("seed inbox reply");
}
