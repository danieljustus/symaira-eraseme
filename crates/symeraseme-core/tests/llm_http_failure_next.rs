use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};
use symeraseme_core::llm::{ClassifyOptions, ClientError, CreateOptions, create_with};

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-failures-next/case.json"
));
const FIXTURE_404: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-failures-next/case-404.json"
));
const FIXTURE_400: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-failures-next/case-400.json"
));
const FIXTURE_500: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-failures-next/case-500.json"
));
const FIXTURE_401: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-failures-next/case-401.json"
));
const FIXTURE_MALFORMED_ENVELOPE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-failures-next/malformed-envelope.json"
));
const FIXTURE_MALFORMED_CHOICE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-failures-next/malformed-choice.json"
));
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[test]
fn llm_failure_fixtures_regenerate_from_pinned_go_oracle() {
    let cases = [
        (FIXTURE, &[][..]),
        (FIXTURE_404, &["--status", "404"][..]),
        (FIXTURE_400, &["--status", "400"][..]),
        (FIXTURE_500, &["--status", "500"][..]),
        (FIXTURE_401, &["--status", "401"][..]),
        (
            FIXTURE_MALFORMED_ENVELOPE,
            &["--status", "403", "--malformed-envelope"][..],
        ),
        (
            FIXTURE_MALFORMED_CHOICE,
            &["--status", "403", "--malformed-choice"][..],
        ),
    ];
    thread::scope(|scope| {
        let checks = cases.map(|(fixture, args)| {
            scope.spawn(move || assert_oracle_fixture_matches(fixture, args))
        });
        for check in checks {
            check.join().expect("Go fixture comparison thread");
        }
    });
}

#[test]
fn context_overflow_response_matches_go_with_secret_redaction() {
    assert_failure_matches_go(FIXTURE_400, 400);
}

#[test]
fn provider_server_error_matches_go_with_secret_redaction() {
    assert_failure_matches_go(FIXTURE_500, 500);
}

#[test]
fn structured_openai_auth_error_matches_go_with_secret_redaction() {
    assert_failure_matches_go(FIXTURE_401, 401);
}

#[test]
fn malformed_openai_response_envelope_keeps_go_status_error() {
    assert_failure_matches_go(FIXTURE_MALFORMED_ENVELOPE, 403);
}

#[test]
fn malformed_openai_choice_field_keeps_go_status_error() {
    assert_failure_matches_go(FIXTURE_MALFORMED_CHOICE, 403);
}

#[test]
fn forbidden_provider_response_matches_go_with_secret_redaction() {
    assert_failure_matches_go(FIXTURE, 403);
}

#[test]
fn missing_model_response_matches_go_with_secret_redaction() {
    assert_failure_matches_go(FIXTURE_404, 404);
}

fn assert_failure_matches_go(fixture_json: &str, expected_status: u16) {
    let fixture: Value = serde_json::from_str(fixture_json).expect("Go fixture parses");
    assert_eq!(
        fixture["schema"],
        "symeraseme.go-oracle.llm-failures-next.v1"
    );
    assert_eq!(
        fixture["go_module"],
        "github.com/danieljustus/symaira-corekit v0.16.2"
    );
    assert_eq!(fixture["go_version"], "go1.26.6");
    for (path, expected) in fixture["sources_sha256"].as_object().unwrap() {
        let digest = Sha256::digest(go_source(path));
        assert_eq!(hex::encode(digest), expected.as_str().unwrap(), "{path}");
    }

    let api_key = fixture["api_key"].as_str().unwrap();
    let attempts = fixture["attempts"].as_u64().unwrap() as usize;
    assert_eq!(fixture["status"], expected_status);
    let (base_url, server) =
        local_failure_server(attempts, expected_status, fixture["body"].as_str().unwrap());
    let client = create_with(
        &CreateOptions {
            provider: "openai".to_owned(),
            base_url,
            api_key: api_key.to_owned(),
            ..CreateOptions::default()
        },
        &|_| None,
        &|_| false,
    )
    .expect("local fake provider client");
    let result = client.classify("system", "user", &ClassifyOptions::default());
    let paths = server.join().expect("local provider server thread");

    assert_eq!(paths.len(), attempts, "Go retry count");
    let go_path = fixture["path"].as_str().unwrap();
    assert!(paths.iter().all(|path| path == go_path));
    assert_eq!(fixture["error_kind"], "provider");
    let error = result.expect_err("provider rejected the local request");
    assert!(matches!(error, ClientError::Provider(_)), "{error}");

    let go_message = fixture["message"].as_str().unwrap();
    let redacted_go_message = go_message.replace(api_key, "[REDACTED]");
    assert_ne!(
        go_message, redacted_go_message,
        "negative control: Go oracle contains the echoed synthetic key"
    );
    assert_ne!(
        error.to_string(),
        go_message,
        "negative control: Rust must not match the unredacted Go message"
    );
    assert_eq!(error.to_string(), redacted_go_message);
    assert!(
        !error.to_string().contains(api_key),
        "negative control: Rust must redact the echoed key"
    );
}

fn go_source(path: &str) -> &'static [u8] {
    match path {
        "internal/llm/factory.go" => include_bytes!("../../../internal/llm/factory.go"),
        "internal/llm/llmkit.go" => include_bytes!("../../../internal/llm/llmkit.go"),
        "internal/llm/llm.go" => include_bytes!("../../../internal/llm/llm.go"),
        "go.mod" => include_bytes!("../../../go.mod"),
        "go.sum" => include_bytes!("../../../go.sum"),
        "rust-tests/parity/oracle/llm-failures-next/main.go" => {
            include_bytes!("../../../rust-tests/parity/oracle/llm-failures-next/main.go")
        }
        _ => panic!("unexpected Go source {path}"),
    }
}

fn assert_oracle_fixture_matches(fixture_json: &str, args: &[&str]) {
    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock after Unix epoch")
        .as_nanos();
    let index = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let output_path = std::env::temp_dir().join(format!(
        "symeraseme-llm-failure-oracle-{}-{nonce}-{index}.json",
        std::process::id(),
    ));
    let output = Command::new("go")
        .args(["run", "./rust-tests/parity/oracle/llm-failures-next"])
        .args(args)
        .current_dir(ROOT)
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .env("LLM_FAILURES_NEXT_FIXTURE", &output_path)
        .output()
        .expect("run source-pinned Go LLM failure oracle");
    assert!(
        output.status.success(),
        "Go LLM failure oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let generated = std::fs::read(&output_path).expect("read regenerated Go fixture");
    let _ = std::fs::remove_file(output_path);
    assert_eq!(
        generated,
        fixture_json.as_bytes(),
        "Go 1.26.6 failure oracle, source hashes or fixture drifted"
    );
}

fn local_failure_server(
    attempts: usize,
    status: u16,
    body: &str,
) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback provider server");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let body = body.to_owned();
    let server = thread::spawn(move || {
        let mut paths = Vec::with_capacity(attempts);
        let deadline = Instant::now() + Duration::from_secs(15);
        while paths.len() < attempts {
            let (stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("accept provider request: {error}"),
            };
            let (mut stream, path) = read_request(stream);
            paths.push(path);
            let response = format!(
                "HTTP/1.1 {status} Synthetic Failure\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            );
            stream
                .write_all(response.as_bytes())
                .expect("write local 403 response");
        }
        paths
    });
    (base_url, server)
}

fn read_request(stream: TcpStream) -> (TcpStream, String) {
    stream
        .set_nonblocking(false)
        .expect("set accepted provider stream blocking");
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read request line");
    let path = line
        .split_whitespace()
        .nth(1)
        .expect("request path")
        .to_owned();
    let mut content_length = 0usize;
    loop {
        line.clear();
        reader.read_line(&mut line).expect("read request headers");
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().expect("content length");
        }
    }
    let mut request_body = vec![0; content_length];
    reader
        .read_exact(&mut request_body)
        .expect("read request body");
    (reader.into_inner(), path)
}
