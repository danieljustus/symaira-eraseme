//! Replays actual Go llmkit requests against local-only fake HTTP providers.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};
use symeraseme_core::llm::{create_with, ClassifyOptions, ClientError, CreateOptions};

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llmkit-transport/cases.json"
));

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("Go transport fixture parses")
}

fn source_bytes(path: &str) -> &'static [u8] {
    match path {
        "internal/llm/factory.go" => include_bytes!("../../../internal/llm/factory.go"),
        "internal/llm/llmkit.go" => include_bytes!("../../../internal/llm/llmkit.go"),
        "internal/llm/llm.go" => include_bytes!("../../../internal/llm/llm.go"),
        "go.mod" => include_bytes!("../../../go.mod"),
        "go.sum" => include_bytes!("../../../go.sum"),
        _ => panic!("unexpected source path {path}"),
    }
}

#[test]
fn go_source_is_pinned_to_the_fixture() {
    let fixture = fixture();
    assert_eq!(
        fixture["schema"],
        "symeraseme.go-oracle.llmkit-transport.v1"
    );
    assert_eq!(
        fixture["go_module"],
        "github.com/danieljustus/symaira-corekit v0.16.2"
    );
    for (path, expected) in fixture["sources_sha256"].as_object().unwrap() {
        let digest = Sha256::digest(source_bytes(path));
        assert_eq!(hex::encode(digest), expected.as_str().unwrap(), "{path}");
    }
}

#[test]
fn constructor_errors_match_the_go_oracle_and_validation_order() {
    let fixture = fixture();
    let cases = fixture["construction_errors"]
        .as_array()
        .expect("constructor error cases");
    assert_eq!(cases.len(), 3);
    for case in cases {
        let options = CreateOptions {
            provider: case["provider"].as_str().unwrap().to_owned(),
            base_url: case["base_url"].as_str().unwrap().to_owned(),
            api_key: case["api_key"].as_str().unwrap().to_owned(),
            ..CreateOptions::default()
        };
        let error =
            create_with(&options, &|_| None, &|_| false).expect_err(case["id"].as_str().unwrap());
        assert_eq!(error.to_string(), case["message"], "{}", case["id"]);
    }
}

#[test]
fn echoed_api_keys_are_redacted_from_provider_errors_and_debug() {
    let api_key = "synthetic-secret-that-must-not-leak";
    let options = CreateOptions {
        api_key: api_key.to_owned(),
        ..CreateOptions::default()
    };
    let debug = format!("{options:?}");
    assert!(!debug.contains(api_key));
    assert!(debug.contains("has_api_key: true"));

    let (base_url, server) = local_error_server(api_key);
    let mut client = create_with(
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
    client.base.max_retries = 1;
    let error = client
        .classify("system", "user", &ClassifyOptions::default())
        .expect_err("fake provider rejects the request");
    let message = error.to_string();
    assert!(!message.contains(api_key));
    assert!(message.contains("[REDACTED]"));
    server.join().expect("fake server thread");
}

#[test]
fn local_transport_matches_go_factory_and_llmkit_cases() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("recorded cases");
    assert_eq!(cases.len(), 5);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let (base_url, server) = local_server(case["provider"].as_str().unwrap(), id);
        let mut env = HashMap::new();
        env.insert("SYMERASEME_LLM_BASE_URL".to_owned(), base_url.clone());
        match id {
            "openai-env-default-model" => {
                env.insert(
                    "OPENAI_API_KEY".to_owned(),
                    "synthetic-openai-key".to_owned(),
                );
            }
            "openai-direct-key-wins" => {
                env.insert(
                    "OPENAI_API_KEY".to_owned(),
                    "ignored-environment-key".to_owned(),
                );
            }
            "anthropic-env-explicit-model" => {
                env.insert(
                    "ANTHROPIC_API_KEY".to_owned(),
                    "synthetic-anthropic-key".to_owned(),
                );
            }
            "ollama-host-no-auth" => {
                env.remove("SYMERASEME_LLM_BASE_URL");
                env.insert("OLLAMA_HOST".to_owned(), base_url.clone());
            }
            "custom-direct-key" => {}
            _ => panic!("unrecognized Go case {id}"),
        }
        let env_lookup = |name: &str| env.get(name).cloned();
        let options = CreateOptions {
            provider: case["provider"].as_str().unwrap().to_owned(),
            model: case["model"].as_str().unwrap().to_owned(),
            api_key: match id {
                "custom-direct-key" => "synthetic-custom-key",
                "openai-direct-key-wins" => "synthetic-direct-openai-key",
                _ => "",
            }
            .to_owned(),
            base_url: if id == "custom-direct-key" {
                base_url
            } else {
                String::new()
            },
            ..CreateOptions::default()
        };
        let client = create_with(&options, &env_lookup, &|_| false)
            .unwrap_or_else(|error| panic!("{id}: create failed: {error}"));
        assert!(client.is_available(), "{id}");
        let (text, usage) = client
            .classify(
                "system",
                "user",
                &ClassifyOptions {
                    max_tokens: 64,
                    temperature: 0.25,
                    cache_key: String::new(),
                },
            )
            .unwrap_or_else(|error| panic!("{id}: classify failed: {error}"));
        let (path, headers, request) = server.join().expect("fake server thread");
        assert_eq!(path, case["path"], "{id} path");
        assert_eq!(headers, case["headers"], "{id} auth/version headers");
        assert_eq!(request, case["request"], "{id} request");
        assert_eq!(text, case["text"], "{id} response text");
        assert_eq!(usage.model, case["usage_model"], "{id} usage model");
        assert_eq!(usage.input_tokens, 0, "Go llmkit has no non-stream usage");
    }
}

#[test]
fn transport_failures_match_go_retry_counts_and_error_classes() {
    let fixture = fixture();
    let cases = fixture["failure_cases"]
        .as_array()
        .expect("Go failure cases");
    assert_eq!(cases.len(), 3);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let attempts = case["attempts"].as_u64().unwrap() as usize;
        let (base_url, server) = local_failure_server(id, attempts);
        let client = create_with(
            &CreateOptions {
                provider: "openai".to_owned(),
                base_url,
                api_key: "synthetic-failure-key".to_owned(),
                ..CreateOptions::default()
            },
            &|_| None,
            &|_| false,
        )
        .unwrap_or_else(|error| panic!("{id}: create failed: {error}"));
        let result = client.classify(
            "system",
            "user",
            &ClassifyOptions {
                max_tokens: 64,
                temperature: 0.25,
                cache_key: String::new(),
            },
        );
        let paths = server.join().expect("local failure server thread");
        assert_eq!(paths.len(), attempts, "{id} attempts");
        assert!(
            paths.iter().all(|path| path == "/chat/completions"),
            "{id} path"
        );

        match case["error_kind"].as_str().unwrap() {
            "none" => {
                let (text, _) = result.unwrap_or_else(|error| panic!("{id}: {error}"));
                assert_eq!(text, case["text"], "{id} text");
            }
            "rate_limit" => {
                let error = result.expect_err(id);
                assert!(matches!(error, ClientError::RateLimit(_)), "{id}: {error}");
                assert_eq!(error.to_string(), case["message"], "{id} message");
            }
            "provider" => {
                let error = result.expect_err(id);
                assert!(matches!(error, ClientError::Provider(_)), "{id}: {error}");
                if id == "openai-empty-choices-response" {
                    assert_eq!(error.to_string(), case["message"], "{id} message");
                } else {
                    assert!(
                        error
                            .to_string()
                            .contains("llmkit: provider_error: decode chat response"),
                        "{id}: {error}"
                    );
                }
            }
            other => panic!("{id}: unexpected Go error kind {other}"),
        }
    }
}

fn local_failure_server(id: &str, attempts: usize) -> (String, thread::JoinHandle<Vec<String>>) {
    let (status, body) = match id {
        "openai-rate-limit-exhausted" => (
            429,
            r#"{"error":{"message":"try later","type":"rate_limit_error"}}"#,
        ),
        "openai-invalid-json-response" => (200, "{not json"),
        "openai-empty-choices-response" => (200, r#"{"choices":[]}"#),
        _ => panic!("unrecognized Go failure case {id}"),
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback failure server");
    listener
        .set_nonblocking(true)
        .expect("set failure server nonblocking");
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut paths = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(15);
        while paths.len() < attempts {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error)
                    if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("accept provider request: {error}"),
            };
            stream
                .set_nonblocking(false)
                .expect("set failure stream blocking");
            let (path, _, _) = read_http_request(&mut stream);
            paths.push(path);
            let reason = if status == 429 {
                "Too Many Requests"
            } else {
                "OK"
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write local failure response");
        }
        paths
    });
    (base_url, server)
}

fn local_server(provider: &str, id: &str) -> (String, thread::JoinHandle<(String, Value, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback fake provider");
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}");
    let provider = provider.to_owned();
    let id = id.to_owned();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept provider request");
        let (path, headers, body) = read_http_request(&mut stream);
        let response_body = if provider == "anthropic" {
            format!(r#"{{"content":[{{"type":"text","text":" hello {id} "}}]}}"#)
        } else {
            format!(r#"{{"choices":[{{"message":{{"content":" hello {id} "}}}}]}}"#)
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write local response");
        (
            path,
            headers,
            serde_json::from_slice(&body).expect("JSON request"),
        )
    });
    (base_url, server)
}

fn local_error_server(api_key: &str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback fake provider");
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}");
    let api_key = api_key.to_owned();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept provider request");
        let _ = read_http_request(&mut stream);
        let response_body = format!("provider echoed credential: {api_key}");
        let response = format!(
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write local error response");
    });
    (base_url, server)
}

fn read_http_request(stream: &mut TcpStream) -> (String, Value, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).expect("read request headers");
        assert_ne!(count, 0, "request ended before headers");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let raw_headers = String::from_utf8_lossy(&bytes[..header_end]);
    let mut lines = raw_headers.split("\r\n");
    let request_line = lines.next().unwrap();
    let path = request_line.split_whitespace().nth(1).unwrap().to_owned();
    let mut headers = serde_json::Map::new();
    let mut content_length = 0usize;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').expect("header line");
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        if name == "content-length" {
            content_length = value.parse().expect("content length");
        }
        if matches!(
            name.as_str(),
            "authorization" | "x-api-key" | "anthropic-version"
        ) {
            headers.insert(name, Value::String(value));
        }
    }
    while bytes.len() - header_end < content_length {
        let count = stream.read(&mut chunk).expect("read request body");
        assert_ne!(count, 0, "request body truncated");
        bytes.extend_from_slice(&chunk[..count]);
    }
    for name in ["authorization", "x-api-key", "anthropic-version"] {
        headers
            .entry(name.to_owned())
            .or_insert(Value::String(String::new()));
    }
    (
        path,
        Value::Object(headers),
        bytes[header_end..header_end + content_length].to_vec(),
    )
}
