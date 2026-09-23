#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use symeraseme_core::storage::Store;
use symeraseme_core::storage::repository::Repository;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const AGENT: &str = include_str!("../../../rust-tests/parity/oracle/mcp-triage/agent.sh");

#[test]
fn mcp_triage_matches_live_go_handler_with_local_agent() {
    let go = Command::new("go")
        .args(["run", "./rust-tests/parity/oracle/mcp-triage"])
        .current_dir(ROOT)
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .output()
        .expect("run Go MCP triage oracle");
    assert!(
        go.status.success(),
        "{}",
        String::from_utf8_lossy(&go.stderr)
    );
    let cases: Vec<Value> = serde_json::from_slice(&go.stdout).expect("Go observations");
    assert_eq!(cases.len(), 3);

    for case in cases {
        let root = TestRoot::new();
        let home = root.0.join("home");
        let data = root.0.join("data");
        let bin = root.0.join("bin");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("claude");
        fs::write(&fake, AGENT).unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        seed(&data);

        let arguments = case["wire_arguments"]
            .as_str()
            .map(|wire| serde_json::from_str::<Value>(wire).expect("numeric wire arguments"))
            .unwrap_or_else(|| case["arguments"].clone());
        let frame = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": case["name"], "arguments": arguments},
        });
        let mut input = serde_json::to_vec(&frame).unwrap();
        input.push(b'\n');
        let mut child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
            .args(["mcp", "--stdio"])
            .current_dir(&root.0)
            .env_clear()
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("PATH", &bin)
            .env("SYMERASEME_DATA_DIR", &data)
            .env("SYMERASEME_LLM_PROVIDER", "agent")
            .env("SYMERASEME_AGENT_BACKEND", "claude")
            .env("TERM", "dumb")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(&input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            response["result"]["content"][0]["text"], case["result"],
            "{}",
            case["name"]
        );
    }
}

fn seed(data: &Path) {
    let store = Store::open(data.join("symeraseme.db")).unwrap();
    let request_id = Repository::new(&store)
        .create_removal_request(
            "oracle-broker",
            "email",
            "oracle-campaign",
            "DE",
            "gdpr-art17.de.md.j2",
            "",
        )
        .unwrap();
    store.connection().execute(
        "INSERT INTO inbox_replies (request_id, message_id, thread_id, from_addr, subject, snippet) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (request_id, "oracle-message", "oracle-thread", "privacy@example.invalid", "We need your current address", "Your current address does not match our records."),
    ).unwrap();
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mcp-triage-{}-{stamp}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
