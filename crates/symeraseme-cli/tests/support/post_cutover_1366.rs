//! Explicit expected-value transforms for the post-cutover contract change
//! #1366 (see `docs/rust-port-contract-matrix.md`).
//!
//! The frozen Go records stay untouched. Go printed only `success` for
//! text-mode `generate-report`, which dropped the report when no `--output`
//! file was named and never said where a named file went. Each transform
//! replaces exactly that recorded `success\n` and passes anything else through.
#![allow(dead_code)] // Each including test target uses a subset.

/// The report the recorded empty store renders with `--format json`, its
/// `generated_at` instant masked as by [`mask_generated_at`].
const EMPTY_STORE_JSON_REPORT: &str = "{\n  \"broker_leaderboard\": [],\n  \"campaigns\": [],\n  \"error\": \"Campaign 'none' not found or empty\",\n  \"generated_at\": \"<TIMESTAMP>\",\n  \"historical_comparison\": {},\n  \"jurisdiction_stats\": [],\n  \"status_breakdown\": {},\n  \"success_metrics\": {},\n  \"timeline\": [],\n  \"total_campaigns\": 0,\n  \"total_requests\": 0\n}\n";

/// `operate-generate-report` (empty store, `--format json`, `--output ''`):
/// the report itself replaces the recorded `success\n`.
pub fn report_text(recorded: &[u8]) -> Vec<u8> {
    if recorded != b"success\n" {
        return recorded.to_vec();
    }
    EMPTY_STORE_JSON_REPORT.as_bytes().to_vec()
}

/// Masks the `"generated_at": "<instant>"` value of a pretty-printed JSON
/// report after checking that it is an RFC 3339 instant.
pub fn mask_generated_at(stdout: &[u8]) -> Vec<u8> {
    let text = String::from_utf8(stdout.to_vec()).expect("UTF-8 report");
    let prefix = "\"generated_at\": \"";
    let Some(relative) = text.find(prefix) else {
        return stdout.to_vec();
    };
    let start = relative + prefix.len();
    let end = start + text[start..].find('"').expect("a terminated instant");
    symeraseme_core::timeutil::parse_rfc3339(&text[start..end])
        .expect("generated_at is an RFC 3339 instant");
    format!("{}<TIMESTAMP>{}", &text[..start], &text[end..]).into_bytes()
}

/// A run that named a file (`--output <path>`): `wrote <path> (<format>)`
/// replaces the recorded `success\n`. `recorded_sha256` is the digest the
/// filesystem fixtures keep instead of the bytes.
pub fn written_report_text(recorded_sha256: &str, path: &str, format: &str) -> Option<Vec<u8>> {
    const SUCCESS_SHA256: &str = "81b2bd4ea98c8db66554fbc8d7637a1a69a130f331feb732b75caab4c4868fd5";
    (recorded_sha256 == SUCCESS_SHA256).then(|| format!("wrote {path} ({format})\n").into_bytes())
}

#[test]
fn generate_report_transforms_are_narrow() {
    assert_eq!(report_text(b"other\n"), b"other\n");
    assert!(report_text(b"success\n").starts_with(b"{\n  \"broker_leaderboard\""));
    let masked = mask_generated_at(b"{\n  \"generated_at\": \"2026-11-28T02:05:07+00:00\",\n}\n");
    assert_eq!(masked, b"{\n  \"generated_at\": \"<TIMESTAMP>\",\n}\n");
    assert_eq!(
        written_report_text(
            "81b2bd4ea98c8db66554fbc8d7637a1a69a130f331feb732b75caab4c4868fd5",
            "<ORACLE_ROOT>/r.html",
            "html"
        ),
        Some(b"wrote <ORACLE_ROOT>/r.html (html)\n".to_vec())
    );
    assert_eq!(written_report_text("e3b0c442", "p", "html"), None);
}
