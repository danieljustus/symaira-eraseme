//! Pure broker-reply triage contracts.
//!
//! This module deliberately stops at parsing and deterministic fallback
//! selection.  LLM transport, prompt construction, template rendering,
//! persistence, and event emission belong to later adapters.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const CONFIDENCE_THRESHOLD_LOW: f64 = 0.4;
pub const SUMMARY_BYTE_LIMIT: usize = 200;

/// The raw summary bytes retained after the Go-compatible byte cut.
///
/// The raw value is intentionally separate from its JSON representation:
/// Go's parser can retain an invalid UTF-8 suffix in a string, while
/// `encoding/json` replaces each invalid byte only when serializing it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SummaryBytes(Vec<u8>);

impl SummaryBytes {
    /// Borrow the exact bytes retained by the parser for persistence.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consume the summary and return its exact retained bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Return the Go-compatible JSON string representation.
    pub fn wire_string(&self) -> String {
        go_json_lossy(&self.0)
    }

    fn truncated(value: &str) -> Self {
        let bytes = value.as_bytes();
        let bytes = if bytes.len() > SUMMARY_BYTE_LIMIT {
            &bytes[..SUMMARY_BYTE_LIMIT]
        } else {
            bytes
        };
        Self(bytes.to_vec())
    }
}

impl Serialize for SummaryBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.wire_string())
    }
}

impl<'de> Deserialize<'de> for SummaryBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|value| Self(value.into_bytes()))
    }
}

/// A parsed classifier result in the Go wire shape.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClassificationResult {
    pub classification: String,
    pub event_type: String,
    pub confidence: f64,
    pub summary: SummaryBytes,
    pub extracted_fields: BTreeMap<String, Value>,
    pub needs_human_review: bool,
}

impl Default for ClassificationResult {
    fn default() -> Self {
        Self {
            classification: "unclear".to_owned(),
            event_type: "HUMAN_ACTION_REQUIRED".to_owned(),
            confidence: 0.0,
            summary: SummaryBytes(Vec::new()),
            extracted_fields: BTreeMap::new(),
            needs_human_review: true,
        }
    }
}

/// Parse a classifier response, accepting the same optional JSON code fence
/// as the Go implementation.
pub fn parse_classification_response(response: &str) -> ClassificationResult {
    let mut result = ClassificationResult::default();
    let data = match parse_object(response) {
        Ok(data) => data,
        Err(()) => {
            result.summary = SummaryBytes::truncated("Failed to parse classifier response");
            return result;
        }
    };

    let classification = data
        .get("classification")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_lowercase())
        .filter(|value| classification_event(value).is_some())
        .unwrap_or_else(|| "unclear".to_owned());
    let confidence = clamp_confidence(data.get("confidence"));
    let summary = data
        .get("summary")
        .and_then(Value::as_str)
        .map(SummaryBytes::truncated)
        .unwrap_or_default();
    let extracted_fields = data
        .get("extracted_fields")
        .and_then(Value::as_object)
        .map(sorted_fields)
        .unwrap_or_default();

    result.classification = classification.clone();
    result.event_type = classification_event(&classification)
        .expect("classification was validated above")
        .to_owned();
    result.confidence = confidence;
    result.summary = summary;
    result.extracted_fields = extracted_fields;
    result.needs_human_review =
        confidence < CONFIDENCE_THRESHOLD_LOW || classification == "unclear";
    result
}

/// Descriptive alias matching the Go service caller name.
pub fn parse_response(response: &str) -> ClassificationResult {
    parse_classification_response(response)
}

/// A parsed rejection classifier result in the Go wire shape.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RejectionClassification {
    pub classification: String,
    pub confidence: f64,
    pub summary: SummaryBytes,
    pub key_points: Option<Vec<String>>,
    pub jurisdiction: String,
}

impl Default for RejectionClassification {
    fn default() -> Self {
        Self {
            classification: "other".to_owned(),
            confidence: 0.0,
            summary: SummaryBytes(Vec::new()),
            key_points: None,
            jurisdiction: "unknown".to_owned(),
        }
    }
}

/// Parse a rejection-classification response without invoking an LLM.
pub fn parse_rejection_classification(response: &str) -> RejectionClassification {
    let mut result = RejectionClassification::default();
    let data = match parse_object(response) {
        Ok(data) => data,
        Err(()) => {
            result.summary = SummaryBytes::truncated("Failed to parse classifier response");
            return result;
        }
    };

    let classification = data
        .get("classification")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_lowercase())
        .filter(|value| is_rejection_classification(value))
        .unwrap_or_else(|| "other".to_owned());
    let key_points = data
        .get("key_points")
        .and_then(Value::as_array)
        .and_then(|points| {
            let values = points
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            (!values.is_empty()).then_some(values)
        });
    let jurisdiction = data
        .get("jurisdiction")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_owned();

    result.classification = classification;
    result.confidence = clamp_confidence(data.get("confidence"));
    result.summary = data
        .get("summary")
        .and_then(Value::as_str)
        .map(SummaryBytes::truncated)
        .unwrap_or_default();
    result.key_points = key_points;
    result.jurisdiction = jurisdiction;
    result
}

/// Select the first Go-ordered keyword fallback. The empty string means no
/// fallback matched, matching the Go helper's observable return value.
pub fn select_fallback_template(message: &str) -> &'static str {
    let message = message.to_lowercase();
    for (keywords, key) in [
        (
            [
                "address",
                "old address",
                "previous address",
                "current address",
            ]
            .as_slice(),
            "address_mismatch",
        ),
        (
            [
                "identity",
                "verify",
                "verification",
                "id",
                "passport",
                "driver",
            ]
            .as_slice(),
            "identity_challenged",
        ),
        (
            ["ccpa", "california", "section 1798"].as_slice(),
            "ccpa_identity_challenged",
        ),
    ] {
        if keywords.iter().any(|keyword| message.contains(keyword)) {
            return key;
        }
    }
    ""
}

fn parse_object(response: &str) -> Result<Map<String, Value>, ()> {
    match serde_json::from_str::<Value>(&strip_json_code_fence(response)) {
        Ok(Value::Object(data)) => Ok(data),
        // encoding/json unmarshals JSON null into a nil map without an error;
        // subsequent lookups therefore take the same zero/default branches.
        Ok(Value::Null) => Ok(Map::new()),
        Ok(_) | Err(_) => Err(()),
    }
}

fn strip_json_code_fence(response: &str) -> String {
    let mut text = response.trim().to_owned();
    if !text.starts_with("```") {
        return text;
    }
    text = text[3..].trim().to_owned();
    if text.to_lowercase().starts_with("json") {
        text = text[4..].trim().to_owned();
    }
    text = text.strip_suffix("```").unwrap_or(&text).trim().to_owned();
    text
}

fn classification_event(classification: &str) -> Option<&'static str> {
    Some(match classification {
        "ack" => "ACK",
        "confirmed" => "CONFIRMED",
        "rejected" => "REJECTED_FINAL",
        "verification" => "VERIFICATION_REQUESTED",
        "human_required" | "unclear" => "HUMAN_ACTION_REQUIRED",
        "autoresponder" => "AUTORESPONDER",
        "bounce" => "BOUNCE",
        _ => return None,
    })
}

fn is_rejection_classification(classification: &str) -> bool {
    matches!(
        classification,
        "address_mismatch" | "identity_challenged" | "ccpa_identity_challenged" | "other"
    )
}

fn clamp_confidence(value: Option<&Value>) -> f64 {
    let confidence = value.and_then(Value::as_f64).unwrap_or(0.0);
    confidence.clamp(0.0, 1.0)
}

fn sorted_fields(fields: &Map<String, Value>) -> BTreeMap<String, Value> {
    fields
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Match Go's `utf8.DecodeRuneInString` behavior used by `encoding/json`:
/// preserve valid spans and consume exactly one byte for each decode error.
pub(crate) fn go_json_lossy(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len());
    let mut offset = 0;
    while offset < bytes.len() {
        match std::str::from_utf8(&bytes[offset..]) {
            Ok(valid) => {
                output.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    output.push_str(
                        std::str::from_utf8(&bytes[offset..offset + valid_up_to])
                            .expect("valid UTF-8 prefix reported by from_utf8"),
                    );
                    offset += valid_up_to;
                }
                output.push('\u{FFFD}');
                offset += 1;
            }
        }
    }
    output
}
