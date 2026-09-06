//! Narrow comparison with field-level mismatch diagnostics.

use crate::case::{Case, ComparisonMode, Normalizer};
use crate::filesystem::{Manifest, manifest};
use crate::process::{RawStatus, RunResult, run_program};
use std::fmt::Write as _;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Difference {
    pub field: String,
    pub expected: String,
    pub actual: String,
}

impl std::fmt::Display for Difference {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            output,
            "{}: expected {}, got {}",
            self.field, self.expected, self.actual
        )
    }
}

fn normalized(mut bytes: Vec<u8>, path: &str, normalizers: &[Normalizer]) -> Vec<u8> {
    for rule in normalizers.iter().filter(|rule| rule.path == path) {
        if rule.from.is_empty() {
            continue;
        }
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

fn status_difference(expected: &RawStatus, actual: &RawStatus) -> Option<Difference> {
    if expected == actual {
        None
    } else {
        Some(Difference {
            field: "status".into(),
            expected: format!("{expected:?}"),
            actual: format!("{actual:?}"),
        })
    }
}

fn bytes_difference(field: &str, expected: &[u8], actual: &[u8]) -> Option<Difference> {
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
    })
}

fn format_bytes(bytes: &[u8]) -> String {
    let preview = bytes
        .iter()
        .take(64)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if bytes.len() > 64 {
        format!("0x{preview}… ({} bytes)", bytes.len())
    } else {
        format!("0x{preview} ({} bytes)", bytes.len())
    }
}

fn manifest_difference(expected: &Manifest, actual: &Manifest) -> Option<Difference> {
    if expected == actual {
        None
    } else {
        let first_expected = expected.entries.first();
        let first_actual = actual.entries.first();
        Some(Difference {
            field: "filesystem.manifest".into(),
            expected: format!(
                "{} entries; first={first_expected:?}",
                expected.entries.len()
            ),
            actual: format!("{} entries; first={first_actual:?}", actual.entries.len()),
        })
    }
}

fn compare_results(
    case: &Case,
    expected: &RunResult,
    actual: &RunResult,
    expected_manifest: &Manifest,
    actual_manifest: &Manifest,
) -> Vec<Difference> {
    let mut differences = Vec::new();
    if let Some(difference) = status_difference(&expected.status, &actual.status) {
        differences.push(difference);
    }
    let expected_stdout = normalized(expected.stdout.clone(), "stdout", &case.normalizers);
    let actual_stdout = normalized(actual.stdout.clone(), "stdout", &case.normalizers);
    if let Some(difference) = bytes_difference("stdout", &expected_stdout, &actual_stdout) {
        differences.push(difference);
    }
    let expected_stderr = normalized(expected.stderr.clone(), "stderr", &case.normalizers);
    let actual_stderr = normalized(actual.stderr.clone(), "stderr", &case.normalizers);
    if let Some(difference) = bytes_difference("stderr", &expected_stderr, &actual_stderr) {
        differences.push(difference);
    }
    if matches!(case.comparison, ComparisonMode::JsonSemantic) {
        differences.push(Difference {
            field: "comparison.mode".into(),
            expected: "supported semantic JSON comparator".into(),
            actual: "not enabled without a parser dependency".into(),
        });
    }
    if let Some(difference) = manifest_difference(expected_manifest, actual_manifest) {
        differences.push(difference);
    }
    differences
}

pub fn compare_case(case: &Case) -> std::io::Result<Result<(), Vec<Difference>>> {
    let go = run_program(case, &case.go)?;
    let rust = run_program(case, &case.rust)?;
    let go_manifest = manifest(&go.sandbox)?;
    let rust_manifest = manifest(&rust.sandbox)?;
    let differences = compare_results(case, &go, &rust, &go_manifest, &rust_manifest);
    let _ = std::fs::remove_dir_all(&go.sandbox);
    let _ = std::fs::remove_dir_all(&rust.sandbox);
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
        assert!(report.contains("expected 0x6f7261636c65"), "{report}");
        assert!(report.contains("got 0x72657772697465"), "{report}");
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
        assert!(
            result.is_err(),
            "the normalizer must not rewrite the other side"
        );
    }
}
