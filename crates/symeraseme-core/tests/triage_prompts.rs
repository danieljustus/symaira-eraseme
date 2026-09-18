use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use symeraseme_core::redaction::RedactionProfile;
use symeraseme_core::triage_prompts::{build_rebuttal_classifier_prompt, build_user_prompt};

const FIXTURE: &str = include_str!("../../../tests/fixtures/triage-prompts/dom004b.json");
const CLASSIFIER_GO: &[u8] = include_bytes!("../../../internal/triage/classifier.go");
const REBUTTAL_GO: &[u8] = include_bytes!("../../../internal/triage/rebuttal.go");

#[derive(Debug, Deserialize)]
struct Fixture {
    schema: String,
    oracle_revision: String,
    source_sha256: std::collections::BTreeMap<String, String>,
    cases: Vec<PromptCase>,
}

#[derive(Debug, Deserialize)]
struct PromptCase {
    name: String,
    function: String,
    broker_name: String,
    #[serde(default)]
    broker_website: String,
    #[serde(default)]
    original_subject: String,
    #[serde(default)]
    original_snippet: String,
    #[serde(default)]
    reply_subject: String,
    #[serde(default)]
    reply_body: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    original_template: String,
    profile: Option<ProfileCase>,
    raw_prompt_base64: String,
    wire_json_base64: String,
}

#[derive(Debug, Deserialize)]
struct ProfileCase {
    full_name: String,
    name_variants: Vec<String>,
    email_addresses: Vec<String>,
    phone_numbers: Vec<String>,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("DOM-004b fixture must be valid JSON")
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn profile(value: Option<&ProfileCase>) -> Option<RedactionProfile> {
    value.map(|value| RedactionProfile {
        full_name: value.full_name.clone(),
        name_variants: value.name_variants.clone(),
        email_addresses: value.email_addresses.clone(),
        phone_numbers: value.phone_numbers.clone(),
        ..RedactionProfile::default()
    })
}

#[test]
fn fixture_is_bound_to_the_reviewed_go_prompt_oracle() {
    let fixture = fixture();
    assert_eq!(fixture.schema, "dom004b-triage-prompts-v1");
    assert_eq!(
        fixture.oracle_revision,
        "387a81d116a0e62ac9704841ed3381028fae033b"
    );
    assert_eq!(
        fixture.source_sha256["internal/triage/classifier.go"],
        sha256_hex(CLASSIFIER_GO)
    );
    assert_eq!(
        fixture.source_sha256["internal/triage/rebuttal.go"],
        sha256_hex(REBUTTAL_GO)
    );
    assert_eq!(fixture.cases.len(), 9);
}

#[test]
fn prompt_cases_match_go_raw_bytes_and_json_wire() {
    for case in fixture().cases {
        let redaction = profile(case.profile.as_ref());
        let prompt = match case.function.as_str() {
            "BuildUserPrompt" => build_user_prompt(
                &case.broker_name,
                &case.broker_website,
                &case.original_subject,
                &case.original_snippet,
                &case.reply_subject,
                &case.reply_body,
                redaction.as_ref(),
            ),
            "BuildRebuttalClassifierPrompt" => build_rebuttal_classifier_prompt(
                &case.broker_name,
                &case.message,
                &case.original_template,
                redaction.as_ref(),
            ),
            other => panic!("unknown prompt function {other}"),
        };
        assert_eq!(
            prompt.as_bytes(),
            STANDARD
                .decode(&case.raw_prompt_base64)
                .expect("raw prompt base64"),
            "{} raw prompt",
            case.name
        );
        assert_eq!(
            prompt.wire_json(),
            STANDARD
                .decode(&case.wire_json_base64)
                .expect("wire JSON base64"),
            "{} JSON wire",
            case.name
        );
    }
}

#[test]
fn prompt_limits_are_byte_limits_and_wire_invalid_utf8_is_lossy() {
    let prompt = build_user_prompt(
        "Acme",
        "https://acme.test",
        "Request",
        &("A".repeat(499) + "é"),
        "Reply",
        "body",
        None,
    );
    let marker = b"Original request body (truncated):\n";
    let snippet_start = prompt
        .as_bytes()
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("original snippet marker")
        + marker.len();
    assert_eq!(prompt.as_bytes()[snippet_start + 499], 0xc3);
    assert!(std::str::from_utf8(prompt.as_bytes()).is_err());
    assert!(prompt.wire_string().contains('\u{fffd}'));

    let prompt =
        build_rebuttal_classifier_prompt("Acme", &("A".repeat(2_999) + "é"), "template", None);
    assert_eq!(prompt.as_bytes().last(), Some(&0xc3));
    assert!(prompt.wire_string().ends_with('\u{fffd}'));
}

#[test]
fn wire_json_matches_go_html_escaping() {
    let prompt = build_user_prompt(
        "<&>",
        "https://acme.test",
        "line\u{2028}separator",
        "body",
        "line\u{2029}separator",
        "body",
        None,
    );
    let wire = String::from_utf8(prompt.wire_json()).expect("JSON is UTF-8");
    assert!(wire.contains("\\u003c\\u0026\\u003e"));
    assert!(wire.contains("line\\u2028separator"));
    assert!(wire.contains("line\\u2029separator"));
}
