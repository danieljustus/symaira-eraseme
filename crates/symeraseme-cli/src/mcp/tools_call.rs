//! Transport-independent MCP `tools/call` request validation.
//!
//! Ports the validation half of the `tools/call` case in
//! `internal/mcp/server.go` (`handleRequest` + `validateToolArguments` +
//! `jsonValueMatches`). Tool *execution* is a later slice, so a request that
//! passes validation is answered with an explicit fail-closed error rather
//! than a fabricated result.

use serde_json::Value;

use super::handler::ToolHandler;

const TOOL_CATALOGUE: &[u8] = include_bytes!("../../../../internal/mcp/tools.json");

/// The legacy name Go accepts in `tools/call` without a catalogue entry.
const LEGACY_STATUS_ALIAS: &str = "status";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ToolsCallOutcome {
    Response(Vec<u8>),
    Notification,
    ParseError,
}

/// Handles one JSON-RPC `tools/call` request without transport concerns.
pub(crate) fn tools_call(raw: &[u8], handler: &dyn ToolHandler) -> ToolsCallOutcome {
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
        // The legacy `status` alias passes validation but has no case in Go's
        // contract handler either, so the handler answers with its default.
        return dispatch(handler, id, name, arguments);
    };

    if let Err(message) = validate_arguments(tool, arguments) {
        return response_error(id, -32602, &message);
    }

    dispatch(handler, id, name, arguments)
}

/// Runs the handler and maps its outcome the way Go does: a result becomes the
/// content envelope, a failure becomes a sanitized `-32603`.
fn dispatch(
    handler: &dyn ToolHandler,
    id: &Value,
    name: &str,
    arguments: &serde_json::Map<String, Value>,
) -> ToolsCallOutcome {
    match handler.call(name, arguments) {
        Ok(result) => {
            ToolsCallOutcome::Response(super::envelope::result_response(id, Some(&result)))
        }
        Err(error) => response_error(
            id,
            -32603,
            &super::envelope::sanitize_error(&error.to_string()),
        ),
    }
}

/// Whether the embedded catalogue contains a tool with this name.
pub(crate) fn catalogue_has_tool(name: &str) -> bool {
    let catalogue: Value =
        serde_json::from_slice(TOOL_CATALOGUE).expect("Go MCP catalogue is valid JSON");
    catalogue
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools
                .iter()
                .any(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
        })
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

fn response_error(id: &Value, code: i32, message: &str) -> ToolsCallOutcome {
    ToolsCallOutcome::Response(super::envelope::error_response(id, code, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::handler::test_support::no_backend_handler;

    fn response_text(raw: &str) -> String {
        match tools_call(raw.as_bytes(), &no_backend_handler()) {
            ToolsCallOutcome::Response(bytes) => {
                String::from_utf8(bytes).expect("response is UTF-8")
            }
            other => panic!("expected response, got {other:?}"),
        }
    }

    #[test]
    fn non_object_payloads_are_parse_errors() {
        assert_eq!(
            tools_call(b"not json", &no_backend_handler()),
            ToolsCallOutcome::ParseError
        );
        assert_eq!(
            tools_call(br#"[{"jsonrpc":"2.0"}]"#, &no_backend_handler()),
            ToolsCallOutcome::ParseError
        );
    }

    #[test]
    fn a_valid_call_matches_the_go_handler_less_backend_error() {
        // Go substitutes a failing default handler when none is injected, so
        // this is the defined answer of a handler-less server. It is pinned by
        // the mcp-result fixture case default_handler_reports_missing_backend.
        // MCP-003 replaces the deferral itself with a real executor.
        assert_eq!(
            response_text(
                r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"redact_file","arguments":{"path":"x"}}}"#,
            ),
            "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"tool backend is not available\"},\"id\":9}\n"
        );
    }

    #[test]
    fn the_legacy_status_alias_reaches_the_handler() {
        // `status` passes validation but is not in the catalogue, so it lands on
        // the handler's default — the same path Go takes.
        let text = response_text(
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"status"}}"#,
        );
        assert_eq!(
            text,
            "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"tool backend is not available\"},\"id\":9}\n"
        );
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
