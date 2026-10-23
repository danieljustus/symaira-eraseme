//! Native executable parity for malformed host-agent stderr. The portable
//! portion checks the helper and Go observation on other hosts.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Deserialize;

#[path = "support/capture_agent_error.rs"]
mod capture_agent_error;
#[path = "support/frozen_agent_error.rs"]
mod frozen_agent_error;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;

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

fn measured(corpus: &frozen_agent_error::Corpus, name: &str) -> Captured {
    let (code, stdout, stderr) = corpus.process(name);
    #[cfg(unix)]
    let status = ExitStatus::from_raw(code << 8);
    #[cfg(windows)]
    let status = ExitStatus::from_raw(u32::try_from(code).unwrap());
    Captured {
        status,
        stdout,
        stderr,
    }
}

fn capture(mut command: Command, root: &Path, name: &str, timeout: Duration) -> Captured {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn child");
    let (tx, rx) = mpsc::channel();
    let readers: Vec<Box<dyn Read + Send>> = vec![
        Box::new(child.stdout.take().unwrap()),
        Box::new(child.stderr.take().unwrap()),
    ];
    for (index, reader) in readers.into_iter().enumerate() {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = reader.take(MAX_CAPTURE + 1).read_to_end(&mut bytes);
            let _ = tx.send((index, result.map(|_| bytes)));
        });
    }
    drop(tx);
    let deadline = Instant::now() + timeout;
    let mut output = [None, None];
    let mut status = None;
    loop {
        while let Ok((index, result)) = rx.try_recv() {
            match result {
                Ok(bytes) if bytes.len() as u64 <= MAX_CAPTURE => output[index] = Some(bytes),
                _ => {
                    let _ = child.kill();
                    child.wait().expect("reap output-rejected child");
                    panic!("{name}: capture limit or read failure");
                }
            }
        }
        if status.is_none() {
            status = child.try_wait().expect("poll child");
        }
        if status.is_some() && output.iter().all(Option::is_some) {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait().expect("reap timed-out child");
            panic!("{name} exceeded {timeout:?}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let [stdout, stderr] = output;
    let status = child.wait().expect("reap completed child");
    let stdout = stdout.unwrap();
    let stderr = stderr.unwrap();
    fs::write(root.join(format!("{name}.stdout")), &stdout).expect("bounded stdout log");
    fs::write(root.join(format!("{name}.stderr")), &stderr).expect("bounded stderr log");
    Captured {
        status,
        stdout,
        stderr,
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

fn rust_fixture(root: &Path) -> PathBuf {
    let executable = root.join(if cfg!(windows) {
        "synthetic-agent-error.exe"
    } else {
        "synthetic-agent-error"
    });
    let mut build = Command::new("rustc");
    build
        .args(["+1.98.0", "--edition=2024", "--crate-type=bin", "-o"])
        .arg(&executable)
        .arg(Path::new(ROOT).join("crates/symeraseme-cli/tests/fixtures/native_agent_error.rs"));
    let output = capture(build, root, "rust-fixture-build", Duration::from_secs(60));
    assert!(
        output.status.success(),
        "native synthetic Rust fixture build: {}",
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
    let corpus = frozen_agent_error::observations();
    let executable = corpus.is_none().then(|| helper(root.path()));
    let agent = root.path().join(if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    });
    if let Some(executable) = &executable {
        fs::copy(executable, &agent).expect("native Go fake agent copy");
    }

    let sanity = if let Some(corpus) = &corpus {
        measured(corpus, "helper_sanity")
    } else {
        let mut sanity_command = Command::new(&agent);
        isolated(
            &mut sanity_command,
            root.path(),
            root.path(),
            &root.path().join("sanity-data"),
        );
        capture(
            sanity_command,
            root.path(),
            "fake-agent",
            Duration::from_secs(5),
        )
    };
    assert_eq!(sanity.status.code(), Some(23));
    assert!(sanity.stdout.is_empty());
    assert_eq!(sanity.stderr, BAD_STDERR, "fake agent emitted raw bytes");

    let rust_fixture = rust_fixture(root.path());
    let mut fixture_command = Command::new(&rust_fixture);
    isolated(
        &mut fixture_command,
        root.path(),
        root.path(),
        &root.path().join("rust-fixture-data"),
    );
    let fixture_sanity = capture(
        fixture_command,
        root.path(),
        "rust-fixture",
        Duration::from_secs(5),
    );
    assert_eq!(fixture_sanity.status, sanity.status);
    assert_eq!(fixture_sanity.stdout, sanity.stdout);
    assert_eq!(fixture_sanity.stderr, sanity.stderr);

    let mut go_limit_controls = Vec::new();
    let mut fixtures = vec![("rust", &rust_fixture)];
    if let Some(executable) = &executable {
        fixtures.insert(0, ("go", executable));
    }
    for (fixture_name, fixture) in fixtures {
        for stream in ["stdout", "stderr"] {
            let started = Instant::now();
            let error = std::panic::catch_unwind(|| {
                let mut flood = Command::new(fixture);
                flood.args(["--flood", stream]);
                isolated(
                    &mut flood,
                    root.path(),
                    root.path(),
                    &root.path().join("sanity-data"),
                );
                capture(
                    flood,
                    root.path(),
                    &format!("{fixture_name}-flood-{stream}"),
                    Duration::from_secs(10),
                );
            })
            .expect_err("live capture rejects oversized output");
            let text = error.downcast_ref::<String>().expect("capture error text");
            assert!(
                text.contains("capture limit"),
                "must fail on size, not timeout"
            );
            assert!(started.elapsed() < Duration::from_secs(10));
            if fixture_name == "go" {
                go_limit_controls.push(serde_json::json!({
                    "stream": stream, "rejected": true,
                    "actual_reason": text, "elapsed_ms": started.elapsed().as_millis(),
                    "capture_limit_bytes": MAX_CAPTURE,
                }));
            }
        }
    }

    let go = if let Some(corpus) = &corpus {
        measured(corpus, "oracle")
    } else {
        let go_root = root.path().join("go");
        let go_bin = go_root.join("bin");
        let go_data = go_root.join("data");
        fs::create_dir_all(&go_bin).expect("Go bin");
        fs::create_dir_all(&go_data).expect("Go data");
        fs::copy(&agent, go_bin.join(agent.file_name().unwrap())).expect("Go fake agent");
        let mut command = Command::new(executable.as_ref().unwrap());
        command.arg("--oracle");
        isolated(&mut command, &go_root, &go_bin, &go_data);
        capture(command, root.path(), "go-oracle", Duration::from_secs(30))
    };
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

    let stream = |raw: &[u8]| {
        serde_json::json!({
            "bytes": raw.len(),
            "base64": base64::engine::general_purpose::STANDARD.encode(raw),
        })
    };
    if let Some(executable) = &executable {
        capture_agent_error::record(
            executable,
            serde_json::json!({
                "oracle": {"argv": ["--oracle"], "exit_status": go.status.code().unwrap(),
                           "stdout": stream(&go.stdout), "stderr": stream(&go.stderr)},
                "helper_sanity": {"argv": [], "exit_status": sanity.status.code().unwrap(),
                                  "stdout": stream(&sanity.stdout), "stderr": stream(&sanity.stderr)},
                "helper_limit_controls": go_limit_controls,
                "Rust_fixture_matches_actual_Go_sanity": fixture_sanity.status == sanity.status
                    && fixture_sanity.stdout == sanity.stdout && fixture_sanity.stderr == sanity.stderr,
            }),
        );
    }

    #[cfg(windows)]
    {
        let rust_root = root.path().join("rust");
        let rust_bin = rust_root.join("bin");
        let rust_data = rust_root.join("data");
        fs::create_dir_all(&rust_bin).expect("Rust bin");
        fs::create_dir_all(&rust_data).expect("Rust data");
        fs::copy(&rust_fixture, rust_bin.join("claude.exe")).expect("Rust native fake agent");
        seed(&rust_data);
        let mut command = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
        command.args(["mcp", "--stdio"]);
        isolated(&mut command, &rust_root, &rust_bin, &rust_data);
        let input_path = rust_root.join("request.json");
        fs::write(&input_path, &request).expect("bounded request fixture");
        command.stdin(std::fs::File::open(input_path).expect("request stdin"));
        let rust = capture(command, root.path(), "rust", Duration::from_secs(30));
        assert!(
            rust.status.success(),
            "Rust MCP status {}: {}",
            rust.status,
            String::from_utf8_lossy(&rust.stderr)
        );
        assert!(rust.stderr.is_empty(), "MCP stdio stderr stays clean");
        compare_bytes(&rust.stdout, &expected).expect("native Go/Rust MCP byte parity");
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
