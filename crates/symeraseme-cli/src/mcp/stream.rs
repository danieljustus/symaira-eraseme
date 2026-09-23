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

use super::protocol::{InitializeOutcome, initialize, scan_json_value, skip_whitespace};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StreamError {
    /// Go's decoder reached EOF inside an otherwise valid JSON value.
    UnexpectedEof,
    /// Go's decoder rejected a byte before EOF.
    Syntax(String),
    /// The scanner accepted a value the shared protocol could not parse. This
    /// is an internal inconsistency; failing loudly beats dropping a request.
    UnparsableValue(usize),
    /// Reading stdin or writing stdout failed.
    Io(String),
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamError::UnexpectedEof => write!(f, "unexpected EOF"),
            StreamError::Syntax(message) => write!(f, "{message}"),
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
        let end = match scan_json_value(input, start, false) {
            Ok(Some(end)) => end,
            Err(byte) => return Err(max_depth_error(byte)),
            Ok(None) => {
                return Err(syntax_error(&input[start..]).unwrap_or(StreamError::UnexpectedEof));
            }
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
        if position < buffer.len() {
            let end = match scan_json_value(&buffer, position, false) {
                Ok(Some(end)) => Some(end),
                Err(byte) => return Err(max_depth_error(byte)),
                Ok(None) => None,
            };
            if let Some(end) = end {
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
        }
        if position < buffer.len()
            && let Some(error) = syntax_error(&buffer[position..])
        {
            return Err(error);
        }
        let more = input.fill_buf().map_err(map_io)?;
        if more.is_empty() {
            return if position >= buffer.len() {
                Ok(())
            } else {
                Err(StreamError::UnexpectedEof)
            };
        }
        buffer.extend_from_slice(more);
        let length = more.len();
        input.consume(length);
    }
}

fn max_depth_error(byte: u8) -> StreamError {
    StreamError::Syntax(format!(
        "invalid character '{}' exceeded max depth",
        go_quoted_byte(byte)
    ))
}

fn go_quoted_byte(byte: u8) -> String {
    match byte {
        b'\\' => "\\\\".to_owned(),
        b'\'' => "\\'".to_owned(),
        b'\t' => "\\t".to_owned(),
        b'\n' => "\\n".to_owned(),
        b'\r' => "\\r".to_owned(),
        0x0b => "\\v".to_owned(),
        b'\x0c' => "\\f".to_owned(),
        b'\x08' => "\\b".to_owned(),
        0..=0x1f => format!("\\x{byte:02x}"),
        _ => char::from(byte).to_string(),
    }
}

fn expects_object_key(input: &[u8], end: usize) -> bool {
    let mut containers = Vec::<(u8, bool)>::new();
    let mut in_string = false;
    let mut escaped = false;
    for byte in input.iter().take(end).copied() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => containers.push((b'{', true)),
            b'[' => containers.push((b'[', false)),
            b'}' | b']' => {
                containers.pop();
            }
            b':' => {
                if let Some((b'{', expect_key)) = containers.last_mut() {
                    *expect_key = false;
                }
            }
            b',' => {
                if let Some((b'{', expect_key)) = containers.last_mut() {
                    *expect_key = true;
                }
            }
            _ => {}
        }
    }
    matches!(containers.last(), Some((b'{', true)))
}

fn syntax_error(input: &[u8]) -> Option<StreamError> {
    // ponytail: the recorded Go error classes are matched; port Go's
    // full scanner if a wider malformed-input corpus requires exact wording.
    let error = serde_json::from_slice::<serde_json::Value>(input).err()?;
    if error.is_eof() || error.to_string().starts_with("recursion limit exceeded") {
        return None;
    }
    let (mut in_string, mut escaped) = (false, false);
    for (index, byte) in input.iter().enumerate() {
        if escaped {
            if !matches!(
                *byte,
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'u'
            ) {
                return Some(StreamError::Syntax(format!(
                    "invalid character '{}' in string escape code",
                    go_quoted_byte(*byte)
                )));
            }
            escaped = false;
        } else if in_string && *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            in_string = !in_string;
        } else if !in_string {
            let (name, literal): (&str, &[u8]) = match *byte {
                b'n' => ("null", b"null"),
                b't' => ("true", b"true"),
                b'f' => ("false", b"false"),
                _ => continue,
            };
            for (offset, expected) in literal.iter().enumerate() {
                if let Some(actual) = input.get(index + offset)
                    && actual != expected
                {
                    return Some(StreamError::Syntax(format!(
                        "invalid character '{}' in literal {name} (expecting '{}')",
                        go_quoted_byte(*actual),
                        go_quoted_byte(*expected)
                    )));
                }
            }
        }
    }
    let trimmed = input
        .iter()
        .copied()
        .filter(|byte| !matches!(*byte, b' ' | b'\t' | b'\r' | b'\n'))
        .collect::<Vec<_>>();
    if trimmed.ends_with(b",}") {
        return Some(StreamError::Syntax(
            "invalid character '}' looking for beginning of object key string".to_owned(),
        ));
    }
    let byte = input
        .get(error.column().saturating_sub(1))
        .copied()
        .or_else(|| input.last().copied())
        .unwrap_or_default();
    let error_position = error.column().saturating_sub(1);
    let preceding = input
        .iter()
        .take(error_position)
        .rev()
        .copied()
        .find(|value| !matches!(*value, b' ' | b'\t' | b'\r' | b'\n'));
    if byte == 0x0b && (preceding == Some(b'{') || expects_object_key(input, error_position)) {
        return Some(StreamError::Syntax(format!(
            "invalid character '{}' looking for beginning of object key string",
            go_quoted_byte(byte)
        )));
    }
    Some(StreamError::Syntax(format!(
        "invalid character '{}' looking for beginning of value",
        go_quoted_byte(byte)
    )))
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
        assert_eq!(error, StreamError::UnexpectedEof);
        assert!(output.is_empty());
    }

    #[test]
    fn stream_error_text_names_position_and_io_cause() {
        assert_eq!(StreamError::UnexpectedEof.to_string(), "unexpected EOF");
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
    fn a_truncated_stream_reports_unexpected_eof_and_writes_nothing_extra() {
        let mut output = Vec::new();
        let error =
            serve_stream(br#"{"jsonrpc":"2.0","#, &mut output, &no_backend_handler()).unwrap_err();
        assert_eq!(error, StreamError::UnexpectedEof);
        assert!(output.is_empty());
    }
}
