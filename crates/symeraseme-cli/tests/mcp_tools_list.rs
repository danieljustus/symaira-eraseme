// `tools_list` now shares the Go-compatible HTML escaping with the response
// envelope, so the envelope module has to be compiled in here too.
#[path = "../src/mcp/envelope.rs"]
mod envelope;
#[path = "../src/mcp/tools_list.rs"]
mod tools_list;
// #1296/#1342: the two changed tools are compared through narrow, documented overrides.
#[path = "../../../rust-tests/brokers_paging.rs"]
mod brokers_paging;
#[path = "../../../rust-tests/dashboard_compact.rs"]
mod dashboard_compact;

use tools_list::{ToolsListOutcome, tools_list};

const LIST_REQUEST: &[u8] =
    include_bytes!("../../../tests/fixtures/mcp-contract/mcp-002/tools-list.request.jsonl");
const LIST_RESPONSE: &[u8] =
    include_bytes!("../../../tests/fixtures/mcp-contract/mcp-002/tools-list.response.json");
const INVALID_PARAMS_REQUEST: &[u8] = include_bytes!(
    "../../../tests/fixtures/mcp-contract/mcp-002/tools-list.invalid-params.request.jsonl"
);
const INVALID_PARAMS_RESPONSE: &[u8] = include_bytes!(
    "../../../tests/fixtures/mcp-contract/mcp-002/tools-list.invalid-params.response.json"
);
const NOTIFICATION: &[u8] = include_bytes!(
    "../../../tests/fixtures/mcp-contract/mcp-002/tools-list.notification.request.jsonl"
);

#[test]
fn tools_list_matches_source_bound_response_shape() {
    let ToolsListOutcome::Response(response) = tools_list(LIST_REQUEST) else {
        panic!("tools/list request did not return a response");
    };
    // Recorded bytes, except the compact `get_dashboard_data` and paged
    // `list_brokers` entries (#1296, #1342).
    assert_eq!(
        response,
        brokers_paging::with_paged_brokers_tool(&dashboard_compact::with_compact_dashboard_tool(
            LIST_RESPONSE
        ))
    );
    let document = serde_json::from_slice::<serde_json::Value>(&response).expect("response JSON");
    let names = document
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(serde_json::Value::as_array)
        .expect("result.tools");
    assert_eq!(names.len(), 26);
    assert_eq!(names[0]["name"], "redact_file");
    assert_eq!(names[25]["name"], "grant");
}

#[test]
fn tools_list_rejects_non_object_params() {
    assert_eq!(
        tools_list(INVALID_PARAMS_REQUEST),
        ToolsListOutcome::Response(INVALID_PARAMS_RESPONSE.to_vec())
    );
}

#[test]
fn tools_list_notifications_are_silent() {
    assert_eq!(tools_list(NOTIFICATION), ToolsListOutcome::Notification);
}
