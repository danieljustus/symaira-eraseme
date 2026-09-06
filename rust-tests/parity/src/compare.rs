//! Field-level parity comparison with raw transport and SQLite diagnostics.

use crate::case::{
    Case, ComparisonMode, Normalizer, NormalizerScope, normalizer_reasons, normalizers_for,
    validate_normalizers,
};
use crate::filesystem::{Manifest, manifest};
use crate::process::{RawStatus, RunResult, run_program};
use crate::sqlite::{SqliteSnapshot, snapshot_database};
use std::fmt::Write as _;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Difference {
    pub field: String,
    pub expected: String,
    pub actual: String,
    pub reason: Option<String>,
}

impl std::fmt::Display for Difference {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            output,
            "{}: expected {}, got {}",
            self.field, self.expected, self.actual
        )?;
        if let Some(reason) = &self.reason {
            write!(output, " (normalizer reason: {reason})")?;
        }
        Ok(())
    }
}

fn normalized(mut bytes: Vec<u8>, scope: &NormalizerScope, normalizers: &[Normalizer]) -> Vec<u8> {
    for rule in normalizers_for(scope, normalizers) {
        let mut result = Vec::with_capacity(bytes.len());
        let mut cursor = 0;
        while let Some(relative) = bytes[cursor..]
            .windows(rule.from.len())
            .position(|window| window == rule.from)
        {
            let index = cursor + relative;
            result.extend_from_slice(&bytes[cursor..index]);
            result.extend_from_slice(&rule.to);
            cursor = index + rule.from.len();
        }
        result.extend_from_slice(&bytes[cursor..]);
        bytes = result;
    }
    bytes
}

fn reason(scope: &NormalizerScope, normalizers: &[Normalizer]) -> Option<String> {
    let reasons = normalizer_reasons(scope, normalizers);
    (!reasons.is_empty()).then_some(reasons)
}

fn status_difference(expected: &RawStatus, actual: &RawStatus) -> Option<Difference> {
    if expected == actual {
        None
    } else {
        Some(Difference {
            field: "status".into(),
            expected: format!("{expected:?}"),
            actual: format!("{actual:?}"),
            reason: None,
        })
    }
}

fn bytes_difference(
    field: &str,
    expected: &[u8],
    actual: &[u8],
    reason: Option<String>,
) -> Option<Difference> {
    if expected == actual {
        return None;
    }
    let common = expected
        .iter()
        .zip(actual)
        .take_while(|(left, right)| left == right)
        .count();
    Some(Difference {
        field: format!("{field}[byte {common}]"),
        expected: format_bytes(expected),
        actual: format_bytes(actual),
        reason,
    })
}

fn format_bytes(bytes: &[u8]) -> String {
    format!("<redacted bytes: {} bytes>", bytes.len())
}

#[derive(Default)]
struct JsonShape {
    nodes: usize,
    objects: usize,
    arrays: usize,
    strings: usize,
    numbers: usize,
    booleans: usize,
    nulls: usize,
    max_depth: usize,
    truncated: bool,
}

fn collect_json_shape(value: &serde_json::Value, depth: usize, shape: &mut JsonShape) {
    if shape.nodes >= 128 {
        shape.truncated = true;
        return;
    }
    shape.nodes += 1;
    shape.max_depth = shape.max_depth.max(depth);
    match value {
        serde_json::Value::Object(fields) => {
            shape.objects += 1;
            for value in fields.values() {
                collect_json_shape(value, depth + 1, shape);
            }
        }
        serde_json::Value::Array(values) => {
            shape.arrays += 1;
            for value in values {
                collect_json_shape(value, depth + 1, shape);
            }
        }
        serde_json::Value::String(_) => shape.strings += 1,
        serde_json::Value::Number(_) => shape.numbers += 1,
        serde_json::Value::Bool(_) => shape.booleans += 1,
        serde_json::Value::Null => shape.nulls += 1,
    }
}

fn format_json_shape(value: &serde_json::Value) -> String {
    let mut shape = JsonShape::default();
    collect_json_shape(value, 0, &mut shape);
    format!(
        "<redacted JSON shape: nodes={}, objects={}, arrays={}, strings={}, numbers={}, booleans={}, nulls={}, depth={}, truncated={}>",
        shape.nodes,
        shape.objects,
        shape.arrays,
        shape.strings,
        shape.numbers,
        shape.booleans,
        shape.nulls,
        shape.max_depth,
        shape.truncated
    )
}

fn json_semantic_difference(
    field: &str,
    expected: &[u8],
    actual: &[u8],
    reason: Option<String>,
) -> Option<Difference> {
    let expected_value = match serde_json::from_slice::<serde_json::Value>(expected) {
        Ok(value) => value,
        Err(_error) => {
            return Some(Difference {
                field: format!("{field}.json"),
                expected: "<invalid JSON>".into(),
                actual: format_bytes(actual),
                reason,
            });
        }
    };
    let actual_value = match serde_json::from_slice::<serde_json::Value>(actual) {
        Ok(value) => value,
        Err(_error) => {
            return Some(Difference {
                field: format!("{field}.json"),
                expected: format_json_shape(&expected_value),
                actual: "<invalid JSON>".into(),
                reason,
            });
        }
    };
    (expected_value != actual_value).then(|| Difference {
        field: format!("{field}.json"),
        expected: format_json_shape(&expected_value),
        actual: format_json_shape(&actual_value),
        reason,
    })
}

fn manifest_difference(expected: &Manifest, actual: &Manifest) -> Option<Difference> {
    if expected == actual {
        None
    } else {
        let structure = |manifest: &Manifest| {
            let files = manifest
                .entries
                .iter()
                .filter(|entry| matches!(entry.entry_type, crate::filesystem::EntryType::File))
                .count();
            let directories = manifest
                .entries
                .iter()
                .filter(|entry| matches!(entry.entry_type, crate::filesystem::EntryType::Directory))
                .count();
            format!(
                "<redacted manifest: {} entries, {} files, {} directories>",
                manifest.entries.len(),
                files,
                directories
            )
        };
        Some(Difference {
            field: "filesystem.manifest".into(),
            expected: structure(expected),
            actual: structure(actual),
            reason: None,
        })
    }
}

fn compare_http(
    expected: &[crate::http::HttpExchange],
    actual: &[crate::http::HttpExchange],
    normalizers: &[Normalizer],
) -> Vec<Difference> {
    let mut differences = Vec::new();
    if expected.len() != actual.len() {
        differences.push(Difference {
            field: "http.exchange.count".into(),
            expected: expected.len().to_string(),
            actual: actual.len().to_string(),
            reason: None,
        });
    }
    for (index, (left, right)) in expected.iter().zip(actual).enumerate() {
        let request_scope = NormalizerScope::HttpRequest;
        let response_scope = NormalizerScope::HttpResponse;
        let request = bytes_difference(
            &format!("http[{index}].request"),
            &normalized(left.request.clone(), &request_scope, normalizers),
            &normalized(right.request.clone(), &request_scope, normalizers),
            reason(&request_scope, normalizers),
        );
        let response = bytes_difference(
            &format!("http[{index}].response"),
            &normalized(left.response.clone(), &response_scope, normalizers),
            &normalized(right.response.clone(), &response_scope, normalizers),
            reason(&response_scope, normalizers),
        );
        differences.extend(request.into_iter().chain(response));
    }
    differences
}

fn compare_mcp(
    expected: &[crate::mcp::RawFrame],
    actual: &[crate::mcp::RawFrame],
    normalizers: &[Normalizer],
) -> Vec<Difference> {
    let mut differences = Vec::new();
    if expected.len() != actual.len() {
        differences.push(Difference {
            field: "mcp.frame.count".into(),
            expected: expected.len().to_string(),
            actual: actual.len().to_string(),
            reason: None,
        });
    }
    for (index, (left, right)) in expected.iter().zip(actual).enumerate() {
        if left.direction != right.direction {
            differences.push(Difference {
                field: format!("mcp[{index}].direction"),
                expected: format!("{:?}", left.direction),
                actual: format!("{:?}", right.direction),
                reason: None,
            });
            continue;
        }
        let scope = match left.direction {
            crate::mcp::Direction::Stdin => NormalizerScope::McpStdin,
            crate::mcp::Direction::Stdout => NormalizerScope::McpStdout,
        };
        if let Some(difference) = bytes_difference(
            &format!("mcp[{index}].bytes"),
            &normalized(left.bytes.clone(), &scope, normalizers),
            &normalized(right.bytes.clone(), &scope, normalizers),
            reason(&scope, normalizers),
        ) {
            differences.push(difference);
        }
    }
    differences
}

fn sqlite_text_structure(text: &str) -> String {
    format!(
        "<redacted SQLite text: {} bytes, {} rows>",
        text.len(),
        text.lines().count()
    )
}

fn sqlite_results_structure(results: &[String]) -> String {
    let bytes = results.iter().map(String::len).sum::<usize>();
    let rows = results
        .iter()
        .map(|result| result.lines().count())
        .sum::<usize>();
    format!(
        "<redacted SQLite results: {} queries, {} bytes, {} rows>",
        results.len(),
        bytes,
        rows
    )
}

fn compare_sqlite(expected: &SqliteSnapshot, actual: &SqliteSnapshot) -> Vec<Difference> {
    let mut differences = Vec::new();
    if expected.schema != actual.schema {
        differences.push(Difference {
            field: "sqlite.schema".into(),
            expected: sqlite_text_structure(&expected.schema),
            actual: sqlite_text_structure(&actual.schema),
            reason: None,
        });
    }
    if expected.ordered_results != actual.ordered_results {
        differences.push(Difference {
            field: "sqlite.ordered_results".into(),
            expected: sqlite_results_structure(&expected.ordered_results),
            actual: sqlite_results_structure(&actual.ordered_results),
            reason: None,
        });
    }
    differences
}

fn compare_results(
    case: &Case,
    expected: &RunResult,
    actual: &RunResult,
    expected_manifest: &Manifest,
    actual_manifest: &Manifest,
    expected_sqlite: Option<&SqliteSnapshot>,
    actual_sqlite: Option<&SqliteSnapshot>,
) -> Vec<Difference> {
    let mut differences = Vec::new();
    if let Some(difference) = status_difference(&expected.status, &actual.status) {
        differences.push(difference);
    }
    let stdout_scope = NormalizerScope::Stdout;
    let expected_stdout = normalized(expected.stdout.clone(), &stdout_scope, &case.normalizers);
    let actual_stdout = normalized(actual.stdout.clone(), &stdout_scope, &case.normalizers);
    let stdout_difference = match case.comparison {
        ComparisonMode::Exact => bytes_difference(
            "stdout",
            &expected_stdout,
            &actual_stdout,
            reason(&stdout_scope, &case.normalizers),
        ),
        ComparisonMode::JsonSemantic => json_semantic_difference(
            "stdout",
            &expected_stdout,
            &actual_stdout,
            reason(&stdout_scope, &case.normalizers),
        ),
    };
    differences.extend(stdout_difference);
    let stderr_scope = NormalizerScope::Stderr;
    if let Some(difference) = bytes_difference(
        "stderr",
        &normalized(expected.stderr.clone(), &stderr_scope, &case.normalizers),
        &normalized(actual.stderr.clone(), &stderr_scope, &case.normalizers),
        reason(&stderr_scope, &case.normalizers),
    ) {
        differences.push(difference);
    }
    differences.extend(compare_http(
        &expected.http_exchanges,
        &actual.http_exchanges,
        &case.normalizers,
    ));
    differences.extend(compare_mcp(
        &expected.mcp_frames,
        &actual.mcp_frames,
        &case.normalizers,
    ));
    if let (Some(expected_sqlite), Some(actual_sqlite)) = (expected_sqlite, actual_sqlite) {
        differences.extend(compare_sqlite(expected_sqlite, actual_sqlite));
    }
    if let Some(difference) = manifest_difference(expected_manifest, actual_manifest) {
        differences.push(difference);
    }
    differences
}

pub fn compare_case(case: &Case) -> std::io::Result<Result<(), Vec<Difference>>> {
    validate_normalizers(&case.normalizers)?;
    if case.sqlite_database.is_none() && !case.sqlite_queries.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "sqlite_queries requires sqlite_database",
        ));
    }
    if let Some(database) = &case.sqlite_database {
        crate::process::validate_fixture_path(database)?;
    }
    let go = run_program(case, &case.go)?;
    let rust = run_program(case, &case.rust)?;
    let go_manifest = manifest(&go.sandbox)?;
    let rust_manifest = manifest(&rust.sandbox)?;
    let go_sqlite = case
        .sqlite_database
        .as_ref()
        .map(|database| {
            crate::sqlite::reject_symlink_components(&go.cwd, database)?;
            snapshot_database(&go.cwd.join(database), &case.sqlite_queries)
        })
        .transpose()?;
    let rust_sqlite = case
        .sqlite_database
        .as_ref()
        .map(|database| {
            crate::sqlite::reject_symlink_components(&rust.cwd, database)?;
            snapshot_database(&rust.cwd.join(database), &case.sqlite_queries)
        })
        .transpose()?;
    let differences = compare_results(
        case,
        &go,
        &rust,
        &go_manifest,
        &rust_manifest,
        go_sqlite.as_ref(),
        rust_sqlite.as_ref(),
    );
    Ok(if differences.is_empty() {
        Ok(())
    } else {
        Err(differences)
    })
}

pub fn format_differences(differences: &[Difference]) -> String {
    let mut output = String::new();
    for difference in differences {
        let _ = writeln!(output, "- {difference}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::{Case, Program};
    use std::path::PathBuf;

    fn dummy(text: &str) -> Program {
        Program {
            executable: PathBuf::from("/bin/sh"),
            argv: vec!["-c".into(), format!("printf '%s' '{text}'")],
        }
    }

    #[test]
    fn identical_dummy_programs_pass() {
        let case = Case::new("equal-dummies", dummy("same output"), dummy("same output"));
        assert_eq!(compare_case(&case).unwrap(), Ok(()));
    }

    #[test]
    fn deliberate_output_mismatch_has_actionable_field_diff() {
        let case = Case::new("mismatch-dummies", dummy("oracle"), dummy("rewrite"));
        let differences = compare_case(&case)
            .unwrap()
            .expect_err("mismatch must fail");
        let report = format_differences(&differences);
        assert!(report.contains("stdout[byte 0]"), "{report}");
        assert!(report.contains("<redacted bytes: 6 bytes>"), "{report}");
        assert!(report.contains("<redacted bytes: 7 bytes>"), "{report}");
    }

    #[test]
    fn semantic_json_comparison_accepts_key_order_difference() {
        let mut case = Case::new(
            "semantic-equal",
            dummy(r#"{"a":1,"b":[true]}"#),
            dummy(r#"{"b":[true],"a":1}"#),
        );
        case.comparison = ComparisonMode::JsonSemantic;
        case.capture_mcp = false;
        assert_eq!(compare_case(&case).unwrap(), Ok(()));
    }

    #[test]
    fn semantic_json_comparison_reports_value_mismatch() {
        let mut case = Case::new(
            "semantic-mismatch",
            dummy(r#"{"a":1}"#),
            dummy(r#"{"a":2}"#),
        );
        case.comparison = ComparisonMode::JsonSemantic;
        case.capture_mcp = false;
        let differences = compare_case(&case).unwrap().expect_err("JSON differs");
        assert!(format_differences(&differences).contains("stdout.json"));
    }

    #[test]
    fn semantic_json_mismatch_preview_is_utf8_boundary_safe() {
        let expected = format!(r#"{{"text":"{}"}}"#, "é".repeat(300));
        let actual = format!(r#"{{"text":"{}"}}"#, "é".repeat(299) + "x");
        let mut case = Case::new(
            "semantic-unicode-mismatch",
            dummy(&expected),
            dummy(&actual),
        );
        case.comparison = ComparisonMode::JsonSemantic;
        case.capture_mcp = false;
        let differences = compare_case(&case).unwrap().expect_err("JSON differs");
        let report = format_differences(&differences);
        assert!(report.contains("stdout.json"), "{report}");
        assert!(report.contains("<redacted JSON shape:"), "{report}");
        assert!(!report.contains("é"), "{report}");
    }

    #[test]
    fn exact_reason_tagged_normalizer_is_scoped_to_a_field() {
        let mut case = Case::new("normalize", dummy("root-A"), dummy("root-B"));
        case.normalizers.push(Normalizer::exact(
            "stdout",
            "isolated runtime root",
            b"root-A",
            b"<ROOT>",
        ));
        let result = compare_case(&case).unwrap();
        let report = format_differences(&result.expect_err("the other side differs"));
        assert!(report.contains("normalizer reason: isolated runtime root"));
    }
}
