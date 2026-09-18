//! Byte-compatible Go prompt builders for the triage LLM seam.
//!
//! The Go builders count bytes, not Unicode scalar values. A cut can therefore
//! retain an incomplete UTF-8 sequence. `PromptBytes` keeps those bytes until
//! the JSON boundary, where serialization replaces invalid bytes with U+FFFD,
//! matching `encoding/json`.

use crate::redaction::{RedactionProfile, redact_bytes};
use crate::triage_contract::go_json_lossy;

const ORIGINAL_SNIPPET_LIMIT: usize = 500;
const REPLY_BODY_LIMIT: usize = 2_000;
const BROKER_MESSAGE_LIMIT: usize = 3_000;

/// A prompt's exact bytes before JSON serialization.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromptBytes(Vec<u8>);

impl PromptBytes {
    /// Borrow the exact bytes retained by the Go-compatible builder.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consume the prompt and return its exact retained bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Return the Go-compatible JSON string value before JSON escaping.
    pub fn wire_string(&self) -> String {
        go_json_lossy(&self.0)
    }

    /// Return the complete Go-compatible JSON string wire value.
    ///
    /// `encoding/json` HTML-escapes five additional characters after replacing
    /// invalid UTF-8. The existing serde implementation intentionally does not
    /// do that, so this path is explicit at the Go wire boundary.
    pub fn wire_json(&self) -> Vec<u8> {
        go_json_quote_html(&self.wire_string())
    }
}

impl AsRef<[u8]> for PromptBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

/// Build the classifier prompt with the Go byte limits and section layout.
pub fn build_user_prompt(
    broker_name: &str,
    broker_website: &str,
    original_subject: &str,
    original_snippet: &str,
    reply_subject: &str,
    reply_body: &str,
    profile: Option<&RedactionProfile>,
) -> PromptBytes {
    let mut parts = Vec::with_capacity(5);
    parts.push(format!("Broker: {broker_name} ({broker_website})").into_bytes());
    if !original_subject.is_empty() {
        parts.push(format!("Original request subject: {original_subject}").into_bytes());
    }
    if !original_snippet.is_empty() {
        let mut part = b"Original request body (truncated):\n".to_vec();
        part.extend_from_slice(cut_bytes(
            original_snippet.as_bytes(),
            ORIGINAL_SNIPPET_LIMIT,
        ));
        parts.push(part);
    }
    let mut subject = b"\nReply subject: ".to_vec();
    subject.extend_from_slice(&redact_compat(reply_subject.as_bytes(), profile));
    parts.push(subject);

    let mut body = b"Reply body:\n".to_vec();
    body.extend_from_slice(&redact_compat(
        cut_bytes(reply_body.as_bytes(), REPLY_BODY_LIMIT),
        profile,
    ));
    parts.push(body);

    PromptBytes(join_parts(parts))
}

/// Build the rejection-classifier prompt with the Go byte limits and layout.
pub fn build_rebuttal_classifier_prompt(
    broker_name: &str,
    message: &str,
    original_template: &str,
    profile: Option<&RedactionProfile>,
) -> PromptBytes {
    let mut parts = Vec::with_capacity(3);
    let broker_name = if broker_name.is_empty() {
        "Unknown"
    } else {
        broker_name
    };
    parts.push(format!("Broker: {broker_name}").into_bytes());
    if !original_template.is_empty() {
        let mut original = b"Original request (truncated):\n".to_vec();
        original.extend_from_slice(cut_bytes(
            original_template.as_bytes(),
            ORIGINAL_SNIPPET_LIMIT,
        ));
        parts.push(original);
    }

    let mut response = b"\nBroker response:\n".to_vec();
    response.extend_from_slice(&redact_compat(
        cut_bytes(message.as_bytes(), BROKER_MESSAGE_LIMIT),
        profile,
    ));
    parts.push(response);

    PromptBytes(join_parts(parts))
}

fn cut_bytes(value: &[u8], limit: usize) -> &[u8] {
    &value[..value.len().min(limit)]
}

fn redact_compat(value: &[u8], profile: Option<&RedactionProfile>) -> Vec<u8> {
    redact_bytes(value, profile).unwrap_or_default()
}

fn join_parts(parts: Vec<Vec<u8>>) -> Vec<u8> {
    let separator = b"\n\n";
    let capacity = parts.iter().map(Vec::len).sum::<usize>()
        + separator
            .len()
            .saturating_mul(parts.len().saturating_sub(1));
    let mut output = Vec::with_capacity(capacity);
    for (index, part) in parts.into_iter().enumerate() {
        if index != 0 {
            output.extend_from_slice(separator);
        }
        output.extend_from_slice(&part);
    }
    output
}

fn go_json_quote_html(value: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(value.len() + 2);
    output.push(b'"');
    for character in value.chars() {
        match character {
            '"' => output.extend_from_slice(b"\\\""),
            '\\' => output.extend_from_slice(b"\\\\"),
            '\u{08}' => output.extend_from_slice(b"\\b"),
            '\u{0c}' => output.extend_from_slice(b"\\f"),
            '\n' => output.extend_from_slice(b"\\n"),
            '\r' => output.extend_from_slice(b"\\r"),
            '\t' => output.extend_from_slice(b"\\t"),
            character if character <= '\u{1f}' => {
                let value = character as u32;
                output.extend_from_slice(b"\\u00");
                output.push(hex_digit((value >> 4) as u8));
                output.push(hex_digit(value as u8));
            }
            '&' => output.extend_from_slice(b"\\u0026"),
            '<' => output.extend_from_slice(b"\\u003c"),
            '>' => output.extend_from_slice(b"\\u003e"),
            '\u{2028}' => output.extend_from_slice(b"\\u2028"),
            '\u{2029}' => output.extend_from_slice(b"\\u2029"),
            character => {
                let mut bytes = [0; 4];
                output.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
            }
        }
    }
    output.push(b'"');
    output
}

fn hex_digit(value: u8) -> u8 {
    match value & 0x0f {
        0..=9 => b'0' + (value & 0x0f),
        value => b'a' + (value - 10),
    }
}
