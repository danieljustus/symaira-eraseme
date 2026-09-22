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
    /// Reading stdin or writing stdout failed.
    Io(String),
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamError::MalformedValue(position) => {
                write!(f, "malformed JSON value at byte {position}")
            }
            StreamError::UnparsableValue(position) => {
                write!(f, "unparsable JSON value at byte {position}")
            }
            StreamError::Io(message) => write!(f, "{message}"),
        }
    }
}

/// Answers every JSON value in `input`, appending one response per request to
/// `output`. Notifications contribute nothing, and a malformed stream aborts
/// without a fabricated response for the offending value.
///
/// ponytail: the whole input is buffered instead of read incrementally; the
/// streaming reader arrives with the real `serve` command, which is what decides
/// when a value is complete on a live pipe.
pub(crate) fn serve_stream(
    input: &[u8],
    output: &mut Vec<u8>,
    handler: &dyn super::handler::ToolHandler,
) -> Result<(), StreamError> {
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
        match initialize(&input[start..end], handler) {
            InitializeOutcome::Response(bytes) => output.extend_from_slice(&bytes),
            InitializeOutcome::Notification => {}
            InitializeOutcome::ParseError => return Err(StreamError::UnparsableValue(start)),
        }
        index = end;
    }
}

/// Go's `ServeStdio` against a live pipe: each value is answered as soon as it
/// completes, because an MCP client waits for the initialize response before
/// it sends anything else. Clean EOF returns `Ok(())` (Go's `io.EOF` → nil);
/// a stream cut mid-value aborts.
///
/// ponytail: `skip_json_value` cannot tell a truncated value from a
/// syntactically invalid one, so a malformed value mid-stream waits for the
/// next read where Go's decoder errors immediately, and the abort text is ours
/// rather than `encoding/json`'s. Neither path is recorded in the corpus.
/// Upgrade path: port the decoder's eager syntax-error detection and text.
pub(crate) fn serve_stdio(
    input: &mut dyn std::io::BufRead,
    output: &mut dyn std::io::Write,
    handler: &dyn super::handler::ToolHandler,
) -> Result<(), StreamError> {
    fn map_io(error: std::io::Error) -> StreamError {
        StreamError::Io(error.to_string())
    }

    let mut buffer: Vec<u8> = Vec::new();
    let mut position = 0usize;
    loop {
        skip_whitespace(&buffer, &mut position);
        if position < buffer.len()
            && let Some(end) = skip_json_value(&buffer, position)
        {
            match initialize(&buffer[position..end], handler) {
                InitializeOutcome::Response(bytes) => {
                    output.write_all(&bytes).map_err(map_io)?;
                    output.flush().map_err(map_io)?;
                }
                InitializeOutcome::Notification => {}
                InitializeOutcome::ParseError => {
                    return Err(StreamError::UnparsableValue(position));
                }
            }
            buffer.drain(..end);
            position = 0;
            continue;
        }
        let more = input.fill_buf().map_err(map_io)?;
        if more.is_empty() {
            return if position >= buffer.len() {
                Ok(())
            } else {
                Err(StreamError::MalformedValue(position))
            };
        }
        buffer.extend_from_slice(more);
        let length = more.len();
        input.consume(length);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::handler::test_support::no_backend_handler;
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

    /// Every recorded stream case must behave the same live (answered per
    /// value as bytes arrive) as buffered: identical outputs, identical
    /// abort-or-success verdict. This is the differential for `serve_stdio`
    /// against the fixture-verified `serve_stream`.
    #[test]
    fn serve_stdio_matches_the_buffered_stream_for_every_fixture_case() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/mcp-contract/mcp-stream/cases.json"
        ))
        .expect("stream fixture");
        assert_eq!(fixture.cases.len(), 13, "fixture case count changed");
        for case in fixture.cases {
            let input = case
                .request_b64
                .as_deref()
                .map(decode_base64)
                .unwrap_or_default();
            let handler = no_backend_handler();
            let mut buffered = Vec::new();
            let buffered_result = serve_stream(&input, &mut buffered, &handler);
            let mut live = Vec::new();
            let live_result = serve_stdio(&mut std::io::Cursor::new(input), &mut live, &handler);
            assert_eq!(
                live_result.is_ok(),
                buffered_result.is_ok(),
                "{}",
                case.name
            );
            assert_eq!(live, buffered, "{}", case.name);
        }
    }

    /// A notification has no id: nothing is written, and the verdict matches
    /// the buffered stream.
    #[test]
    fn serve_stdio_emits_nothing_for_a_notification() {
        let handler = no_backend_handler();
        let input = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let mut live = Vec::new();
        let live_result = serve_stdio(&mut std::io::Cursor::new(&input[..]), &mut live, &handler);
        let mut buffered = Vec::new();
        let buffered_result = serve_stream(&input[..], &mut buffered, &handler);
        assert_eq!(live_result.is_ok(), buffered_result.is_ok());
        assert_eq!(live, buffered);
    }

    /// EOF inside a value aborts without a fabricated response (Go's
    /// `io.ErrUnexpectedEOF` direction, our wording — see `serve_stdio`).
    #[test]
    fn serve_stdio_reports_a_value_cut_off_mid_stream() {
        let handler = no_backend_handler();
        let mut output = Vec::new();
        let truncated: &[u8] = br#"{"jsonrpc":"2.0","#;
        let error = serve_stdio(&mut std::io::Cursor::new(truncated), &mut output, &handler)
            .expect_err("truncated stream must abort");
        assert_eq!(error, StreamError::MalformedValue(0));
        assert!(output.is_empty());
    }

    #[test]
    fn stream_error_text_names_position_and_io_cause() {
        assert_eq!(
            StreamError::MalformedValue(3).to_string(),
            "malformed JSON value at byte 3"
        );
        assert_eq!(
            StreamError::UnparsableValue(7).to_string(),
            "unparsable JSON value at byte 7"
        );
        assert_eq!(
            StreamError::Io("broken pipe".to_owned()).to_string(),
            "broken pipe"
        );
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
            let result = serve_stream(&input, &mut output, &no_backend_handler());
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
            &no_backend_handler(),
        )
        .expect("stream is well formed");
        let text = String::from_utf8(output).expect("responses are UTF-8");
        assert_eq!(text.matches("\"jsonrpc\":\"2.0\"").count(), 2, "{text}");
        assert!(text.ends_with("}\n"), "{text}");
    }

    #[test]
    fn a_truncated_stream_reports_the_offset_and_writes_nothing_extra() {
        let mut output = Vec::new();
        let error =
            serve_stream(br#"{"jsonrpc":"2.0","#, &mut output, &no_backend_handler()).unwrap_err();
        assert_eq!(error, StreamError::MalformedValue(0));
        assert!(output.is_empty());
    }
}
