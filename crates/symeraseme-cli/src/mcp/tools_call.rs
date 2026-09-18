//! Transport-independent MCP `tools/call` request validation.
//!
//! Ports the validation half of the `tools/call` case in
//! `internal/mcp/server.go` (`handleRequest` + `validateToolArguments` +
//! `jsonValueMatches`). Tool *execution* is a later slice, so a request that
//! passes validation is answered with an explicit fail-closed error rather
//! than a fabricated result.

use serde::Serialize;
use serde_json::Value;

const TOOL_CATALOGUE: &[u8] = include_bytes!("../../../../internal/mcp/tools.json");

/// The legacy name Go accepts in `tools/call` without a catalogue entry.
const LEGACY_STATUS_ALIAS: &str = "status";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ToolsCallOutcome {
    Response(Vec<u8>),
    Notification,
    ParseError,
}

#[derive(Serialize)]
struct Response<'a> {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError<'a>>,
    id: &'a Value,
}

#[derive(Serialize)]
struct RpcError<'a> {
    code: i32,
    message: &'a str,
}

/// Handles one JSON-RPC `tools/call` request without transport concerns.
pub(crate) fn tools_call(raw: &[u8]) -> ToolsCallOutcome {
    let Ok(request) = serde_json::from_slice::<Value>(raw) else {
        return ToolsCallOutcome::ParseError;
    };
    let Some(request) = request.as_object() else {
        return ToolsCallOutcome::ParseError;
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
    if !request.contains_key("id") {
        return ToolsCallOutcome::Notification;
    }
    let id = request.get("id").expect("contains_key checked");
    if !matches!(id, Value::Null | Value::String(_) | Value::Number(_)) {
        return invalid_request(Some(id));
    }

    // Go's decodeParams treats an absent or null `params` as an empty object,
    // so only a present non-object value is rejected.
    let empty = serde_json::Map::new();
    let params = match request.get("params") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(object)) => object,
        Some(_) => return response_error(id, -32602, "invalid params"),
    };

    let name = params.get("name").and_then(Value::as_str);
    let Some(name) = name else {
        return response_error(id, -32602, "missing tool name");
    };
    if name.trim().is_empty() {
        return response_error(id, -32602, "missing tool name");
    }

    // Go's objectMap accepts an absent or null `arguments` as an empty object.
    let empty_args = serde_json::Map::new();
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => &empty_args,
        Some(Value::Object(object)) => object,
        Some(_) => return response_error(id, -32602, "invalid params"),
    };

    let catalogue =
        serde_json::from_slice::<Value>(TOOL_CATALOGUE).expect("Go MCP catalogue is valid JSON");
    let tools = catalogue
        .get("tools")
        .and_then(Value::as_array)
        .expect("Go MCP catalogue has a tools array");
    let tool = tools
        .iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name));
    let Some(tool) = tool else {
        if name != LEGACY_STATUS_ALIAS {
            return response_error(id, -32601, "method not found");
        }
        // ponytail: the legacy `status` alias validates like Go but has no
        // executor here; extend with the real handler when MCP-003 lands.
        return deferred_execution(id);
    };

    if let Err(message) = validate_arguments(tool, arguments) {
        return response_error(id, -32602, &message);
    }

    // ponytail: validation is complete, execution is not part of this slice.
    // Go would call the tool handler here and answer with a content envelope;
    // answering with a fabricated envelope would fake parity, so this slice
    // fails closed until MCP-003 provides the executor.
    deferred_execution(id)
}

/// Mirrors Go's `validateToolArguments`: required keys first, then the type of
/// every supplied argument.
fn validate_arguments(
    tool: &Value,
    arguments: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let schema = tool.get("inputSchema");
    if let Some(required) = schema
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
    {
        for key in required {
            let Ok(key) = serde_json::from_value::<String>(key.clone()) else {
                continue;
            };
            if !arguments.contains_key(&key) {
                return Err(format!("missing required argument: {key}"));
            }
        }
    }
    let properties = schema.and_then(|schema| schema.get("properties"));
    for (key, value) in arguments {
        let kind = properties
            .and_then(|properties| properties.get(key))
            .and_then(|property| property.get("type"))
            .and_then(Value::as_str);
        if let Some(kind) = kind
            && !json_value_matches(kind, value)
        {
            return Err(format!("invalid parameter type: {key}"));
        }
    }
    Ok(())
}

/// Mirrors Go's `jsonValueMatches`. An unknown kind matches everything.
fn json_value_matches(kind: &str, value: &Value) -> bool {
    match kind {
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value
            .as_f64()
            .is_some_and(|number| number == (number as i64) as f64),
        "array" => value.is_array(),
        _ => true,
    }
}

fn invalid_request(id: Option<&Value>) -> ToolsCallOutcome {
    response_error(id.unwrap_or(&Value::Null), -32600, "invalid request")
}

fn deferred_execution(id: &Value) -> ToolsCallOutcome {
    response_error(
        id,
        -32603,
        "tool execution is not implemented in this slice",
    )
}

fn response_error(id: &Value, code: i32, message: &str) -> ToolsCallOutcome {
    let response = Response {
        jsonrpc: "2.0",
        result: None,
        error: Some(RpcError { code, message }),
        id,
    };
    let mut output = serde_json::to_vec(&response).expect("MCP response is serializable");
    output.push(b'\n');
    ToolsCallOutcome::Response(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_text(raw: &str) -> String {
        match tools_call(raw.as_bytes()) {
            ToolsCallOutcome::Response(bytes) => {
                String::from_utf8(bytes).expect("response is UTF-8")
            }
            other => panic!("expected response, got {other:?}"),
        }
    }

    #[test]
    fn non_object_payloads_are_parse_errors() {
        assert_eq!(tools_call(b"not json"), ToolsCallOutcome::ParseError);
        assert_eq!(
            tools_call(br#"[{"jsonrpc":"2.0"}]"#),
            ToolsCallOutcome::ParseError
        );
    }

    #[test]
    fn a_valid_call_is_deferred_with_an_explicit_error() {
        // Deliberate fail-closed gap, asserted on the Rust side only: Go would
        // execute the tool and return a content envelope here, so pinning a
        // byte value would fake parity. MCP-003 replaces this with the executor.
        let text = response_text(
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"redact_file","arguments":{"path":"x"}}}"#,
        );
        assert!(text.contains("\"code\":-32603"), "{text}");
        assert!(
            text.contains("tool execution is not implemented in this slice"),
            "{text}"
        );
    }

    #[test]
    fn the_legacy_status_alias_validates_and_is_deferred() {
        let text = response_text(
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"status"}}"#,
        );
        assert!(text.contains("\"code\":-32603"), "{text}");
    }

    #[test]
    fn argument_kinds_match_the_go_checks() {
        assert!(json_value_matches("integer", &serde_json::json!(3)));
        assert!(!json_value_matches("integer", &serde_json::json!(3.5)));
        assert!(json_value_matches("boolean", &serde_json::json!(true)));
        assert!(!json_value_matches("boolean", &serde_json::json!("true")));
        assert!(json_value_matches("array", &serde_json::json!([])));
        assert!(!json_value_matches("array", &serde_json::json!({})));
        // An unknown kind matches everything, as in Go.
        assert!(json_value_matches("object", &serde_json::json!(1)));
    }
}
