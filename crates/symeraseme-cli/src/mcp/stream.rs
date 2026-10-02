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
            Err(error) => return Err(StreamError::Syntax(error.0)),
            Ok(None) => return Err(StreamError::UnexpectedEof),
        };
        match initialize(&input[start..end], handler) {
            InitializeOutcome::Response(bytes) => output.extend_from_slice(&bytes),
            InitializeOutcome::Notification => {}
            InitializeOutcome::ParseError => return Err(StreamError::UnparsableValue(start)),
        }
        index = end;
    }
}

/// Go's `ServeStdio` against a live pipe: objects/arrays complete immediately;
/// top-level scalars need a following byte or EOF, as in Go's Decoder.readValue.
/// Clean EOF returns `Ok(())` (Go's `io.EOF` → nil); a cut mid-value aborts.
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
    let mut eof = false;
    loop {
        skip_whitespace(&buffer, &mut position);
        if position < buffer.len() {
            let end = match scan_json_value(&buffer, position, false) {
                Ok(Some(end))
                    if end < buffer.len() || eof || matches!(buffer[position], b'{' | b'[') =>
                {
                    Some(end)
                }
                Err(error) => return Err(StreamError::Syntax(error.0)),
                Ok(_) => None,
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
        if eof {
            return if position >= buffer.len() {
                Ok(())
            } else {
                Err(StreamError::UnexpectedEof)
            };
        }
        let more = input.fill_buf().map_err(map_io)?;
        if more.is_empty() {
            eof = true;
            continue;
        }
        buffer.extend_from_slice(more);
        let length = more.len();
        input.consume(length);
    }
}

#[cfg(test)]
fn syntax_error(input: &[u8]) -> Option<StreamError> {
    scan_json_value(input, 0, false)
        .err()
        .map(|error| StreamError::Syntax(error.0))
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

    /// Go's Decoder needs lookahead/EOF for top-level scalars, but not for
    /// objects/arrays. Observe read/write ordering directly, without sleeps.
    #[test]
    fn stdio_primitive_waits_for_lookahead_or_eof() {
        use std::cell::Cell;
        use std::io::{BufRead, Read, Write};

        struct Chunks<'a> {
            bytes: &'a [u8],
            following: &'a [u8],
            reads: &'a Cell<usize>,
        }
        impl Read for Chunks<'_> {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let bytes = self.fill_buf()?;
                let length = bytes.len().min(out.len());
                out[..length].copy_from_slice(&bytes[..length]);
                self.consume(length);
                Ok(length)
            }
        }
        impl BufRead for Chunks<'_> {
            fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
                self.reads.set(self.reads.get() + 1);
                Ok(self.bytes)
            }
            fn consume(&mut self, amount: usize) {
                self.bytes = &self.bytes[amount..];
                if self.bytes.is_empty() {
                    self.bytes = std::mem::take(&mut self.following);
                }
            }
        }
        struct ObservedOutput<'a> {
            bytes: Vec<u8>,
            reads: &'a Cell<usize>,
            minimum_reads: usize,
        }
        impl Write for ObservedOutput<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                assert_eq!(
                    self.reads.get(),
                    self.minimum_reads,
                    "dispatch must occur exactly at its lookahead/EOF boundary"
                );
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        for token in [
            b"\"x\"".as_slice(),
            b"null",
            b"true",
            b"false",
            b"1",
            b"-0",
            b"1e2",
            b"{}",
            b"[]",
        ] {
            // Go's stateEndTop returns scanEnd even for non-space lookahead:
            // the scalar response precedes rejection of the next value's byte.
            for following in [b" ".as_slice(), b"", b"x"] {
                let reads = Cell::new(0);
                let mut input = Chunks {
                    bytes: token,
                    following,
                    reads: &reads,
                };
                let minimum_reads = if matches!(token[0], b'{' | b'[') {
                    1
                } else {
                    2
                };
                let mut output = ObservedOutput {
                    bytes: Vec::new(),
                    reads: &reads,
                    minimum_reads,
                };
                let handler = no_backend_handler();
                let mut expected = Vec::new();
                let verdict = serve_stream(&[token, following].concat(), &mut expected, &handler);
                assert_eq!(serve_stdio(&mut input, &mut output, &handler), verdict);
                assert_eq!(output.bytes, expected);
            }
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

    /// Run source-generated Go diagnostics through both production stream
    /// paths, including one-byte reads and every possible offending byte.
    #[test]
    fn go_syntax_oracle_matches_buffered_and_chunked_streams() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../rust-tests/parity/oracle/mcp-stdio-mutations/cases.json"
        ))
        .unwrap();
        let mut executed = 0;
        for case in fixture["cases"].as_array().unwrap() {
            let Some(encoded) = case["input_spec"]["base64"].as_str() else {
                continue;
            };
            let input = STANDARD.decode(encoded).unwrap();
            let expected_out = STANDARD
                .decode(case["stdout_base64"].as_str().unwrap())
                .unwrap();
            let expected_err = STANDARD
                .decode(case["stderr_base64"].as_str().unwrap())
                .unwrap();
            let handler = no_backend_handler();
            for live in [false, true] {
                let mut output = Vec::new();
                let result = if live {
                    serve_stdio(
                        &mut std::io::BufReader::with_capacity(1, &input[..]),
                        &mut output,
                        &handler,
                    )
                } else {
                    serve_stream(&input, &mut output, &handler)
                };
                let error = result
                    .err()
                    .map(|e| format!("{e}\n").into_bytes())
                    .unwrap_or_default();
                assert_eq!(output, expected_out, "{} live={live}", case["name"]);
                assert_eq!(error, expected_err, "{} live={live}", case["name"]);
            }
            executed += 1;
        }
        assert_eq!(executed, 666);
    }

    /// Go's recorded syntax-error wording for the bytes the shared scanner
    /// refuses to frame.
    #[test]
    fn serve_stream_rejects_go_syntax_errors_with_recorded_wording() {
        let handler = no_backend_handler();
        let mut output = Vec::new();

        let error = serve_stream(br#"{"a":truX}"#, &mut output, &handler)
            .expect_err("bad literal must abort the stream");
        assert_eq!(
            error,
            StreamError::Syntax("invalid character 'X' in literal true (expecting 'e')".to_owned())
        );

        let error = serve_stream(br#"{"a":falze}"#, &mut output, &handler)
            .expect_err("bad literal must abort the stream");
        assert_eq!(
            error,
            StreamError::Syntax(
                "invalid character 'z' in literal false (expecting 's')".to_owned()
            )
        );

        // A valid string escape is walked without error before the frame
        // itself fails, so the escape loop must not fire.
        let error = serve_stream(br#"{"a":"\n" x}"#, &mut output, &handler)
            .expect_err("trailing garbage must abort the stream");
        assert!(matches!(error, StreamError::Syntax(_)), "{error}");
        assert!(
            error.to_string().starts_with("invalid character"),
            "{error}"
        );

        // A vertical tab where an object key belongs gets Go's key wording.
        let error = serve_stream(b"{\x0b}", &mut output, &handler)
            .expect_err("vertical tab must abort the stream");
        assert_eq!(
            error,
            StreamError::Syntax(
                "invalid character '\\v' looking for beginning of object key string".to_owned()
            )
        );
    }

    /// Nesting past Go's 10,000-value limit aborts both transports with the
    /// same recorded wording, and frames the deserializer rejects after the
    /// scanner accepted them fail loudly instead of being dropped.
    #[test]
    fn depth_overflow_and_unreadable_frames_fail_loudly() {
        let handler = no_backend_handler();

        // Objects frame in value position only, so the chain repeats the
        // `{"a":` opener; arrays count from their second element.
        let deep_object = "{\"a\":".repeat(10_001);
        let mut output = Vec::new();
        let error = serve_stream(deep_object.as_bytes(), &mut output, &handler)
            .expect_err("10001 nested objects exceed Go's limit");
        assert_eq!(
            error,
            StreamError::Syntax("invalid character '{' exceeded max depth".to_owned())
        );

        let deep_array = "[".repeat(10_001);
        let mut output = Vec::new();
        let error = serve_stream(deep_array.as_bytes(), &mut output, &handler)
            .expect_err("10001 nested arrays exceed Go's limit");
        assert_eq!(
            error,
            StreamError::Syntax("invalid character '[' exceeded max depth".to_owned())
        );

        let mut reader = std::io::BufReader::new(deep_object.as_bytes());
        let mut sink = Vec::new();
        let error = serve_stdio(&mut reader, &mut sink, &handler)
            .expect_err("stdio depth overflow must abort too");
        assert_eq!(
            error,
            StreamError::Syntax("invalid character '{' exceeded max depth".to_owned())
        );

        // 200 nested arrays: the scanner (limit 10 000) accepts them, the
        // catalogue's serde_json (limit 128) rejects them.
        let deep = format!("{}1{}", "[".repeat(200), "]".repeat(200));
        let request = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"redact_file","arguments":{{"deep":{deep}}}}}}}"#
        );
        let mut output = Vec::new();
        let error = serve_stream(request.as_bytes(), &mut output, &handler)
            .expect_err("an accepted-but-unreadable frame fails loudly");
        assert_eq!(error, StreamError::UnparsableValue(0));

        let mut reader = std::io::BufReader::new(request.as_bytes());
        let mut sink = Vec::new();
        let error = serve_stdio(&mut reader, &mut sink, &handler)
            .expect_err("the stdio path fails loudly too");
        assert!(
            matches!(error, StreamError::UnparsableValue(_)),
            "expected UnparsableValue, got {error}"
        );
    }

    /// Every value the scanner refuses to frame aborts the stream; a value cut
    /// off inside the container is Go's EOF, the rest are syntax errors.
    #[test]
    fn serve_stream_rejects_truncated_and_misaligned_values() {
        let handler = no_backend_handler();

        // Missing value after the opening bracket: Go reports EOF.
        let mut output = Vec::new();
        let error = serve_stream(b"[", &mut output, &handler)
            .expect_err("an open array with no value must abort");
        assert_eq!(error, StreamError::UnexpectedEof);

        let misaligned: [&[u8]; 3] = [
            br#"{"a" 1}"#,   // key not followed by a colon
            br#"{"a":1 2}"#, // garbage after an object value
            b"[1 2]",        // garbage after an array value
        ];
        for input in misaligned {
            let mut output = Vec::new();
            let error = match serve_stream(input, &mut output, &handler) {
                Ok(()) => panic!("{} must abort", String::from_utf8_lossy(input)),
                Err(error) => error,
            };
            assert!(
                matches!(error, StreamError::Syntax(_)),
                "expected a syntax error for {}, got {error}",
                String::from_utf8_lossy(input)
            );
        }

        // Number tokens cut off before their digits are complete: the scanner
        // refuses to frame them, and the stream aborts with Go's EOF.
        for input in [&b"1."[..], b"1e"] {
            let mut output = Vec::new();
            let error = match serve_stream(input, &mut output, &handler) {
                Ok(()) => panic!("{:?} must abort", String::from_utf8_lossy(input)),
                Err(error) => error,
            };
            assert_eq!(
                error,
                StreamError::UnexpectedEof,
                "for {:?}",
                String::from_utf8_lossy(input)
            );
        }
    }

    struct FailingWriter;

    impl std::io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("stdout is gone"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FailingReader;

    impl std::io::Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("stdin is gone"))
        }
    }

    impl std::io::BufRead for FailingReader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            Err(std::io::Error::other("stdin is gone"))
        }
        fn consume(&mut self, _amt: usize) {}
    }

    /// A failing stdout or stdin aborts the stdio transport with the I/O
    /// message instead of a fabricated response.
    #[test]
    fn serve_stdio_reports_read_and_write_failures_loudly() {
        let handler = no_backend_handler();
        let request = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let mut reader = std::io::BufReader::new(&request[..]);
        let error = serve_stdio(&mut reader, &mut FailingWriter, &handler)
            .expect_err("stdout failure must abort");
        assert_eq!(error, StreamError::Io("stdout is gone".to_owned()));

        let mut reader = FailingReader;
        let mut sink = Vec::new();
        let error =
            serve_stdio(&mut reader, &mut sink, &handler).expect_err("stdin failure must abort");
        assert_eq!(error, StreamError::Io("stdin is gone".to_owned()));
    }

    /// Go's `encoding/json` wording for each malformed-value class the
    /// scanner distinguishes (keys, separators, escapes, literals, nesting).
    #[test]
    fn syntax_error_matches_go_wording_per_error_class() {
        let cases: &[(&[u8], &str)] = &[
            (br#"{"a" 1}"#, "invalid character '1' after object key"),
            (br#"{"a" "b"}"#, "invalid character '\"' after object key"),
            (
                br#"{"a":1,"b" 2}"#,
                "invalid character '2' after object key",
            ),
            (br#"{"a:b" 1}"#, "invalid character '1' after object key"),
            (br#"{"a\"" 1}"#, "invalid character '1' after object key"),
            (
                br#"{"a":"x" 1}"#,
                "invalid character '1' after object key:value pair",
            ),
            (
                br#"{"a":"b":1}"#,
                "invalid character ':' after object key:value pair",
            ),
            (
                br#"{"a":"x","b":"y" 1}"#,
                "invalid character '1' after object key:value pair",
            ),
            (
                br#"{"a":{"b":"c" 1}}"#,
                "invalid character '1' after object key:value pair",
            ),
            (
                br#"{"a":"x,y" 1}"#,
                "invalid character '1' after object key:value pair",
            ),
            (
                br#"{"a":"}" x}"#,
                "invalid character 'x' after object key:value pair",
            ),
            (
                br#"{"a":"\"" x}"#,
                "invalid character 'x' after object key:value pair",
            ),
            (
                br#"{"a":[1] x}"#,
                "invalid character 'x' after object key:value pair",
            ),
            (
                br#"{"a":{} x}"#,
                "invalid character 'x' after object key:value pair",
            ),
            (
                br#"{"a":1 2}"#,
                "invalid character '2' after object key:value pair",
            ),
            (
                br#"{ "a" : "b" x }"#,
                "invalid character 'x' after object key:value pair",
            ),
            (
                br#"{"a":"\q"}"#,
                "invalid character 'q' in string escape code",
            ),
            (
                br#"{"a":nulx}"#,
                "invalid character 'x' in literal null (expecting 'l')",
            ),
            (
                br#"[nul]"#,
                "invalid character ']' in literal null (expecting 'l')",
            ),
            (
                br#"[1,nux]"#,
                "invalid character 'x' in literal null (expecting 'l')",
            ),
            (
                br#"[tx]"#,
                "invalid character 'x' in literal true (expecting 'r')",
            ),
            (
                br#"[fx]"#,
                "invalid character 'x' in literal false (expecting 'a')",
            ),
            // A literal letter glued to a value is not a literal start.
            (
                br#"{"a":1t}"#,
                "invalid character 't' after object key:value pair",
            ),
            // Brackets inside a string do not open or close containers.
            (
                br#"{"a":"]" , x}"#,
                "invalid character 'x' looking for beginning of object key string",
            ),
            (
                br#"{1}"#,
                "invalid character '1' looking for beginning of object key string",
            ),
            (
                br#"{"a":1,2}"#,
                "invalid character '2' looking for beginning of object key string",
            ),
            (
                br#"{"a":1,}"#,
                "invalid character '}' looking for beginning of object key string",
            ),
            (
                br#"{"a":1 , }"#,
                "invalid character '}' looking for beginning of object key string",
            ),
            (
                br#"{"a":"b",:1}"#,
                "invalid character ':' looking for beginning of object key string",
            ),
            (
                br#"{,"a":1}"#,
                "invalid character ',' looking for beginning of object key string",
            ),
            (
                br#"[{]"#,
                "invalid character ']' looking for beginning of object key string",
            ),
            (
                b"{\"a\":1,\x0b}",
                "invalid character '\\v' looking for beginning of object key string",
            ),
            (
                b"{\x0b}",
                "invalid character '\\v' looking for beginning of object key string",
            ),
            (
                b"[\x0b]",
                "invalid character '\\v' looking for beginning of value",
            ),
            (
                b"{\"a\":\x0b}",
                "invalid character '\\v' looking for beginning of value",
            ),
            (
                b"[1,\x0b]",
                "invalid character '\\v' looking for beginning of value",
            ),
            (
                br#"{"a":}"#,
                "invalid character '}' looking for beginning of value",
            ),
            (
                br#"{"a":[1,x]}"#,
                "invalid character 'x' looking for beginning of value",
            ),
            (
                br#"[{"a":1},x]"#,
                "invalid character 'x' looking for beginning of value",
            ),
            (
                br#"{"a":[}"#,
                "invalid character '}' looking for beginning of value",
            ),
            (
                br#"{"a"::1}"#,
                "invalid character ':' looking for beginning of value",
            ),
            (
                br#"[:]"#,
                "invalid character ':' looking for beginning of value",
            ),
            (
                br#"[1,]"#,
                "invalid character ']' looking for beginning of value",
            ),
            (b"x", "invalid character 'x' looking for beginning of value"),
            (b"]", "invalid character ']' looking for beginning of value"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                syntax_error(input),
                Some(StreamError::Syntax((*expected).to_owned())),
                "{}",
                String::from_utf8_lossy(input)
            );
        }
        // EOF inside a value is not a syntax error.
        assert_eq!(syntax_error(br#"{"a":"#), None);
    }

    /// The live transport reports the syntax error itself, not a later EOF.
    #[test]
    fn serve_stdio_reports_syntax_errors_before_eof() {
        let mut sink = Vec::new();
        let error = serve_stdio(
            &mut std::io::Cursor::new(&br#"{"a":truX}"#[..]),
            &mut sink,
            &no_backend_handler(),
        )
        .expect_err("bad literal must abort");
        assert_eq!(
            error,
            StreamError::Syntax("invalid character 'X' in literal true (expecting 'e')".to_owned())
        );
    }
}
