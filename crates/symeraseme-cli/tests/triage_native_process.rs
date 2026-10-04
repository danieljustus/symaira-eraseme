//! Native CLI and MCP triage differentials with a compiled local agent.
use base64::Engine;
use serde::Serialize;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use symeraseme_core::storage::{Repository, Store};
#[path = "support/mcp_http_port.rs"]
#[allow(dead_code)]
mod mcp_http_port;
use mcp_http_port::StartedChild;
#[path = "support/capture_native_triage.rs"]
mod capture_native_triage;

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
    let binary = logs.join("go-triage.exe");
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

#[derive(Debug, PartialEq, Serialize)]
struct Snapshot {
    classification: Option<String>,
    confidence: Option<f64>,
    summary: Option<String>,
    events: Vec<Event>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Event {
    request_id: i64,
    event_type: String,
    source: String,
    payload_json: String,
}

fn seed_store(data_dir: &Path) {
    let store = Store::open(data_dir.join("symeraseme.db")).expect("seed store open");
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
            "INSERT INTO inbox_replies (request_id, message_id, thread_id, from_addr, subject, snippet)\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                request_id,
                "oracle-message",
                "oracle-thread",
                "privacy@example.invalid",
                "We need your current address",
                "Your current address does not match our records.",
            ),
        )
        .expect("seed inbox reply");
}

fn read_snapshot(data_dir: &Path) -> Snapshot {
    let store = Store::open(data_dir.join("symeraseme.db")).expect("result store open");
    let (classification, confidence, summary) = store
        .connection()
        .query_row(
            "SELECT classified_as, classifier_confidence, llm_summary FROM inbox_replies WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read reply result");
    let mut statement = store
        .connection()
        .prepare(
            "SELECT request_id, event_type, source, payload_json FROM request_events ORDER BY id",
        )
        .expect("prepare events");
    let events = statement
        .query_map([], |row| {
            Ok(Event {
                request_id: row.get(0)?,
                event_type: row.get(1)?,
                source: row.get(2)?,
                payload_json: row.get(3)?,
            })
        })
        .expect("query events")
        .map(|row| row.expect("event row"))
        .collect();
    Snapshot {
        classification,
        confidence,
        summary,
        events,
    }
}

// Go iterates its provider registry map in randomized order in unknown-provider
// errors. Canonicalize only that list so the fixture stays stable without hiding
// missing or unexpected providers.
fn normalize_provider_order(stderr: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(stderr);
    let Some(index) = text.find("Known providers: ") else {
        return stderr.to_vec();
    };
    let start = index + "Known providers: ".len();
    let end = text[start..]
        .find('\n')
        .map_or(text.len(), |offset| start + offset);
    let mut providers = text[start..end].split(", ").collect::<Vec<_>>();
    providers.sort_unstable();
    format!("{}{}{}", &text[..start], providers.join(", "), &text[end..]).into_bytes()
}

fn prepare(logs: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let go = oracle(logs);
    let bin = logs.join("bin");
    fs::create_dir(&bin).unwrap();
    let agent = bin.join(if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    });
    let mut build = Command::new("go");
    build
        .args(["build", "-o"])
        .arg(&agent)
        .arg("./rust-tests/parity/oracle/native-triage-agent")
        .current_dir(repo())
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off");
    let output = capture(build, logs, "agent-build", Duration::from_secs(180));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rust_bin = logs.join("rust-bin");
    fs::create_dir(&rust_bin).unwrap();
    let rust_agent = rust_bin.join(agent.file_name().unwrap());
    let mut build = Command::new("rustc");
    build
        .args(["+1.98.0", "--edition=2024", "--crate-type=bin", "-o"])
        .arg(&rust_agent)
        .arg(repo().join("crates/symeraseme-cli/tests/fixtures/native_triage_agent.rs"));
    let output = capture(build, logs, "rust-agent-build", Duration::from_secs(60));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (go, bin, rust_bin)
}

fn command(binary: &Path, root: &Path, bin: &Path) -> Command {
    let data = root.join("data");
    fs::create_dir_all(&data).unwrap();
    seed_store(&data);
    let mut command = Command::new(binary);
    private(&mut command, root);
    command
        .env("PATH", bin)
        .env("PATHEXT", ".EXE")
        .env("TERM", "dumb")
        .env("SYMERASEME_AGENT_BACKEND", "claude")
        .env(
            "SYMERASEME_NATIVE_AGENT_CONTROL",
            root.join("agent-control"),
        );
    command
}

fn assert_process_and_state(
    go: Output,
    rust: Output,
    go_root: &Path,
    rust_root: &Path,
    case: &str,
) {
    assert_eq!(rust.status.code(), go.status.code(), "{case} exit");
    assert_eq!(rust.stdout, go.stdout, "{case} stdout");
    assert_eq!(
        normalize_provider_order(&rust.stderr),
        normalize_provider_order(&go.stderr),
        "{case} stderr"
    );
    assert_eq!(
        read_snapshot(&rust_root.join("data")),
        read_snapshot(&go_root.join("data")),
        "{case} saved reply and ordered events"
    );
    let invocations =
        |root: &Path| fs::read_to_string(root.join("agent-control")).unwrap_or_default();
    assert_eq!(
        invocations(rust_root),
        invocations(go_root),
        "{case} actual agent calls"
    );
}

fn observed(output: &Output, root: &Path, id: &str, input: Value) -> Value {
    json!({
        "id": id, "input": input,
        "exit_status": output.status.code().unwrap(),
        "stdout_bytes": output.stdout.len(), "stderr_bytes": output.stderr.len(),
        "stdout_base64": base64::engine::general_purpose::STANDARD.encode(&output.stdout),
        "stderr_base64": base64::engine::general_purpose::STANDARD.encode(&output.stderr),
        "saved_reply_and_ordered_events": read_snapshot(&root.join("data")),
        "actual_agent_invocations": fs::read_to_string(root.join("agent-control")).unwrap_or_default(),
    })
}

#[test]
fn native_cli_triage_matches_go_for_every_retained_case() {
    let root = tempfile::tempdir().unwrap();
    let (go, bin, rust_bin) = prepare(root.path());
    let cases: Value = serde_json::from_slice(
        &fs::read(repo().join("tests/fixtures/cli-triage/cases.json")).unwrap(),
    )
    .unwrap();
    let cases = cases["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    let mut positive_controls = 0;
    let mut recorded = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let argv = case["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>();
        let go_root = root.path().join(format!("go-{id}"));
        let rust_root = root.path().join(format!("rust-{id}"));
        let mut outputs = Vec::new();
        for (label, binary, directory) in [
            ("go", go.as_path(), &go_root),
            (
                "rust",
                Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
                &rust_root,
            ),
        ] {
            let mut child = command(
                binary,
                directory,
                if label == "go" { &bin } else { &rust_bin },
            );
            child.args(&argv).stdin(Stdio::null());
            for (key, value) in case["environment"].as_object().unwrap() {
                child.env(key, value.as_str().unwrap());
            }
            outputs.push(capture(
                child,
                root.path(),
                &format!("{label}-{id}"),
                Duration::from_secs(30),
            ));
        }
        if case["exit_code"] == 0 || id == "classify-agent-invalid-utf8-stderr" {
            assert!(
                !fs::read(go_root.join("agent-control")).unwrap().is_empty(),
                "{id}: Go never invoked the native fixture"
            );
            positive_controls += 1;
        }
        let rust = outputs.pop().unwrap();
        let go = outputs.pop().unwrap();
        recorded.push(observed(
            &go,
            &go_root,
            id,
            json!({
                "argv": argv, "environment": case["environment"], "stdin_base64": "",
            }),
        ));
        assert_eq!(
            go.status.code().map(i64::from),
            case["exit_code"].as_i64(),
            "{id}: native Go must reach the retained success/error class"
        );
        assert_process_and_state(go, rust, &go_root, &rust_root, id);
    }
    assert!(positive_controls >= 9);
    capture_native_triage::record(&go, "cli", &recorded);
    eprintln!(
        "native CLI triage executed all 16 actual Go/Rust cases and {positive_controls} agent controls"
    );
}

#[test]
fn native_mcp_triage_matches_go_raw_frames_and_saved_effects() {
    let root = tempfile::tempdir().unwrap();
    let (go, bin, rust_bin) = prepare(root.path());
    let cases = [
        (
            "classify-nosave",
            "classify_reply",
            json!({"request_id":1,"save":false}),
        ),
        (
            "classify-save",
            "classify_reply",
            json!({"request_id":1,"save":true}),
        ),
        (
            "classify-float",
            "classify_reply",
            json!({"request_id":1.0,"save":false}),
        ),
        (
            "classify-missing",
            "classify_reply",
            json!({"request_id":999,"save":false}),
        ),
        (
            "classify-invalid",
            "classify_reply",
            json!({"request_id":"bad"}),
        ),
        (
            "rebuttal-nosave",
            "generate_rebuttal",
            json!({"request_id":1,"save":false}),
        ),
        (
            "rebuttal-save",
            "generate_rebuttal",
            json!({"request_id":1,"save":true}),
        ),
        (
            "rebuttal-empty-message-fallback",
            "generate_rebuttal",
            json!({"request_id":999,"save":false}),
        ),
    ];
    let mut recorded = Vec::new();
    for (id, name, arguments) in cases {
        let frame = root.path().join(format!("{id}.input"));
        let mut input = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}})).unwrap();
        input.push(b'\n');
        fs::write(&frame, input).unwrap();
        let go_root = root.path().join(format!("go-{id}"));
        let rust_root = root.path().join(format!("rust-{id}"));
        let mut outputs = Vec::new();
        for (label, binary, directory) in [
            ("go", go.as_path(), &go_root),
            (
                "rust",
                Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
                &rust_root,
            ),
        ] {
            let mut child = command(
                binary,
                directory,
                if label == "go" { &bin } else { &rust_bin },
            );
            child
                .args(["mcp", "--stdio"])
                .env("SYMERASEME_LLM_PROVIDER", "agent")
                .stdin(fs::File::open(&frame).unwrap());
            outputs.push(capture(
                child,
                root.path(),
                &format!("{label}-{id}"),
                Duration::from_secs(30),
            ));
        }
        if id.ends_with("save") || id == "classify-float" {
            assert!(
                !fs::read(go_root.join("agent-control")).unwrap().is_empty(),
                "{id}: missing native Go agent control"
            );
        }
        let rust = outputs.pop().unwrap();
        let go = outputs.pop().unwrap();
        recorded.push(observed(&go, &go_root, id, json!({
            "argv": ["mcp", "--stdio"],
            "environment": {"SYMERASEME_LLM_PROVIDER": "agent"},
            "stdin_base64": base64::engine::general_purpose::STANDARD.encode(fs::read(&frame).unwrap()),
        })));
        assert!(go.status.success() && go.stderr.is_empty());
        let reply: Value = serde_json::from_slice(&go.stdout).unwrap();
        let positive = id.ends_with("save")
            || id == "classify-float"
            || id == "rebuttal-empty-message-fallback";
        assert!(
            reply
                .get(if positive { "result" } else { "error" })
                .is_some(),
            "{id}: native Go must exercise the intended result/error boundary"
        );
        assert_process_and_state(go, rust, &go_root, &rust_root, id);
    }
    capture_native_triage::record(&go, "mcp", &recorded);
    eprintln!(
        "native MCP triage executed all eight actual Go/Rust raw-frame and saved-effect comparisons"
    );
}
