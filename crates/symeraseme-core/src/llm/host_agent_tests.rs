use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{AgentClient, AgentCommandConfig, CancellationToken, ClientError};

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-provider-surface/cases.json"
));
const INVALID_UTF8_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/agent-stderr/invalid-utf8.json"
));
const CANCEL_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/agent-cancel/http.json"
));

/// Linux can fail `execve` of a freshly written fake agent with ETXTBSY while a
/// concurrently forked test child still holds the inherited write descriptor
/// (closed only at its own exec). The child never ran, so retrying is safe.
fn retry_text_file_busy<T>(
    mut call: impl FnMut() -> Result<T, ClientError>,
) -> Result<T, ClientError> {
    for _ in 0..50 {
        match call() {
            Err(error) if error.to_string().contains("Text file busy") => {
                std::thread::sleep(Duration::from_millis(20));
            }
            result => return result,
        }
    }
    call()
}

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("the recorded host-agent fixture parses")
}

fn nul_strings(path: &std::path::Path) -> Vec<String> {
    let bytes = fs::read(path).expect("fake agent capture exists");
    assert_eq!(bytes.last(), Some(&0), "capture ends in NUL");
    bytes[..bytes.len() - 1]
        .split(|byte| *byte == 0)
        .map(|part| String::from_utf8(part.to_vec()).expect("captured UTF-8"))
        .collect()
}

fn unique_root() -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("llm-agent-{}-{nonce}", std::process::id()))
}

fn error_type(error: &ClientError) -> &'static str {
    match error {
        ClientError::Provider(_) => "Error",
        ClientError::RateLimit(_) => "RateLimitError",
        ClientError::UnknownProvider(_) => "ProviderError",
        ClientError::Context(_) => "context deadline exceeded",
        ClientError::RetriesExhausted { .. } => "*fmt.wrapError",
        ClientError::Foreign(_) => "*errors.errorString",
    }
}

#[test]
fn host_agent_subprocess_protocol_matches_real_go_oracle() {
    let fixture = fixture();
    let protocol = &fixture["host_agent_protocol"];
    let source = Sha256::digest(include_bytes!("../../../../internal/llm/agent.go"));
    assert_eq!(
        hex::encode(source),
        protocol["sources_sha256"]["internal/llm/agent.go"]
            .as_str()
            .expect("source digest")
    );
    let program = protocol["program"].as_str().expect("fake CLI program");
    let cases = protocol["cases"].as_array().expect("host-agent cases");
    assert_eq!(
        cases.len(),
        6,
        "all three backends and process failure cases execute"
    );

    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let root = unique_root();
        let bin = root.join("bin");
        let capture = root.join("capture");
        let home = root.join("home");
        let tmp = root.join("tmp");
        for path in [&bin, &capture, &home, &tmp] {
            fs::create_dir_all(path).expect("isolated fake-agent directory");
        }
        let cli = bin.join(case["cli"].as_str().expect("CLI executable"));
        fs::write(&cli, program).expect("write Go-oracled fake CLI");
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700))
            .expect("make fake CLI executable");
        let spawn_error = case["mode"] == "spawn";
        if spawn_error {
            // The Go oracle caches availability first, then the executable
            // disappears before Classify resolves it from PATH.
            fs::remove_file(&cli).expect("remove fake CLI after backend detection");
        }

        let environment = vec![
            (OsString::from("PATH"), bin.as_os_str().to_owned()),
            (OsString::from("HOME"), home.as_os_str().to_owned()),
            (OsString::from("TMPDIR"), tmp.as_os_str().to_owned()),
            (OsString::from("LC_ALL"), OsString::from("C")),
            (OsString::from("TZ"), OsString::from("UTC")),
            (OsString::from("TERM"), OsString::from("oracle-term")),
            (
                OsString::from("AGENT_CAPTURE_DIR"),
                capture.as_os_str().to_owned(),
            ),
            (
                OsString::from("AGENT_SCENARIO"),
                OsString::from(case["mode"].as_str().expect("mode")),
            ),
            (
                OsString::from("AGENT_SENTINEL"),
                OsString::from("environment-survived"),
            ),
            (
                OsString::from("AGENT_STDERR"),
                OsString::from(case["stderr"].as_str().unwrap_or("")),
            ),
            (
                OsString::from("AGENT_EXIT_CODE"),
                OsString::from(case["exit_code"].as_i64().unwrap_or_default().to_string()),
            ),
            (
                OsString::from("SYMERASEME_LLM_PROVIDER"),
                OsString::from("agent"),
            ),
        ];

        let model = case["model"].as_str().expect("model");
        let backend = case["backend"].as_str().expect("backend");
        let cli_name = case["cli"].as_str().expect("CLI executable");
        let mut agent =
            AgentClient::with_probe(model, backend, Vec::new(), &|name| name == cli_name);
        agent.base.max_retries = 1;
        let timeout = Duration::from_millis(
            case["test_timeout_millis"]
                .as_u64()
                .filter(|millis| *millis > 0)
                .unwrap_or(5000)
                .max(1000), // Let the fake shell start even under parallel CI load.
        );
        let result = retry_text_file_busy(|| {
            agent.classify_with_command(
                case["system_prompt"].as_str().expect("system prompt"),
                case["user_prompt"].as_str().expect("user prompt"),
                &super::ClassifyOptions::default(),
                AgentCommandConfig {
                    timeout,
                    executable_override: (!spawn_error).then_some(cli.as_path()),
                    environment: &environment,
                    inherit_environment: false,
                },
                &CancellationToken::default(),
            )
        });

        let expected = &case["result"];
        match result {
            Ok((text, usage)) => {
                assert!(expected["error"].is_null(), "{id} unexpectedly succeeded");
                assert_eq!(text, expected["text"].as_str().unwrap_or(""), "{id} text");
                assert_eq!(
                    usage.model,
                    expected["usage_model"].as_str().unwrap_or(""),
                    "{id} usage model"
                );
            }
            Err(error) => {
                assert_eq!(
                    error.to_string(),
                    expected["error"].as_str().expect("expected error"),
                    "{id} error text"
                );
                assert_eq!(
                    error_type(&error),
                    expected["error_type"].as_str().expect("error type"),
                    "{id} error type"
                );
            }
        }

        if spawn_error {
            assert!(expected["arguments"].is_null(), "{id} has no argv");
            assert!(!capture.join("arguments").exists(), "{id} did not run CLI");
            assert!(
                !capture.join("environment").exists(),
                "{id} did not run CLI"
            );
            assert!(!capture.join("stdin").exists(), "{id} did not run CLI");
        } else {
            let expected_args = expected["arguments"]
                .as_array()
                .expect("recorded arguments")
                .iter()
                .map(|value| value.as_str().expect("argument string").to_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                nul_strings(&capture.join("arguments")),
                expected_args,
                "{id} argv"
            );

            let environment_capture = nul_strings(&capture.join("environment"));
            assert_eq!(environment_capture.len(), 2, "{id} env fields");
            assert_eq!(
                environment_capture[0],
                expected["environment"]["TERM"].as_str().expect("TERM"),
                "{id} TERM"
            );
            assert_eq!(
                environment_capture[1],
                expected["environment"]["AGENT_SENTINEL"]
                    .as_str()
                    .expect("sentinel"),
                "{id} inherited environment"
            );
            assert_eq!(
                fs::read_to_string(capture.join("stdin")).expect("stdin capture"),
                if expected["stdin_eof"].as_bool().expect("stdin contract") {
                    "eof"
                } else {
                    "input"
                },
                "{id} stdin"
            );
        }

        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn invalid_utf8_agent_stderr_matches_go_byte_truncation() {
    let fixture: Value =
        serde_json::from_str(INVALID_UTF8_FIXTURE).expect("Go stderr fixture parses");
    assert_eq!(fixture["schema"], "symeraseme.go-oracle.agent-stderr.v1");
    assert_eq!(fixture["go_version"], "go1.26.6");
    for (path, expected) in fixture["sources_sha256"].as_object().unwrap() {
        let source: &[u8] = match path.as_str() {
            "internal/llm/agent.go" => include_bytes!("../../../../internal/llm/agent.go"),
            "go.mod" => include_bytes!("../../../../go.mod"),
            other => panic!("unexpected source {other}"),
        };
        assert_eq!(
            hex::encode(Sha256::digest(source)),
            expected.as_str().unwrap(),
            "{path}"
        );
    }

    let root = unique_root();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("isolated fake-agent root");
    let cli = root.join("claude");
    fs::write(
        &cli,
        "#!/bin/sh\nprintf '%b' \"$AGENT_STDERR_ESCAPED\" >&2\nexit 23\n",
    )
    .expect("write fake agent");
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700))
        .expect("make fake agent executable");

    let environment = [(
        OsString::from("AGENT_STDERR_ESCAPED"),
        OsString::from(
            fixture["stderr_printf_escape"]
                .as_str()
                .expect("Go-recorded stderr bytes"),
        ),
    )];
    let mut agent = AgentClient::with_probe("auto", "claude", Vec::new(), &|_| true);
    agent.base.max_retries = 1;
    let result = retry_text_file_busy(|| {
        agent.classify_with_command(
            "system",
            "user",
            &Default::default(),
            AgentCommandConfig {
                timeout: Duration::from_secs(2),
                executable_override: Some(&cli),
                environment: &environment,
                inherit_environment: false,
            },
            &CancellationToken::default(),
        )
    });
    let error = result.expect_err("fake agent exits non-zero");
    assert_eq!(
        error.to_string(),
        fixture["error"].as_str().expect("Go-recorded error")
    );
    assert!(
        fixture["error_type"]
            .as_str()
            .unwrap()
            .ends_with("llm.Error")
    );
    assert_eq!(error_type(&error), "Error");

    fs::remove_dir_all(root).expect("remove isolated fake-agent root");
}

#[test]
fn cancelling_host_agent_kills_the_child_and_returns_context_canceled() {
    let fixture: Value = serde_json::from_str(CANCEL_FIXTURE).expect("Go cancel fixture parses");
    assert_eq!(fixture["schema"], "symeraseme.go-oracle.agent-cancel.v1");
    assert_eq!(fixture["go_version"], "go1.26.6");
    for (path, expected) in fixture["sources_sha256"].as_object().unwrap() {
        let source: &[u8] = match path.as_str() {
            "cmd/symeraseme/main.go" => {
                include_bytes!("../../../../cmd/symeraseme/main.go")
            }
            "internal/mcp/server.go" => include_bytes!("../../../../internal/mcp/server.go"),
            "internal/mcp/contract_handler.go" => {
                include_bytes!("../../../../internal/mcp/contract_handler.go")
            }
            "internal/llm/agent.go" => include_bytes!("../../../../internal/llm/agent.go"),
            "internal/llm/llm.go" => include_bytes!("../../../../internal/llm/llm.go"),
            "internal/llm/factory.go" => include_bytes!("../../../../internal/llm/factory.go"),
            "internal/triage/classifier.go" => {
                include_bytes!("../../../../internal/triage/classifier.go")
            }
            "internal/replies/service.go" => {
                include_bytes!("../../../../internal/replies/service.go")
            }
            "internal/eventstore/store.go" => {
                include_bytes!("../../../../internal/eventstore/store.go")
            }
            "go.mod" => include_bytes!("../../../../go.mod"),
            "rust-tests/parity/oracle/agent-cancel/main.go" => {
                include_bytes!("../../../../rust-tests/parity/oracle/agent-cancel/main.go")
            }
            other => panic!("unexpected source {other}"),
        };
        assert_eq!(
            hex::encode(Sha256::digest(source)),
            expected.as_str().unwrap(),
            "{path}"
        );
    }

    let root = unique_root();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("isolated fake-agent root");
    let cli = root.join("claude");
    let started = root.join("started");
    fs::write(
        &cli,
        "#!/bin/sh\nprintf '%s' \"$$\" > \"$AGENT_STARTED\"\nexec /bin/sleep 30\n",
    )
    .expect("write blocking fake agent");
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700))
        .expect("make fake agent executable");
    let environment = [(
        OsString::from("AGENT_STARTED"),
        started.as_os_str().to_owned(),
    )];
    let cancellation = CancellationToken::default();
    let cancel_after_start = cancellation.clone();
    let started_by_agent = started.clone();
    let cancel_thread = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !started_by_agent.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        cancel_after_start.cancel();
    });

    let mut agent = AgentClient::with_probe("auto", "claude", Vec::new(), &|_| true);
    agent.base.max_retries = 1;
    let started_at = std::time::Instant::now();
    let result = retry_text_file_busy(|| {
        agent.classify_with_command(
            "system",
            "user",
            &Default::default(),
            AgentCommandConfig {
                timeout: Duration::from_secs(30),
                executable_override: Some(&cli),
                environment: &environment,
                inherit_environment: false,
            },
            &cancellation,
        )
    });
    cancel_thread.join().expect("cancellation helper exits");
    assert!(
        started.exists(),
        "fake host agent started before cancellation"
    );
    assert!(
        started_at.elapsed() < Duration::from_secs(3),
        "cancellation interrupts the process instead of waiting for its timeout"
    );
    assert_eq!(
        result.expect_err("cancelled host agent returns an error"),
        ClientError::Context(fixture["handler_error"].as_str().unwrap().to_owned())
    );
    assert_eq!(fixture["child_exited"], true);
    fs::remove_dir_all(root).expect("remove isolated fake-agent root");
}
