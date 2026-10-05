#![cfg(unix)]

#[path = "support/frozen_unix_process.rs"]
mod frozen_unix_process;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use symeraseme_core::storage::Store;
use symeraseme_core::storage::repository::Repository;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/mcp-agent-error-json/error.json"
));
const AGENT_SCRIPT: &str = "#!/bin/sh\nprintf '%b' \"$AGENT_STDERR_ESCAPED\" >&2\nexit 23\n";

#[derive(Deserialize)]
struct Fixture {
    schema: String,
    go_version: String,
    sources_sha256: BTreeMap<String, String>,
    agent_script_sha256: String,
    agent_stderr_escaped: String,
    agent_stderr_bytes_base64: String,
    request_base64: String,
    response_base64: String,
}

#[test]
fn mcp_json_normalizes_raw_host_agent_error_like_go() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("fixture parses");
    assert_eq!(
        fixture.schema,
        "symeraseme.go-oracle.mcp-agent-error-json.v1"
    );
    assert_eq!(fixture.go_version, "go1.26.6");
    assert_eq!(sha256(AGENT_SCRIPT.as_bytes()), fixture.agent_script_sha256);
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(&fixture.agent_stderr_bytes_base64)
            .expect("Go recorded stderr bytes"),
        b"before\xf0\x80\x80after\xffend"
    );
    for (path, expected) in &fixture.sources_sha256 {
        let source = match path.as_str() {
            "go.mod" => include_bytes!("../../../go.mod").as_slice(),
            "internal/mcp/server.go" => {
                include_bytes!("../../../internal/mcp/server.go").as_slice()
            }
            "internal/mcp/contract_handler.go" => {
                include_bytes!("../../../internal/mcp/contract_handler.go").as_slice()
            }
            "internal/llm/agent.go" => include_bytes!("../../../internal/llm/agent.go").as_slice(),
            "internal/llm/llm.go" => include_bytes!("../../../internal/llm/llm.go").as_slice(),
            "internal/triage/classifier.go" => {
                include_bytes!("../../../internal/triage/classifier.go").as_slice()
            }
            "internal/eventstore/store.go" => {
                include_bytes!("../../../internal/eventstore/store.go").as_slice()
            }
            "rust-tests/parity/oracle/mcp-agent-error-json/main.go" => {
                include_bytes!("../../../rust-tests/parity/oracle/mcp-agent-error-json/main.go")
                    .as_slice()
            }
            other => panic!("unexpected pinned source {other}"),
        };
        assert_eq!(sha256(source), *expected, "Go source changed: {path}");
    }

    let observed = frozen_unix_process::observation("mcp-agent-error-json").unwrap_or_else(|| {
        let oracle = Command::new("go")
            .args(["run", "./rust-tests/parity/oracle/mcp-agent-error-json"])
            .current_dir(ROOT)
            .env("GOTOOLCHAIN", "go1.26.6")
            .env("GOPROXY", "off")
            .env("GOSUMDB", "off")
            .output()
            .expect("run Go MCP JSON oracle");
        assert!(
            oracle.status.success(),
            "Go oracle failed: {}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        oracle.stdout
    });
    assert_eq!(observed, FIXTURE.as_bytes(), "Go fixture drifted");

    let request = base64::engine::general_purpose::STANDARD
        .decode(&fixture.request_base64)
        .expect("Go recorded MCP frame");
    let expected = base64::engine::general_purpose::STANDARD
        .decode(&fixture.response_base64)
        .expect("Go recorded MCP response");
    let root = TestRoot::new();
    let home = root.path().join("home");
    let data = root.path().join("data");
    let bin = root.path().join("bin");
    for directory in [&home, &data, &bin] {
        fs::create_dir_all(directory).expect("isolated test directory");
    }
    let fake_agent = bin.join("claude");
    fs::write(&fake_agent, AGENT_SCRIPT).expect("fake agent written");
    let mut permissions = fs::metadata(&fake_agent)
        .expect("fake agent metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&fake_agent, permissions).expect("fake agent executable");
    seed(&data);

    let mut child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(["mcp", "--stdio"])
        .current_dir(root.path())
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("PATH", &bin)
        .env("SYMERASEME_DATA_DIR", &data)
        .env("SYMERASEME_LLM_PROVIDER", "agent")
        .env("SYMERASEME_AGENT_BACKEND", "claude")
        .env("AGENT_STDERR_ESCAPED", &fixture.agent_stderr_escaped)
        .env("TERM", "dumb")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Rust MCP process");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("MCP stdin")
        .write_all(&request)
        .expect("write MCP request");
    let output = child.wait_with_output().expect("wait for Rust MCP process");
    assert!(
        output.status.success(),
        "Rust MCP exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "MCP stdio stays clean");
    assert_eq!(output.stdout, expected, "MCP JSON bytes match Go");
}

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

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let stamp = format!("{stamp}-{}", unique_seq());
        let path = std::env::temp_dir().join(format!(
            "mcp-agent-error-json-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Per-process sequence: macOS clocks tick in microseconds, so pid+nanos alone
/// collides when parallel tests create their directories at the same instant.
fn unique_seq() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
