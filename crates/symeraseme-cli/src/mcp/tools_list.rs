//! Transport-independent MCP `tools/list` contract.
//!
//! This slice deliberately stops at one request and response.  MCP-001a's
//! transport parser can call [`tools_list`] after rebasing this file into the
//! shared `mcp` module.

use serde::Serialize;
use serde_json::Value;

const TOOL_CATALOGUE: &[u8] = include_bytes!("../../../../internal/mcp/tools.json");

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ToolsListOutcome {
    Response(Vec<u8>),
    Notification,
    ParseError,
}

#[derive(Serialize)]
struct Response<'a> {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<ToolsResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
    id: &'a Value,
}

#[derive(Serialize)]
struct ToolsResult {
    tools: Vec<Value>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: &'static str,
}

/// Handle one JSON-RPC `tools/list` request without transport concerns.
pub(crate) fn tools_list(raw: &[u8]) -> ToolsListOutcome {
    let Ok(request) = serde_json::from_slice::<Value>(raw) else {
        return ToolsListOutcome::ParseError;
    };
    let Some(request) = request.as_object() else {
        return ToolsListOutcome::ParseError;
    };

    let Some(jsonrpc) = request.get("jsonrpc").and_then(Value::as_str) else {
        return invalid_request(request.get("id"));
    };
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return invalid_request(request.get("id"));
    };
    if jsonrpc != "2.0" || method.is_empty() {
        return invalid_request(request.get("id"));
    }
    if method != "tools/list" && method != "list_tools" {
        return invalid_request(request.get("id"));
    }

    let notification = !request.contains_key("id");
    if notification {
        return ToolsListOutcome::Notification;
    }

    let id = request.get("id").expect("contains_key checked");
    if !matches!(id, Value::Null | Value::String(_) | Value::Number(_)) {
        return invalid_request(Some(id));
    }
    if let Some(params) = request.get("params")
        && !params.is_null()
        && !params.is_object()
    {
        return response_error(id, -32602, "invalid params");
    }

    let catalogue = serde_json::from_slice::<Catalogue>(TOOL_CATALOGUE)
        .expect("Go MCP catalogue is valid JSON");
    response_tools(id, catalogue.tools)
}

#[derive(serde::Deserialize)]
struct Catalogue {
    tools: Vec<Value>,
}

fn invalid_request(id: Option<&Value>) -> ToolsListOutcome {
    let id = id.unwrap_or(&Value::Null);
    response_error(id, -32600, "invalid request")
}

fn response_tools(id: &Value, tools: Vec<Value>) -> ToolsListOutcome {
    ToolsListOutcome::Response(encode_response(&Response {
        jsonrpc: "2.0",
        result: Some(ToolsResult { tools }),
        error: None,
        id,
    }))
}

fn response_error(id: &Value, code: i32, message: &'static str) -> ToolsListOutcome {
    ToolsListOutcome::Response(encode_response(&Response {
        jsonrpc: "2.0",
        result: None,
        error: Some(RpcError { code, message }),
        id,
    }))
}

fn encode_response(response: &Response<'_>) -> Vec<u8> {
    let mut output = serde_json::to_vec(response).expect("MCP response is serializable");
    output.push(b'\n');
    go_escape_json_strings(&output)
}

// Go's encoding/json HTML-escapes these bytes even though serde_json does not.
fn go_escape_json_strings(input: &[u8]) -> Vec<u8> {
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

    fn response_bytes(raw: &str) -> Vec<u8> {
        match tools_list(raw.as_bytes()) {
            ToolsListOutcome::Response(bytes) => bytes,
            other => panic!("expected response, got {other:?}"),
        }
    }

    fn expected_error(id: &str, code: i32, message: &str) -> Vec<u8> {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"error\":{{\"code\":{code},\"message\":\"{message}\"}},\"id\":{id}}}\n"
        )
        .into_bytes()
    }

    #[test]
    fn non_json_and_non_object_payloads_are_parse_errors() {
        assert_eq!(tools_list(b"not json"), ToolsListOutcome::ParseError);
        assert_eq!(tools_list(b"[1,2]"), ToolsListOutcome::ParseError);
    }

    #[test]
    fn invalid_envelopes_are_invalid_requests_carrying_the_supplied_id() {
        for (name, request, id) in [
            ("missing jsonrpc", r#"{"id":1,"method":"tools/list"}"#, "1"),
            ("missing method", r#"{"jsonrpc":"2.0","id":1}"#, "1"),
            (
                "empty method",
                r#"{"jsonrpc":"2.0","id":1,"method":""}"#,
                "1",
            ),
            (
                "unknown method",
                r#"{"jsonrpc":"2.0","id":1,"method":"other"}"#,
                "1",
            ),
            (
                "wrong protocol",
                r#"{"jsonrpc":"1.0","id":1,"method":"tools/list"}"#,
                "1",
            ),
            (
                "non-scalar id",
                r#"{"jsonrpc":"2.0","id":true,"method":"tools/list"}"#,
                "true",
            ),
            ("absent id", r#"{"jsonrpc":"2.0","method":"other"}"#, "null"),
        ] {
            assert_eq!(
                response_bytes(request),
                expected_error(id, -32600, "invalid request"),
                "{name}"
            );
        }
    }

    #[test]
    fn non_object_params_are_invalid_params() {
        assert_eq!(
            response_bytes(r#"{"jsonrpc":"2.0","id":6,"method":"tools/list","params":[]}"#),
            expected_error("6", -32602, "invalid params")
        );
    }

    #[test]
    fn notifications_are_silent_and_the_list_tools_alias_is_accepted() {
        assert_eq!(
            tools_list(br#"{"jsonrpc":"2.0","method":"tools/list","params":{}}"#),
            ToolsListOutcome::Notification
        );
        let alias: serde_json::Value = serde_json::from_slice(&response_bytes(
            r#"{"jsonrpc":"2.0","id":7,"method":"list_tools"}"#,
        ))
        .expect("alias response is JSON");
        assert_eq!(alias["id"], 7);
        assert_eq!(
            alias["result"]["tools"].as_array().expect("tools").len(),
            26
        );
    }

    #[test]
    fn go_html_escaping_applies_to_string_ids() {
        let response = response_bytes(
            "{\"jsonrpc\":\"2.0\",\"id\":\"<>&\\u2028\\u2029\",\"method\":\"tools/list\"}",
        );
        let text = String::from_utf8(response).expect("response is UTF-8");
        assert!(text.contains("\\u003c"), "expected '<' escaped: {text}");
        assert!(text.contains("\\u003e"), "expected '>' escaped: {text}");
        assert!(text.contains("\\u0026"), "expected '&' escaped: {text}");
        assert!(
            text.contains("\\u2028") && text.contains("\\u2029"),
            "expected U+2028/U+2029 escaped: {text}"
        );
        let document: serde_json::Value =
            serde_json::from_str(text.trim_end()).expect("response JSON");
        assert_eq!(document["id"], "<>&\u{2028}\u{2029}");
    }
}
