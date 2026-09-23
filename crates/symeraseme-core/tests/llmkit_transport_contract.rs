//! Replays actual Go llmkit requests against local-only fake HTTP providers.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use serde_json::Value;
use sha2::{Digest, Sha256};
use symeraseme_core::llm::{ClassifyOptions, CreateOptions, create_with};

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
