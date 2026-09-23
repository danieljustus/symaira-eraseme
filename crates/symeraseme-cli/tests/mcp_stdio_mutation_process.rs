//! Byte-exact Go process oracle for malformed and boundary MCP stdio inputs.

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FIXTURE: &str =
    include_str!("../../../rust-tests/parity/oracle/mcp-stdio-mutations/cases.json");
const INITIALIZE: &str = include_str!("../../../tests/fixtures/mcp-contract/initialize_cases.json");
const GO_MAIN: &[u8] = include_bytes!("../../../cmd/symeraseme/main.go");
const GO_SERVER: &[u8] = include_bytes!("../../../internal/mcp/server.go");
const GENERATOR: &[u8] =
    include_bytes!("../../../rust-tests/parity/oracle/mcp-stdio-mutations/generate.py");
const INITIALIZE_BYTES: &[u8] =
    include_bytes!("../../../tests/fixtures/mcp-contract/initialize_cases.json");
const MAX_CAPTURE_BYTES: u64 = 2 * 1024 * 1024;

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn input_from_spec(spec: &Value) -> Vec<u8> {
    if let Some(encoded) = spec["base64"].as_str() {
        return STANDARD.decode(encoded).expect("base64 input spec");
    }
    match spec["kind"].as_str().expect("input kind") {
        "padding_request" => {
            let size = spec["size"].as_u64().expect("padding request size") as usize;
            let prefix = b"{\"padding\":\"";
            let suffix = b"\"}";
            assert!(size >= prefix.len() + suffix.len());
            let mut input = Vec::with_capacity(size);
            input.extend_from_slice(prefix);
            input.resize(size - suffix.len(), b'x');
            input.extend_from_slice(suffix);
            input
        }
        "nested_request" => {
            let depth = spec["array_depth"].as_u64().expect("array depth") as usize;
            let mut input =
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"ignored\":".to_vec();
            input.extend(std::iter::repeat_n(b'[', depth));
            input.push(b'0');
            input.extend(std::iter::repeat_n(b']', depth));
            input.push(b'}');
            input
        }
        other => panic!("unknown input spec kind {other}"),
    }
}

fn isolated_child() -> (Child, std::path::PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mcp008-{}-{nonce}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(["mcp", "--stdio"])
        .current_dir(&root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    (child, root)
}

fn run_bounded(child: &mut Child, input: Vec<u8>) -> (std::process::ExitStatus, Vec<u8>, Vec<u8>) {
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (write_tx, write_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stdin = stdin;
        let result = stdin.write_all(&input);
        drop(stdin);
        let _ = write_tx.send(result);
    });
    let (stdout_tx, stdout_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(MAX_CAPTURE_BYTES + 1).read_to_end(&mut bytes);
        let _ = stdout_tx.send(result.map(|_| bytes));
    });
    let (stderr_tx, stderr_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stderr.take(MAX_CAPTURE_BYTES + 1).read_to_end(&mut bytes);
        let _ = stderr_tx.send(result.map(|_| bytes));
    });

    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("stdio process exceeded its 30 second bound");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let remaining = || deadline.saturating_duration_since(Instant::now());
    write_rx
        .recv_timeout(remaining())
        .expect("bounded stdin writer did not finish")
        .unwrap();
    let stdout = stdout_rx
        .recv_timeout(remaining())
        .expect("bounded stdout reader did not finish")
        .unwrap();
    let stderr = stderr_rx
        .recv_timeout(remaining())
        .expect("bounded stderr reader did not finish")
        .unwrap();
    assert!(
        stdout.len() as u64 <= MAX_CAPTURE_BYTES,
        "stdout exceeded the capture bound"
    );
    assert!(
        stderr.len() as u64 <= MAX_CAPTURE_BYTES,
        "stderr exceeded the capture bound"
    );
    (status, stdout, stderr)
}

#[test]
fn malformed_and_boundary_stdio_matches_source_bound_go_process() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("Go mutation oracle fixture");
    assert!(fixture["go_version"].as_str().unwrap().starts_with("go"));
    assert_eq!(fixture["source_files"][0]["path"], "cmd/symeraseme/main.go");
    assert_eq!(fixture["source_files"][1]["path"], "internal/mcp/server.go");
    assert_eq!(fixture["source_files"][0]["sha256"], sha256(GO_MAIN));
    assert_eq!(fixture["source_files"][1]["sha256"], sha256(GO_SERVER));
    assert_eq!(
        fixture["initialize_fixture_sha256"],
        sha256(INITIALIZE_BYTES)
    );
    assert_eq!(fixture["generator_sha256"], sha256(GENERATOR));
    assert!(fixture["source_revision"].as_str().is_some());

    let initialize: Value = serde_json::from_str(INITIALIZE).unwrap();
    let expected_parse_errors: Vec<_> = initialize["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["parse_error"].as_bool() == Some(true))
        .map(|case| case["name"].as_str().unwrap())
        .collect();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10, "six parse cases plus four boundaries");
    assert_eq!(
        cases[..6]
            .iter()
            .map(|case| case["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        expected_parse_errors
    );
    assert_eq!(cases[6]["name"], "size-below-8k");
    assert_eq!(cases[7]["name"], "size-above-8k");
    assert_eq!(cases[8]["name"], "nesting-at-go-limit");
    assert_eq!(cases[9]["name"], "nesting-over-go-limit");

    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = input_from_spec(&case["input_spec"]);
        if let Some(size) = case["input_spec"]["size"].as_u64() {
            assert_eq!(input.len() as u64, size, "{name}: requested input size");
        }
        let (mut child, root) = isolated_child();
        let result = run_bounded(&mut child, input);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(
            result.0.code(),
            case["exit_code"].as_i64().map(|n| n as i32),
            "{name}: exit, stdout={:?}, stderr={:?}",
            String::from_utf8_lossy(&result.1),
            String::from_utf8_lossy(&result.2)
        );
        assert_eq!(
            result.1,
            STANDARD
                .decode(case["stdout_base64"].as_str().unwrap())
                .unwrap(),
            "{name}: stdout bytes"
        );
        assert_eq!(
            result.2,
            STANDARD
                .decode(case["stderr_base64"].as_str().unwrap())
                .unwrap(),
            "{name}: stderr bytes"
        );
    }
}
