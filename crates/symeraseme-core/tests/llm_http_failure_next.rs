use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

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
#[path = "support/frozen_native_capture.rs"]
mod frozen_native_capture;
#[path = "support/go_oracle.rs"]
mod go_oracle;

#[test]
fn llm_failure_fixtures_regenerate_from_pinned_go_oracle() {
    let live = if live_go_required() {
        Some(build_live_oracle())
    } else {
        None
    };
    let binary = live.as_ref().map(|root| {
        root.path()
            .join(format!("llm-failure{}", std::env::consts::EXE_SUFFIX))
    });
    let executable = binary.as_deref();
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
            scope.spawn(move || assert_oracle_fixture_matches(fixture, args, executable))
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
    let started = Instant::now();
    let pending =
        PendingFailureServer::new(attempts, expected_status, fixture["body"].as_str().unwrap());
    let base_url = pending.base_url();
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
    let setup_elapsed = started.elapsed();
    // Client setup is not an HTTP attempt. Arm the unchanged 15-second active
    // fixture lifetime only after its actual client is ready to send.
    let server = pending.start(Duration::from_secs(15));
    let active_started = Instant::now();
    let result = client.classify("system", "user", &ClassifyOptions::default());
    writeln!(
        std::io::stderr(),
        "LLM_FAILURE_NEXT status={expected_status} outcome_provider={} elapsed_ms={} setup_ms={} active_ms={}",
        matches!(&result, Err(ClientError::Provider(_))),
        started.elapsed().as_millis(),
        setup_elapsed.as_millis(),
        active_started.elapsed().as_millis()
    )
    .expect("write local provider outcome diagnostics");
    let paths = server
        .join()
        .expect("local provider server thread")
        .expect("complete expected provider attempts");

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

fn live_go_required() -> bool {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => {
            frozen_native_capture::manifest_bytes(std::env::consts::OS, std::env::consts::ARCH)
                .is_none()
        }
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
}

fn build_live_oracle() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let executable = root
        .path()
        .join(format!("llm-failure{}", std::env::consts::EXE_SUFFIX));
    let mut command = Command::new("go");
    command
        .args(["build", "-mod=readonly", "-buildvcs=true", "-o"])
        .arg(executable)
        .arg("./rust-tests/parity/oracle/llm-failures-next")
        .current_dir(ROOT)
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off");
    let output = go_oracle::run_bounded(
        command,
        None,
        &root.path().join("build.stdout"),
        &root.path().join("build.stderr"),
        go_oracle::ORACLE_BUILD_TIMEOUT,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "Go LLM build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    root
}

fn assert_oracle_fixture_matches(
    fixture_json: &str,
    args: &[&str],
    executable: Option<&std::path::Path>,
) {
    let Some(executable) = executable else {
        verify_frozen_llm_fixture(fixture_json.as_bytes(), args);
        return;
    };
    static NEXT_INVOCATION: AtomicU64 = AtomicU64::new(0);
    let index = NEXT_INVOCATION.fetch_add(1, Ordering::Relaxed);
    let root = executable.parent().unwrap();
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(ROOT)
        .env_remove("LLM_FAILURES_NEXT_FIXTURE");
    let output = go_oracle::run_bounded(
        command,
        None,
        &root.join(format!("run-{index}.stdout")),
        &root.join(format!("run-{index}.stderr")),
        go_oracle::ORACLE_TIMEOUT,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "Go LLM fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        fixture_json.as_bytes(),
        "complete pinned Go fixture drifted"
    );
}

fn verify_frozen_llm_fixture(fixture: &[u8], args: &[&str]) {
    if let Some(bytes) =
        frozen_native_capture::manifest_bytes(std::env::consts::OS, std::env::consts::ARCH)
    {
        let manifest =
            frozen_native_capture::verify(bytes, std::env::consts::OS, std::env::consts::ARCH);
        verify_native_llm(&manifest, fixture, args);
        return;
    }
    let manifest: Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/go-frozen/llm-failures-next/manifest.json"
    ))
    .unwrap();
    assert_eq!(
        manifest["source_revision"],
        "54acd9bdb1bb4df1f7c8b323d5cec93da0f7ca54"
    );
    assert_eq!(manifest["go_version"], "go version go1.26.6 linux/amd64");
    assert_eq!(manifest["native_target"], "linux/amd64");
    let cases = manifest["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 7);
    let capture = cases
        .iter()
        .find(|case| case["args"] == serde_json::to_value(args).unwrap())
        .expect("unobserved LLM fixture arguments cannot receive a cached answer");
    assert_eq!(capture["exit_status"], 0);
    assert_eq!(capture["stdout"]["bytes"], fixture.len());
    assert_eq!(
        capture["stdout"]["sha256"],
        hex::encode(Sha256::digest(fixture))
    );
    assert_eq!(capture["stderr"]["bytes"], 0);
    assert_eq!(capture["stderr"]["sha256"], hex::encode(Sha256::digest([])));
    for (path, metadata) in manifest["source_files"].as_object().unwrap() {
        let source = go_source(path);
        assert_eq!(metadata["bytes"], source.len());
        assert_eq!(metadata["sha256"], hex::encode(Sha256::digest(source)));
    }
}

#[test]
fn frozen_llm_failure_rejects_changed_message_and_unobserved_arguments() {
    let mut changed: Value = serde_json::from_str(FIXTURE).unwrap();
    changed["message"] = "fabricated provider message".into();
    assert!(
        std::panic::catch_unwind(|| verify_frozen_llm_fixture(
            &serde_json::to_vec(&changed).unwrap(),
            &[]
        ))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| verify_frozen_llm_fixture(
            FIXTURE.as_bytes(),
            &["--status", "418"]
        ))
        .is_err()
    );
}

#[test]
fn native_frozen_llm_records_reject_wrong_target_and_source_inventory() {
    for (os, arch) in [
        ("linux", "x86_64"),
        ("linux", "aarch64"),
        ("windows", "x86_64"),
        ("windows", "aarch64"),
        ("macos", "x86_64"),
        ("macos", "aarch64"),
    ] {
        let bytes = frozen_native_capture::manifest_bytes(os, arch).unwrap();
        let manifest = frozen_native_capture::verify(bytes, os, arch);
        verify_native_llm(&manifest, FIXTURE.as_bytes(), &[]);
        let mut wrong = manifest.clone();
        wrong["native_target"] = "fabricated/host".into();
        assert!(
            std::panic::catch_unwind(|| frozen_native_capture::verify(
                &serde_json::to_vec(&wrong).unwrap(),
                os,
                arch
            ))
            .is_err()
        );
        let mut wrong = manifest;
        wrong["source_files"]
            .as_object_mut()
            .unwrap()
            .remove("go.mod");
        assert!(
            std::panic::catch_unwind(|| frozen_native_capture::verify(
                &serde_json::to_vec(&wrong).unwrap(),
                os,
                arch
            ))
            .is_err()
        );
    }
    assert!(frozen_native_capture::manifest_bytes("linux", "unrecorded").is_none());
}

#[derive(Debug)]
struct ServerFailure {
    kind: std::io::ErrorKind,
    received: usize,
}

struct PendingFailureServer {
    listener: TcpListener,
    attempts: usize,
    status: u16,
    body: String,
}

impl PendingFailureServer {
    fn new(attempts: usize, status: u16, body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback provider server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        Self {
            listener,
            attempts,
            status,
            body: body.to_owned(),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.listener.local_addr().unwrap())
    }

    fn start(self, budget: Duration) -> thread::JoinHandle<Result<Vec<String>, ServerFailure>> {
        self.start_until(Instant::now() + budget, budget)
    }

    fn start_until(
        self,
        deadline: Instant,
        budget: Duration,
    ) -> thread::JoinHandle<Result<Vec<String>, ServerFailure>> {
        thread::spawn(move || {
            let started = Instant::now();
            let mut paths = Vec::with_capacity(self.attempts);
            while paths.len() < self.attempts {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    eprintln!(
                        "LLM_FAILURE_NEXT status={} phase=accept_deadline received={}/{} elapsed_ms={} accept_limit_ms={}",
                        self.status,
                        paths.len(),
                        self.attempts,
                        started.elapsed().as_millis(),
                        budget.as_millis()
                    );
                    return Err(ServerFailure {
                        kind: std::io::ErrorKind::TimedOut,
                        received: paths.len(),
                    });
                }
                let (stream, _) = match self.listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10).min(remaining));
                        continue;
                    }
                    Err(error) => {
                        return Err(ServerFailure {
                            kind: error.kind(),
                            received: paths.len(),
                        });
                    }
                };
                stream
                    .set_nonblocking(false)
                    .expect("blocking accepted provider stream");
                stream
                    .set_read_timeout(Some(remaining))
                    .expect("bounded provider request read");
                stream
                    .set_write_timeout(Some(remaining))
                    .expect("bounded provider response write");
                let (mut stream, path) = read_request(stream);
                paths.push(path);
                eprintln!(
                    "LLM_FAILURE_NEXT status={} received={}/{} elapsed_ms={} accept_limit_ms={}",
                    self.status,
                    paths.len(),
                    self.attempts,
                    started.elapsed().as_millis(),
                    budget.as_millis()
                );
                let response = format!(
                    "HTTP/1.1 {} Synthetic Failure\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    self.status,
                    self.body.len(),
                    self.body
                );
                if let Err(error) = stream.write_all(response.as_bytes()) {
                    return Err(ServerFailure {
                        kind: error.kind(),
                        received: paths.len(),
                    });
                }
            }
            Ok(paths)
        })
    }
}

#[test]
fn client_preparation_must_not_consume_active_fixture_deadline() {
    let budget = Duration::from_millis(500);
    let old = PendingFailureServer::new(1, 403, "synthetic");
    let old_deadline = Instant::now() + budget;
    let prepared = PendingFailureServer::new(1, 403, "synthetic");
    // A bounded setup delay distinguishes the old clock origin from arming
    // the same budget at readiness. It changes no production retry policy.
    thread::sleep(budget * 2);
    let failure = old
        .start_until(old_deadline, budget)
        .join()
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.kind, std::io::ErrorKind::TimedOut);
    assert_eq!(failure.received, 0);
    let address = prepared.listener.local_addr().unwrap();
    let success = prepared.start(budget);
    let mut client = TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    client
        .write_all(b"POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 403 Synthetic Failure\r\n"));
    assert_eq!(success.join().unwrap().unwrap(), ["/v1/chat/completions"]);
}

#[test]
fn missing_retry_is_a_bounded_failure_with_exact_request_count() {
    let pending = PendingFailureServer::new(3, 400, "synthetic");
    let address = pending.listener.local_addr().unwrap();
    let started = Instant::now();
    let server = pending.start(Duration::from_millis(300));
    let mut client = TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    client
        .write_all(b"POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 400 Synthetic Failure\r\n"));
    let failure = server.join().unwrap().unwrap_err();
    assert_eq!(failure.kind, std::io::ErrorKind::TimedOut);
    assert_eq!(failure.received, 1);
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "missing retry exceeded the bounded control"
    );
}

fn read_request(stream: TcpStream) -> (TcpStream, String) {
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

fn verify_native_llm(manifest: &Value, fixture: &[u8], args: &[&str]) {
    let case = manifest["llm_failures"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["args"] == serde_json::to_value(args).unwrap())
        .expect("unobserved native LLM arguments cannot receive a cached answer");
    assert_eq!(case["exit_status"], 0);
    assert_eq!(case["stdout"]["bytes"], fixture.len());
    assert_eq!(
        case["stdout"]["sha256"],
        hex::encode(Sha256::digest(fixture))
    );
    assert_eq!(case["stderr"]["bytes"], 0);
    assert_eq!(case["stderr"]["sha256"], hex::encode(Sha256::digest([])));
}
