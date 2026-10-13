//! Portable synthetic controls for the native recorder, not Windows observations.
#[path = "support/capture_windows_http_process.rs"]
#[allow(dead_code)]
mod capture_windows_http_process;

#[test]
fn native_capture_owners_finish_after_comparisons() {
    let source = include_str!("mcp_http_windows_process.rs");
    for case in capture_windows_http_process::CASES {
        let owner = if case.starts_with("native_windows_ctrl_") {
            "console_shutdown_matches_go"
        } else {
            case
        };
        let declaration = format!("fn {owner}(");
        let (_, body) = source.split_once(declaration.as_str()).unwrap();
        let (body, _) = body.split_once("\n}").unwrap();
        assert!(
            body.trim_end().ends_with("capture.finish();"),
            "{owner} must finalize its record after the original comparisons"
        );
    }
}
