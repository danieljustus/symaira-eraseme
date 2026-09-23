use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::{AgentClient, AgentCommandConfig, ClientError};

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-provider-surface/cases.json"
));

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
        ClientError::TransportNotPorted { .. } => "ClientError",
        ClientError::Foreign(_) => "*errors.errorString",
    }
}

#[test]
fn host_agent_subprocess_protocol_matches_real_go_oracle() {
    let fixture = fixture();
    let protocol = &fixture["host_agent_protocol"];
    let program = protocol["program"].as_str().expect("fake CLI program");
    let cases = protocol["cases"].as_array().expect("host-agent cases");
    assert_eq!(
        cases.len(),
        3,
        "success, exit-code, and timeout cases execute"
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
        let cli = bin.join("claude");
        fs::write(&cli, program).expect("write Go-oracled fake CLI");
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700))
            .expect("make fake CLI executable");

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
        let mut agent =
            AgentClient::with_probe(model, "claude", Vec::new(), &|name| name == "claude");
        agent.base.max_retries = 1;
        let timeout = Duration::from_millis(
            case["test_timeout_millis"]
                .as_u64()
                .filter(|millis| *millis > 0)
                .unwrap_or(5000),
        );
        let result = agent.classify_with_command(
            &case["system_prompt"].as_str().expect("system prompt"),
            &case["user_prompt"].as_str().expect("user prompt"),
            &super::ClassifyOptions::default(),
            AgentCommandConfig {
                timeout,
                executable_override: Some(&cli),
                environment: &environment,
                inherit_environment: false,
            },
        );

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

        let _ = fs::remove_dir_all(root);
    }
}
