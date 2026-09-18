//! MCP stdio stream framing.
//!
//! Go's `ServeStdio` decodes consecutive JSON values from the stream instead of
//! framing on newlines, so a value may be split across lines and several values
//! may sit on one line. Measured against the Go server: newline, CRLF, adjacent
//! values and leading whitespace all behave the same, while a truncated or
//! malformed value aborts the whole stream.
//!
//! Framing reuses the byte scanner that the shared protocol already uses for
//! Go-compatible acceptance, so both paths accept exactly the same values.

use super::protocol::{InitializeOutcome, initialize, skip_json_value, skip_whitespace};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StreamError {
    /// The stream holds no further complete JSON value but is not exhausted.
    MalformedValue(usize),
    /// The scanner accepted a value the shared protocol could not parse. This
    /// is an internal inconsistency; failing loudly beats dropping a request.
    UnparsableValue(usize),
}

/// Answers every JSON value in `input`, appending one response per request to
/// `output`. Notifications contribute nothing, and a malformed stream aborts
/// without a fabricated response for the offending value.
///
/// ponytail: the whole input is buffered instead of read incrementally; the
/// streaming reader arrives with the real `serve` command, which is what decides
/// when a value is complete on a live pipe.
pub(crate) fn serve_stream(input: &[u8], output: &mut Vec<u8>) -> Result<(), StreamError> {
    let mut index = 0;
    loop {
        skip_whitespace(input, &mut index);
        if index >= input.len() {
            return Ok(());
        }
        let start = index;
        let Some(end) = skip_json_value(input, start) else {
            return Err(StreamError::MalformedValue(start));
        };
        match initialize(&input[start..end]) {
            InitializeOutcome::Response(bytes) => output.extend_from_slice(&bytes),
            InitializeOutcome::Notification => {}
            InitializeOutcome::ParseError => return Err(StreamError::UnparsableValue(start)),
        }
        index = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Case {
        name: String,
        request_b64: Option<String>,
        response: Option<String>,
        #[serde(default)]
        parse_error: bool,
    }

    #[derive(Deserialize)]
    struct Fixture {
        source_revision: String,
        cases: Vec<Case>,
    }

    fn decode_base64(input: &str) -> Vec<u8> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut output = Vec::new();
        let mut buffer = 0u32;
        let mut bits = 0u32;
        for byte in input.bytes() {
            let Some(value) = ALPHABET.iter().position(|entry| *entry == byte) else {
                break;
            };
            buffer = (buffer << 6) | value as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                output.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        }
        output
    }

    #[test]
    fn source_bound_go_stream_fixture_matches() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/mcp-contract/mcp-stream/cases.json"
        ))
        .expect("stream fixture");
        assert_eq!(
            fixture.source_revision,
            "4cdbf9be02f76f4384a0eb0c8fdbe37b3aef19f7"
        );
        assert_eq!(fixture.cases.len(), 13, "fixture case count changed");
        for case in fixture.cases {
            let input = case
                .request_b64
                .as_deref()
                .map(decode_base64)
                .unwrap_or_default();
            let mut output = Vec::new();
            let result = serve_stream(&input, &mut output);
            if case.parse_error {
                assert!(result.is_err(), "{} should have aborted", case.name);
                continue;
            }
            assert!(result.is_ok(), "{} unexpectedly aborted", case.name);
            assert_eq!(
                String::from_utf8(output).expect("responses are UTF-8"),
                case.response.unwrap_or_default(),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn every_request_gets_exactly_one_response_and_notifications_get_none() {
        let mut output = Vec::new();
        serve_stream(
            br#"{"jsonrpc":"2.0","method":"initialize"}
{"jsonrpc":"2.0","id":1,"method":"initialize"}{"jsonrpc":"2.0","id":2,"method":"initialize"}"#,
            &mut output,
        )
        .expect("stream is well formed");
        let text = String::from_utf8(output).expect("responses are UTF-8");
        assert_eq!(text.matches("\"jsonrpc\":\"2.0\"").count(), 2, "{text}");
        assert!(text.ends_with("}\n"), "{text}");
    }

    #[test]
    fn a_truncated_stream_reports_the_offset_and_writes_nothing_extra() {
        let mut output = Vec::new();
        let error = serve_stream(br#"{"jsonrpc":"2.0","#, &mut output).unwrap_err();
        assert_eq!(error, StreamError::MalformedValue(0));
        assert!(output.is_empty());
    }
}
