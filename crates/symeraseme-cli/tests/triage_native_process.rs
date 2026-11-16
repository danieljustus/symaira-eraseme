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
#[path = "support/frozen_native_triage.rs"]
mod frozen_native_triage;

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

fn prepare(logs: &Path, live: bool) -> (Option<(PathBuf, PathBuf)>, PathBuf) {
    let go = live.then(|| oracle(logs));
    let bin = logs.join("bin");
    if live {
        fs::create_dir(&bin).unwrap();
    }
    let agent = bin.join(if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    });
    if live {
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
    }
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
    (go.map(|binary| (binary, bin)), rust_bin)
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

fn assert_process_and_state(go: &Value, rust: Output, rust_root: &Path, case: &str) {
    assert_eq!(
        rust.status.code().map(i64::from),
        go["exit_status"].as_i64(),
        "{case} exit"
    );
    let decode = |stream: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(go[format!("{stream}_base64")].as_str().unwrap())
            .unwrap()
    };
    assert_eq!(rust.stdout, decode("stdout"), "{case} stdout");
    assert_eq!(
        normalize_provider_order(&rust.stderr),
        normalize_provider_order(&decode("stderr")),
        "{case} stderr"
    );
    assert_eq!(
        serde_json::to_value(read_snapshot(&rust_root.join("data"))).unwrap(),
        go["saved_reply_and_ordered_events"],
        "{case} saved reply and ordered events"
    );
    assert_eq!(
        fs::read_to_string(rust_root.join("agent-control")).unwrap_or_default(),
        go["actual_agent_invocations"].as_str().unwrap(),
        "{case} actual agent calls"
    );
}

fn execute(
    binary: &Path,
    directory: &Path,
    bin: &Path,
    input: &Value,
    logs: &Path,
    label: &str,
) -> Output {
    let mut child = command(binary, directory, bin);
    for arg in input["argv"].as_array().unwrap() {
        child.arg(arg.as_str().unwrap());
    }
    for (key, value) in input["environment"].as_object().unwrap() {
        child.env(key, value.as_str().unwrap());
    }
    let stdin = base64::engine::general_purpose::STANDARD
        .decode(input["stdin_base64"].as_str().unwrap())
        .unwrap();
    if stdin.is_empty() {
        child.stdin(Stdio::null());
    } else {
        let frame = logs.join(format!("{label}.input"));
        fs::write(&frame, stdin).unwrap();
        child.stdin(fs::File::open(&frame).unwrap());
    }
    capture(child, logs, label, Duration::from_secs(30))
}

fn go_observation(
    live: Option<&(PathBuf, PathBuf)>,
    corpus: Option<&frozen_native_triage::Corpus>,
    root: &Path,
    id: &str,
    input: &Value,
) -> Value {
    if let Some(corpus) = corpus {
        return corpus.case(id, input);
    }
    let (binary, bin) = live.expect("unrecorded targets require an actual native Go process");
    let directory = root.join(format!("go-{id}"));
    let output = execute(binary, &directory, bin, input, root, &format!("go-{id}"));
    observed(&output, &directory, id, input.clone())
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
    let corpus = frozen_native_triage::observations("cli");
    let (go, rust_bin) = prepare(root.path(), corpus.is_none());
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
        let rust_root = root.path().join(format!("rust-{id}"));
        let input = json!({"argv": argv, "environment": case["environment"], "stdin_base64": ""});
        let measured = go_observation(go.as_ref(), corpus.as_ref(), root.path(), id, &input);
        let rust = execute(
            Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
            &rust_root,
            &rust_bin,
            &input,
            root.path(),
            &format!("rust-{id}"),
        );
        if case["exit_code"] == 0 || id == "classify-agent-invalid-utf8-stderr" {
            assert!(
                !measured["actual_agent_invocations"]
                    .as_str()
                    .unwrap()
                    .is_empty(),
                "{id}: Go never invoked the native fixture"
            );
            assert!(
                !fs::read(rust_root.join("agent-control"))
                    .unwrap()
                    .is_empty(),
                "{id}: Rust never invoked the native fixture"
            );
            positive_controls += 1;
        }
        recorded.push(measured.clone());
        assert_eq!(
            measured["exit_status"].as_i64(),
            case["exit_code"].as_i64(),
            "{id}: native Go must reach the retained success/error class"
        );
        assert_process_and_state(&measured, rust, &rust_root, id);
    }
    assert!(positive_controls >= 9);
    if let Some((binary, _)) = &go {
        capture_native_triage::record(binary, "cli", &recorded);
    }
    eprintln!(
        "native CLI triage executed all 16 measured Go/Rust cases and {positive_controls} agent controls"
    );
}

#[test]
fn native_mcp_triage_matches_go_raw_frames_and_saved_effects() {
    let root = tempfile::tempdir().unwrap();
    let corpus = frozen_native_triage::observations("mcp");
    let (go, rust_bin) = prepare(root.path(), corpus.is_none());
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
        let mut frame = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}})).unwrap();
        frame.push(b'\n');
        let input = json!({"argv": ["mcp", "--stdio"], "environment": {"SYMERASEME_LLM_PROVIDER": "agent"}, "stdin_base64": base64::engine::general_purpose::STANDARD.encode(frame)});
        let rust_root = root.path().join(format!("rust-{id}"));
        let measured = go_observation(go.as_ref(), corpus.as_ref(), root.path(), id, &input);
        let rust = execute(
            Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
            &rust_root,
            &rust_bin,
            &input,
            root.path(),
            &format!("rust-{id}"),
        );
        if id.ends_with("save") || id == "classify-float" {
            assert!(
                !measured["actual_agent_invocations"]
                    .as_str()
                    .unwrap()
                    .is_empty(),
                "{id}: missing native Go agent control"
            );
            assert!(
                !fs::read(rust_root.join("agent-control"))
                    .unwrap()
                    .is_empty(),
                "{id}: missing native Rust agent control"
            );
        }
        recorded.push(measured.clone());
        assert_eq!(measured["exit_status"], 0);
        let decode = |stream: &str| {
            base64::engine::general_purpose::STANDARD
                .decode(measured[format!("{stream}_base64")].as_str().unwrap())
                .unwrap()
        };
        assert!(decode("stderr").is_empty());
        let reply: Value = serde_json::from_slice(&decode("stdout")).unwrap();
        let positive = id.ends_with("save")
            || id == "classify-float"
            || id == "rebuttal-empty-message-fallback";
        assert!(
            reply
                .get(if positive { "result" } else { "error" })
                .is_some(),
            "{id}: native Go must exercise the intended result/error boundary"
        );
        assert_process_and_state(&measured, rust, &rust_root, id);
    }
    if let Some((binary, _)) = &go {
        capture_native_triage::record(binary, "mcp", &recorded);
    }
    eprintln!(
        "native MCP triage executed all eight measured Go/Rust raw-frame and saved-effect comparisons"
    );
}
