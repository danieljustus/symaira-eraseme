//! MCP response building: the tool result envelope and handler-error
//! sanitization.
//!
//! Ports `contentEnvelope`, `sanitizeError` and the success/error response
//! shapes from `internal/mcp/server.go`. Byte order matters: Go serializes maps
//! with sorted keys, which `serde_json`'s default map also does.

use serde::Serialize;
use serde_json::Value;

/// Storage detail that must never reach a client.
const STORAGE_MARKERS: [&str; 4] = [
    "sqlite",
    "no such table",
    "database is locked",
    "database disk image is malformed",
];
const STORAGE_MESSAGE: &str = "Database not ready — start the server again";
const PANIC_MESSAGE: &str = "Internal server error";

#[derive(Serialize)]
struct SuccessResponse<'a> {
    jsonrpc: &'static str,
    result: Value,
    id: &'a Value,
}

#[derive(Serialize)]
struct ErrorResponse<'a> {
    jsonrpc: &'static str,
    error: RpcError<'a>,
    id: &'a Value,
}

#[derive(Serialize)]
struct RpcError<'a> {
    code: i32,
    message: &'a str,
}

#[derive(Serialize)]
struct ContentEnvelope {
    content: Vec<ContentItem>,
}

#[derive(Serialize)]
struct ContentItem {
    text: String,
    #[serde(rename = "type")]
    kind: &'static str,
}

/// Wraps a tool result exactly like Go's `contentEnvelope`: a string result is
/// used verbatim, anything else becomes compact JSON, and an absent result
/// becomes `null`.
pub(crate) fn content_envelope(result: Option<&Value>) -> Value {
    let text = match result {
        Some(Value::String(value)) => value.clone(),
        Some(value) => {
            // Go builds this text with `json.Marshal`, which HTML-escapes, and
            // the outer encoder then escapes the resulting backslashes — so a
            // payload's `&` reaches the wire as `\\u0026`, not `\u0026`.
            let serialized = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
            String::from_utf8(go_escape_json_strings(serialized.as_bytes()))
                .expect("escaped JSON is UTF-8")
        }
        None => "null".to_owned(),
    };
    serde_json::to_value(ContentEnvelope {
        content: vec![ContentItem { text, kind: "text" }],
    })
    .expect("content envelope is serializable")
}

/// Hides storage detail exactly like Go's `sanitizeError`.
pub(crate) fn sanitize_error(message: &str) -> String {
    let lower = message.to_lowercase();
    if STORAGE_MARKERS.iter().any(|marker| lower.contains(marker)) {
        return STORAGE_MESSAGE.to_owned();
    }
    if lower.contains("panic") {
        return PANIC_MESSAGE.to_owned();
    }
    message.to_owned()
}

/// A JSON-RPC success response enclosing a tool result.
pub(crate) fn result_response(id: &Value, result: Option<&Value>) -> Vec<u8> {
    encode(&SuccessResponse {
        jsonrpc: "2.0",
        result: content_envelope(result),
        id,
    })
}

/// A JSON-RPC error response. Handler failures must be passed through
/// [`sanitize_error`] first.
pub(crate) fn error_response(id: &Value, code: i32, message: &str) -> Vec<u8> {
    encode(&ErrorResponse {
        jsonrpc: "2.0",
        error: RpcError { code, message },
        id,
    })
}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut output = serde_json::to_vec(value).expect("MCP response is serializable");
    output.push(b'\n');
    go_escape_json_strings(&output)
}

/// Go's `encoding/json` HTTP-escapes `<`, `>`, `&`, U+2028 and U+2029 inside
/// string values by default, while serde_json does not. Every response this
/// crate emits goes through this so the bytes match Go's encoder.
pub(crate) fn go_escape_json_strings(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while index < input.len() {
        let byte = input[index];
        if !in_string {
            output.push(byte);
            in_string = byte == b'"';
            index += 1;
            continue;
        }
        if escaped {
            output.push(byte);
            escaped = false;
            index += 1;
            continue;
        }
        match byte {
            b'\\' => {
                output.push(byte);
                escaped = true;
                index += 1;
            }
            b'"' => {
                output.push(byte);
                in_string = false;
                index += 1;
            }
            b'<' => {
                output.extend_from_slice(br#"\u003c"#);
                index += 1;
            }
            b'>' => {
                output.extend_from_slice(br#"\u003e"#);
                index += 1;
            }
            b'&' => {
                output.extend_from_slice(br#"\u0026"#);
                index += 1;
            }
            _ if input[index..].starts_with("\u{2028}".as_bytes()) => {
                output.extend_from_slice(br#"\u2028"#);
                index += 3;
            }
            _ if input[index..].starts_with("\u{2029}".as_bytes()) => {
                output.extend_from_slice(br#"\u2029"#);
                index += 3;
            }
            _ => {
                output.push(byte);
                index += 1;
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Deserialize)]
    struct Case {
        name: String,
        handler: String,
        response: Option<String>,
    }

    #[derive(Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    /// Rebuilds the response for one measured handler kind. The fixture stores
    /// the handler that produced it, so every case is reproducible here.
    fn rebuild(handler: &str) -> Vec<u8> {
        let id = json!(7);
        match handler {
            "string" => result_response(&id, Some(&json!("redacted text"))),
            "object" => result_response(&id, Some(&json!({ "b": [2], "a": 1 }))),
            "array" => result_response(&id, Some(&json!([1, "two"]))),
            "number" => result_response(&id, Some(&json!(42))),
            "nil-handler" => result_response(&id, None),
            "error-sqlite" => {
                error_response(&id, -32603, &sanitize_error("SQLite error: disk I/O error"))
            }
            "error-no-such-table" => error_response(
                &id,
                -32603,
                &sanitize_error("no such table: removal_requests"),
            ),
            "error-locked" => error_response(&id, -32603, &sanitize_error("database is locked")),
            "error-malformed" => error_response(
                &id,
                -32603,
                &sanitize_error("database disk image is malformed"),
            ),
            "error-panic-marker" => error_response(
                &id,
                -32603,
                &sanitize_error("panic: runtime error: index out of range"),
            ),
            "error-plain" => error_response(&id, -32603, &sanitize_error("campaign not found")),
            "default-nil" => error_response(
                &id,
                -32603,
                &sanitize_error("tool backend is not available"),
            ),
            other => panic!("fixture uses an unknown handler kind: {other}"),
        }
    }

    #[test]
    fn source_bound_go_result_fixture_matches() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/mcp-contract/mcp-result/cases.json"
        ))
        .expect("result fixture");
        assert_eq!(fixture.cases.len(), 12, "fixture case count changed");
        for case in fixture.cases {
            assert_eq!(
                String::from_utf8(rebuild(&case.handler)).unwrap(),
                case.response.expect("fixture response"),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn sanitize_error_matches_the_go_marker_rules() {
        for message in [
            "SQLITE_ERROR",
            "no such table: x",
            "Database is locked",
            "database disk image is malformed",
        ] {
            assert_eq!(
                sanitize_error(message),
                "Database not ready — start the server again",
                "{message}"
            );
        }
        assert_eq!(sanitize_error("PANIC: boom"), "Internal server error");
        assert_eq!(sanitize_error("campaign not found"), "campaign not found");
    }
}
