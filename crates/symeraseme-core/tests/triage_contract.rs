use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use symeraseme_core::triage_contract::{
    parse_classification_response, parse_rejection_classification, select_fallback_template,
};

const FIXTURE: &str = include_str!("../../../tests/fixtures/triage-contract/dom004a.json");
const CLASSIFIER_GO: &[u8] = include_bytes!("../../../internal/triage/classifier.go");
const REBUTTAL_GO: &[u8] = include_bytes!("../../../internal/triage/rebuttal.go");

#[derive(Debug, Deserialize)]
struct Fixture {
    schema: String,
    oracle_revision: String,
    source_sha256: BTreeMap<String, String>,
    classification_cases: Vec<ClassificationCase>,
    valid_utf8_cases: Vec<ValidUtf8Case>,
    rejection_cases: Vec<RejectionCase>,
    key_points_cases: Vec<KeyPointsCase>,
    fallback_cases: Vec<FallbackCase>,
}

#[derive(Debug, Deserialize)]
struct ClassificationCase {
    name: String,
    response: Option<String>,
    classification: Option<String>,
    event_type: Option<String>,
    confidence: Option<f64>,
    needs_human_review: Option<bool>,
    summary: Option<String>,
    extracted_fields: Option<BTreeMap<String, Value>>,
    ascii_prefix_repetitions: Option<usize>,
    suffix: Option<String>,
    cut_bytes: Option<usize>,
    raw_summary_bytes: Option<usize>,
    wire_summary_bytes: Option<usize>,
    raw_summary_tail_hex: Option<String>,
    wire_summary_tail: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ValidUtf8Case {
    name: String,
    unit: String,
    repetitions: usize,
    suffix: String,
    summary_bytes: usize,
    summary_tail: String,
}

#[derive(Debug, Deserialize)]
struct RejectionCase {
    name: String,
    response: String,
    classification: String,
    confidence: f64,
    jurisdiction: String,
    summary: String,
    key_points: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct KeyPointsCase {
    name: String,
    response: String,
    expected: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct FallbackCase {
    message: String,
    expected: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("DOM-004a fixture must be valid JSON")
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn fixture_is_bound_to_the_reviewed_go_oracle() {
    let fixture = fixture();
    assert_eq!(fixture.schema, "dom004a-triage-contract-v1");
    assert_eq!(
        fixture.oracle_revision,
        "f88a495044b6bb6eb23cd4610302b0f673068110"
    );
    assert_eq!(
        fixture.source_sha256["internal/triage/classifier.go"],
        sha256_hex(CLASSIFIER_GO)
    );
    assert_eq!(
        fixture.source_sha256["internal/triage/rebuttal.go"],
        sha256_hex(REBUTTAL_GO)
    );
}

#[test]
fn classification_cases_preserve_go_defaults_clamps_and_wire_order() {
    let fixture = fixture();
    for case in fixture
        .classification_cases
        .iter()
        .filter(|case| case.response.is_some())
    {
        let result = parse_classification_response(case.response.as_deref().unwrap());
        assert_eq!(
            result.classification,
            case.classification.clone().unwrap(),
            "{}",
            case.name
        );
        assert_eq!(
            result.event_type,
            case.event_type.clone().unwrap(),
            "{}",
            case.name
        );
        assert_eq!(result.confidence, case.confidence.unwrap(), "{}", case.name);
        assert_eq!(
            result.needs_human_review,
            case.needs_human_review.unwrap(),
            "{}",
            case.name
        );
        assert_eq!(
            result.summary.as_bytes(),
            case.summary.clone().unwrap().as_bytes(),
            "{}",
            case.name
        );
        assert_eq!(
            result.extracted_fields,
            case.extracted_fields.clone().unwrap(),
            "{}",
            case.name
        );
    }

    let result = parse_classification_response(
        r#"{"classification":"ack","confidence":0.9,"summary":"done","extracted_fields":{"z":1,"a":2}}"#,
    );
    let wire = serde_json::to_string(&result).expect("classification wire JSON");
    assert_json_field_order(
        &wire,
        &[
            "classification",
            "event_type",
            "confidence",
            "summary",
            "extracted_fields",
            "needs_human_review",
        ],
    );
    assert!(wire.contains(r#""extracted_fields":{"a":2,"z":1}"#));

    let null_result = parse_classification_response("null");
    assert_eq!(null_result.classification, "unclear");
    assert!(null_result.summary.as_bytes().is_empty());

    let low_result = parse_classification_response(
        r#"{"classification":"ack","confidence":0.3,"summary":"low"}"#,
    );
    assert_eq!(low_result.event_type, "ACK");
    assert!(low_result.needs_human_review);
}

#[test]
fn classification_utf8_boundary_uses_byte_cut_and_go_json_lossy_string() {
    let fixture = fixture();
    for case in fixture
        .classification_cases
        .iter()
        .filter(|case| case.ascii_prefix_repetitions.is_some())
    {
        let source =
            "a".repeat(case.ascii_prefix_repetitions.unwrap()) + case.suffix.as_deref().unwrap();
        let response = serde_json::to_string(&json!({
            "classification": "ack",
            "confidence": 0.9,
            "summary": source,
        }))
        .unwrap();
        let result = parse_classification_response(&response);
        let cut_bytes = case.cut_bytes.unwrap();
        assert_eq!(
            source.len(),
            case.ascii_prefix_repetitions.unwrap() + case.suffix.as_deref().unwrap().len(),
            "{}",
            case.name
        );
        assert!(source.len() > cut_bytes, "{}", case.name);
        assert_eq!(
            result.summary.as_bytes().len(),
            case.raw_summary_bytes.unwrap(),
            "{}",
            case.name
        );
        assert!(
            hex::encode(&result.summary.as_bytes()[result.summary.as_bytes().len() - 8..])
                == case.raw_summary_tail_hex.as_deref().unwrap(),
            "{}",
            case.name
        );
        let wire: Value = serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
        let wire_summary = wire["summary"].as_str().unwrap();
        assert_eq!(
            wire_summary.len(),
            case.wire_summary_bytes.unwrap(),
            "{}",
            case.name
        );
        assert!(
            wire_summary.ends_with(case.wire_summary_tail.as_deref().unwrap()),
            "{}",
            case.name
        );
        assert_eq!(wire_summary, result.summary.wire_string(), "{}", case.name);
    }
}

#[test]
fn rejection_cases_preserve_ordered_fields_and_fallbacks() {
    let fixture = fixture();
    for case in &fixture.rejection_cases {
        let result = parse_rejection_classification(&case.response);
        assert_eq!(result.classification, case.classification, "{}", case.name);
        assert_eq!(result.confidence, case.confidence, "{}", case.name);
        assert_eq!(result.jurisdiction, case.jurisdiction, "{}", case.name);
        assert_eq!(
            result.summary.as_bytes(),
            case.summary.as_bytes(),
            "{}",
            case.name
        );
        assert_eq!(result.key_points, case.key_points, "{}", case.name);
    }

    let parsed = parse_rejection_classification(
        r#"{"classification":"other","confidence":0.2,"summary":"done","key_points":[],"jurisdiction":"unknown"}"#,
    );
    let wire = serde_json::to_string(&parsed).expect("rejection wire JSON");
    assert_json_field_order(
        &wire,
        &[
            "classification",
            "confidence",
            "summary",
            "key_points",
            "jurisdiction",
        ],
    );
    assert!(wire.contains(r#""key_points":null"#));
    let nonempty =
        parse_rejection_classification(r#"{"classification":"other","key_points":[1,"one",true]}"#);
    let nonempty_wire = serde_json::to_string(&nonempty).expect("nonempty key_points wire JSON");
    assert!(nonempty_wire.contains(r#""key_points":["one"]"#));
    let null_result = parse_rejection_classification("null");
    assert_eq!(null_result.classification, "other");
    assert!(null_result.summary.as_bytes().is_empty());
    assert_eq!(null_result.jurisdiction, "unknown");

    let long_summary = serde_json::json!({
        "classification": "other",
        "summary": "é".repeat(101),
    });
    let long_result = parse_rejection_classification(&long_summary.to_string());
    assert_eq!(long_result.summary.as_bytes().len(), 200);
    assert!(long_result.summary.wire_string().ends_with("é"));

    for case in fixture.key_points_cases {
        let result = parse_rejection_classification(&case.response);
        assert_eq!(result.key_points, case.expected, "{}", case.name);
    }

    for case in fixture.fallback_cases {
        assert_eq!(
            select_fallback_template(&case.message),
            case.expected,
            "{}",
            case.message
        );
    }
}

#[test]
fn valid_two_hundred_byte_summaries_remain_unchanged() {
    let fixture = fixture();
    for case in fixture.valid_utf8_cases {
        let source = case.unit.repeat(case.repetitions) + &case.suffix;
        let response = serde_json::to_string(&json!({
            "classification": "ack",
            "confidence": 0.9,
            "summary": source,
        }))
        .unwrap();
        let result = parse_classification_response(&response);
        assert_eq!(
            result.summary.as_bytes().len(),
            case.summary_bytes,
            "{}",
            case.name
        );
        assert_eq!(
            result.summary.as_bytes(),
            source.as_bytes(),
            "{}",
            case.name
        );
        assert!(
            result.summary.wire_string().ends_with(&case.summary_tail),
            "{}",
            case.name
        );
    }
}

fn assert_json_field_order(wire: &str, fields: &[&str]) {
    let mut previous = 0;
    for field in fields {
        let needle = format!("\"{field}\"");
        let position = wire.find(&needle).expect("field present in wire JSON");
        assert!(position >= previous, "field {field} out of order in {wire}");
        previous = position;
    }
}
