//! Transport-independent MCP initialize protocol contract.
//!
//! This deliberately stops at one JSON-RPC request and response. HTTP, stdio,
//! tool dispatch, and server lifecycle remain outside this slice.

use serde::Serialize;
use serde_json::Number;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InitializeOutcome {
    Response(Vec<u8>),
    Notification,
    ParseError,
}

enum RequestParse {
    State(RequestState),
    ParseError,
}

enum IdState {
    Valid(GoId),
    Invalid,
}

enum ParamsState {
    Missing,
    Null,
    Object,
    Other,
}

struct RequestState {
    id_present: bool,
    id: IdState,
    request_error: bool,
    valid_request: bool,
    method: Option<String>,
    params: ParamsState,
}

#[derive(Serialize)]
struct Response<'a> {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<InitializeResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError<'a>>,
    #[serde(skip)]
    id: GoId,
}

#[derive(Debug, PartialEq, Eq)]
enum GoId {
    Null,
    String(String),
    Number(String),
}

#[derive(Serialize)]
struct RpcError<'a> {
    code: i32,
    message: &'a str,
}

#[derive(Serialize)]
struct InitializeResult {
    capabilities: Capabilities,
    #[serde(rename = "protocolVersion")]
    protocol_version: &'static str,
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
}

#[derive(Serialize)]
struct Capabilities {
    tools: EmptyObject,
}

#[derive(Serialize)]
struct ServerInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct EmptyObject {}

pub(crate) fn initialize(raw: &[u8]) -> InitializeOutcome {
    let value = match parse_request(raw) {
        RequestParse::State(state) => state,
        RequestParse::ParseError => return InitializeOutcome::ParseError,
    };
    let RequestState {
        id_present,
        id,
        request_error,
        valid_request,
        method,
        params,
    } = value;
    let id = match id {
        IdState::Valid(id) => id,
        IdState::Invalid => {
            return InitializeOutcome::Response(error_response(
                -32600,
                "invalid request",
                GoId::Null,
            ));
        }
    };
    if request_error {
        return InitializeOutcome::Response(error_response(-32600, "invalid request", GoId::Null));
    }
    if !valid_request {
        return InitializeOutcome::Response(error_response(-32600, "invalid request", id));
    }

    let notification = !id_present;
    let method = method.as_deref().unwrap_or_default();
    if method != "initialize" {
        if matches!(method, "tools/list" | "list_tools") {
            return match super::tools_list::tools_list(raw) {
                super::tools_list::ToolsListOutcome::Response(bytes) => {
                    InitializeOutcome::Response(bytes)
                }
                super::tools_list::ToolsListOutcome::Notification => {
                    InitializeOutcome::Notification
                }
                super::tools_list::ToolsListOutcome::ParseError => InitializeOutcome::ParseError,
            };
        }
        if matches!(method, "tools/call") {
            // Go dispatches a notification and then drops every response,
            // including errors, so this check comes before any rejection.
            if notification {
                return InitializeOutcome::Notification;
            }
            return match super::tools_call::tools_call(raw) {
                super::tools_call::ToolsCallOutcome::Response(bytes) => {
                    InitializeOutcome::Response(bytes)
                }
                super::tools_call::ToolsCallOutcome::Notification => {
                    InitializeOutcome::Notification
                }
                super::tools_call::ToolsCallOutcome::ParseError => InitializeOutcome::ParseError,
            };
        }
        return if notification {
            InitializeOutcome::Notification
        } else {
            InitializeOutcome::Response(error_response(-32601, "method not found", id))
        };
    }

    if matches!(params, ParamsState::Other) {
        return if notification {
            InitializeOutcome::Notification
        } else {
            InitializeOutcome::Response(error_response(-32602, "invalid params", id))
        };
    }

    if notification {
        InitializeOutcome::Notification
    } else {
        let result = InitializeResult {
            capabilities: Capabilities {
                tools: EmptyObject {},
            },
            protocol_version: "2025-06-18",
            server_info: ServerInfo {
                name: "symeraseme",
                version: "dev",
            },
        };
        InitializeOutcome::Response(encode_response(&Response {
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            id,
        }))
    }
}

fn error_response(code: i32, message: &'static str, id: GoId) -> Vec<u8> {
    encode_response(&Response {
        jsonrpc: "2.0",
        result: None,
        error: Some(RpcError { code, message }),
        id,
    })
}

fn encode_response(response: &Response<'_>) -> Vec<u8> {
    let mut output = serde_json::to_vec(response).expect("JSON-RPC response is serializable");
    output.pop();
    output.extend_from_slice(b",\"id\":");
    append_id(&mut output, &response.id);
    output.push(b'}');
    output.push(b'\n');
    output
}

fn append_id(output: &mut Vec<u8>, id: &GoId) {
    match id {
        GoId::Null => output.extend_from_slice(b"null"),
        GoId::String(value) => output.extend_from_slice(&go_json_string(value)),
        GoId::Number(value) => output.extend_from_slice(value.as_bytes()),
    }
}

fn go_json_string(value: &str) -> Vec<u8> {
    let encoded = serde_json::to_vec(value).expect("JSON-RPC string ID is serializable");
    let mut output = Vec::with_capacity(encoded.len());
    let mut index = 0;
    while index < encoded.len() {
        if encoded[index..].starts_with(&[0xe2, 0x80, 0xa8]) {
            output.extend_from_slice(b"\\u2028");
            index += 3;
        } else if encoded[index..].starts_with(&[0xe2, 0x80, 0xa9]) {
            output.extend_from_slice(b"\\u2029");
            index += 3;
        } else {
            match encoded[index] {
                b'<' => output.extend_from_slice(b"\\u003c"),
                b'>' => output.extend_from_slice(b"\\u003e"),
                b'&' => output.extend_from_slice(b"\\u0026"),
                byte => output.push(byte),
            }
            index += 1;
        }
    }
    output
}

fn normalize_finite_number(value: f64, negative_zero: bool) -> Option<GoId> {
    if !value.is_finite() {
        return None;
    }
    if negative_zero {
        return Some(GoId::Number("-0".to_owned()));
    }
    const EXACT_INTEGER_LIMIT: f64 = 9_007_199_254_740_992.0;
    if value.fract() == 0.0 && value.abs() <= EXACT_INTEGER_LIMIT {
        return Some(GoId::Number((value as i64).to_string()));
    }
    let serialized = Number::from_f64(value)?.to_string();
    Some(GoId::Number(go_float_text(&serialized)))
}

fn parse_request(raw: &[u8]) -> RequestParse {
    let mut index = 0;
    skip_whitespace(raw, &mut index);
    if raw.get(index) == Some(&b'{') {
        return recover_request_around_large_numbers(raw);
    }
    let Some(end) = skip_json_value(raw, index) else {
        return RequestParse::ParseError;
    };
    index = end;
    skip_whitespace(raw, &mut index);
    if index != raw.len() {
        return RequestParse::ParseError;
    }
    RequestParse::State(invalid_request_state())
}

fn invalid_request_state() -> RequestState {
    RequestState {
        id_present: false,
        id: IdState::Valid(GoId::Null),
        request_error: false,
        valid_request: false,
        method: None,
        params: ParamsState::Missing,
    }
}

struct RawField<'a> {
    name: String,
    value: &'a [u8],
}

fn recover_request_around_large_numbers(raw: &[u8]) -> RequestParse {
    let Some(fields) = scan_object_fields(raw) else {
        return RequestParse::ParseError;
    };
    let mut id_present = false;
    let mut id = IdState::Valid(GoId::Null);
    let mut jsonrpc = None;
    let mut method = None;
    let mut request_error = false;
    let mut params = ParamsState::Missing;
    for field in fields {
        if go_field_name_matches(&field.name, "jsonrpc") {
            if field.value != b"null" {
                let (value, error) = classify_raw_string(field.value);
                jsonrpc = value;
                request_error |= error;
            }
        } else if go_field_name_matches(&field.name, "method") {
            if field.value != b"null" {
                let (value, error) = classify_raw_string(field.value);
                method = value;
                request_error |= error;
            }
        } else if go_field_name_matches(&field.name, "id") {
            id_present = true;
            id = classify_raw_id(field.value);
        } else if go_field_name_matches(&field.name, "params") {
            params = classify_raw_params(field.value);
        }
    }
    RequestParse::State(RequestState {
        id_present,
        id,
        request_error,
        valid_request: jsonrpc.as_deref() == Some("2.0")
            && method.as_deref().is_some_and(|value| !value.is_empty()),
        method,
        params,
    })
}

fn go_field_name_matches(actual: &str, expected: &str) -> bool {
    let mut actual_chars = actual.chars();
    let matches = expected.chars().all(|expected_character| {
        let Some(actual_character) = actual_chars.next() else {
            return false;
        };
        actual_character.eq_ignore_ascii_case(&expected_character)
            || matches!(
                (actual_character, expected_character),
                ('ſ', 's') | ('s', 'ſ') | ('K', 'k') | ('k', 'K')
            )
    });
    matches && actual_chars.next().is_none()
}

fn classify_raw_string(raw: &[u8]) -> (Option<String>, bool) {
    if raw == b"null" {
        return (None, false);
    }
    if raw.first() != Some(&b'"') {
        return (None, true);
    }
    match decode_json_string(raw) {
        Some(value) => (Some(value), false),
        None => (None, true),
    }
}

fn classify_raw_id(raw: &[u8]) -> IdState {
    if raw == b"null" {
        return IdState::Valid(GoId::Null);
    }
    if raw.first() == Some(&b'"') {
        return decode_json_string(raw).map_or(IdState::Invalid, |value| {
            IdState::Valid(GoId::String(value))
        });
    }
    if !is_json_number(raw) {
        return IdState::Invalid;
    }
    let Ok(text) = std::str::from_utf8(raw) else {
        return IdState::Invalid;
    };
    let Ok(value) = text.parse::<f64>() else {
        return IdState::Invalid;
    };
    normalize_finite_number(value, value == 0.0 && text.starts_with('-'))
        .map_or(IdState::Invalid, IdState::Valid)
}

fn classify_raw_params(raw: &[u8]) -> ParamsState {
    if raw == b"null" {
        return ParamsState::Null;
    }
    let mut index = 0;
    if raw.first() == Some(&b'{')
        && let Some(end) = skip_json_value_with_finite_numbers(raw, index)
    {
        index = end;
        skip_whitespace(raw, &mut index);
        if index == raw.len() {
            return ParamsState::Object;
        }
    }
    ParamsState::Other
}

fn scan_object_fields(raw: &[u8]) -> Option<Vec<RawField<'_>>> {
    let mut index = 0;
    skip_whitespace(raw, &mut index);
    if raw.get(index) != Some(&b'{') {
        return None;
    }
    index += 1;
    let mut fields = Vec::new();
    loop {
        skip_whitespace(raw, &mut index);
        if raw.get(index) == Some(&b'}') {
            index += 1;
            skip_whitespace(raw, &mut index);
            return (index == raw.len()).then_some(fields);
        }
        let key_start = index;
        let key_end = skip_json_string(raw, index)?;
        let name = decode_json_string(&raw[key_start..key_end])?;
        index = key_end;
        skip_whitespace(raw, &mut index);
        if raw.get(index) != Some(&b':') {
            return None;
        }
        index += 1;
        skip_whitespace(raw, &mut index);
        let value_start = index;
        let value_end = skip_json_value(raw, index)?;
        fields.push(RawField {
            name,
            value: &raw[value_start..value_end],
        });
        index = value_end;
        skip_whitespace(raw, &mut index);
        match raw.get(index) {
            Some(b',') => {
                index += 1;
                skip_whitespace(raw, &mut index);
                if raw.get(index) == Some(&b'}') {
                    return None;
                }
            }
            Some(b'}') => {
                index += 1;
                skip_whitespace(raw, &mut index);
                return (index == raw.len()).then_some(fields);
            }
            _ => return None,
        }
    }
}

pub(crate) fn skip_whitespace(raw: &[u8], index: &mut usize) {
    while raw
        .get(*index)
        .is_some_and(|byte| is_json_whitespace(*byte))
    {
        *index += 1;
    }
}

fn is_json_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn skip_json_string(raw: &[u8], mut index: usize) -> Option<usize> {
    if raw.get(index) != Some(&b'"') {
        return None;
    }
    index += 1;
    while let Some(byte) = raw.get(index) {
        match byte {
            b'\\' => {
                index += 1;
                match raw.get(index) {
                    Some(b'u') => {
                        for offset in 1..=4 {
                            if !raw
                                .get(index + offset)
                                .is_some_and(|byte| byte.is_ascii_hexdigit())
                            {
                                return None;
                            }
                        }
                        index += 5;
                    }
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                        index += 1;
                    }
                    _ => return None,
                }
            }
            b'"' => return Some(index + 1),
            byte if *byte < 0x20 => return None,
            _ => index += 1,
        }
    }
    None
}

fn decode_json_string(raw: &[u8]) -> Option<String> {
    if raw.first() != Some(&b'"') || raw.last() != Some(&b'"') {
        return None;
    }
    let mut output = String::new();
    let mut index = 1;
    let end = raw.len() - 1;
    let mut pending_high = None;
    while index < end {
        if raw[index] == b'\\' {
            let escape = *raw.get(index + 1)?;
            if escape == b'u' {
                let code = parse_hex_code_unit(raw, index + 2)?;
                if (0xd800..=0xdbff).contains(&code) {
                    flush_pending_surrogate(&mut output, &mut pending_high);
                    pending_high = Some(code);
                } else if (0xdc00..=0xdfff).contains(&code) {
                    if let Some(high) = pending_high.take() {
                        let scalar = 0x1_0000
                            + ((u32::from(high) - 0xd800) << 10)
                            + (u32::from(code) - 0xdc00);
                        output.push(char::from_u32(scalar)?);
                    } else {
                        output.push('\u{fffd}');
                    }
                } else {
                    flush_pending_surrogate(&mut output, &mut pending_high);
                    output.push(char::from_u32(u32::from(code))?);
                }
                index += 6;
                continue;
            }
            flush_pending_surrogate(&mut output, &mut pending_high);
            match escape {
                b'"' => output.push('"'),
                b'\\' => output.push('\\'),
                b'/' => output.push('/'),
                b'b' => output.push('\u{0008}'),
                b'f' => output.push('\u{000c}'),
                b'n' => output.push('\n'),
                b'r' => output.push('\r'),
                b't' => output.push('\t'),
                _ => return None,
            }
            index += 2;
            continue;
        }
        flush_pending_surrogate(&mut output, &mut pending_high);
        let chunk_end = raw[index..end]
            .iter()
            .position(|byte| *byte == b'\\')
            .map_or(end, |offset| index + offset);
        let chunk = &raw[index..chunk_end];
        let mut chunk_index = 0;
        while chunk_index < chunk.len() {
            match std::str::from_utf8(&chunk[chunk_index..]) {
                Ok(value) => {
                    output.push_str(value);
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        output.push_str(
                            std::str::from_utf8(&chunk[chunk_index..chunk_index + valid]).ok()?,
                        );
                        chunk_index += valid;
                    }
                    output.push('\u{fffd}');
                    chunk_index += 1;
                }
            }
        }
        index = chunk_end;
    }
    flush_pending_surrogate(&mut output, &mut pending_high);
    Some(output)
}

fn parse_hex_code_unit(raw: &[u8], start: usize) -> Option<u16> {
    let bytes = raw.get(start..start + 4)?;
    let mut value = 0u16;
    for byte in bytes {
        value = value.checked_mul(16)? + u16::from((*byte as char).to_digit(16)? as u8);
    }
    Some(value)
}

fn flush_pending_surrogate(output: &mut String, pending_high: &mut Option<u16>) {
    if pending_high.take().is_some() {
        output.push('\u{fffd}');
    }
}

pub(crate) fn skip_json_value(raw: &[u8], mut index: usize) -> Option<usize> {
    if raw.get(index) == Some(&b'"') {
        return skip_json_string(raw, index);
    }
    match raw.get(index) {
        Some(b'{') => {
            index += 1;
            skip_whitespace(raw, &mut index);
            if raw.get(index) == Some(&b'}') {
                return Some(index + 1);
            }
            loop {
                let key_end = skip_json_string(raw, index)?;
                index = key_end;
                skip_whitespace(raw, &mut index);
                if raw.get(index) != Some(&b':') {
                    return None;
                }
                index += 1;
                skip_whitespace(raw, &mut index);
                index = skip_json_value(raw, index)?;
                skip_whitespace(raw, &mut index);
                match raw.get(index) {
                    Some(b',') => {
                        index += 1;
                        skip_whitespace(raw, &mut index);
                    }
                    Some(b'}') => return Some(index + 1),
                    _ => return None,
                }
            }
        }
        Some(b'[') => {
            index += 1;
            skip_whitespace(raw, &mut index);
            if raw.get(index) == Some(&b']') {
                return Some(index + 1);
            }
            loop {
                index = skip_json_value(raw, index)?;
                skip_whitespace(raw, &mut index);
                match raw.get(index) {
                    Some(b',') => {
                        index += 1;
                        skip_whitespace(raw, &mut index);
                    }
                    Some(b']') => return Some(index + 1),
                    _ => return None,
                }
            }
        }
        _ => {}
    }
    let start = index;
    while raw
        .get(index)
        .is_some_and(|byte| !is_json_whitespace(*byte) && !matches!(byte, b',' | b'}' | b']'))
    {
        index += 1;
    }
    let token = &raw[start..index];
    (is_json_primitive(token)).then_some(index)
}

fn skip_json_value_with_finite_numbers(raw: &[u8], mut index: usize) -> Option<usize> {
    if raw.get(index) == Some(&b'"') {
        return skip_json_string(raw, index);
    }
    match raw.get(index) {
        Some(b'{') => {
            index += 1;
            skip_whitespace(raw, &mut index);
            if raw.get(index) == Some(&b'}') {
                return Some(index + 1);
            }
            loop {
                index = skip_json_string(raw, index)?;
                skip_whitespace(raw, &mut index);
                if raw.get(index) != Some(&b':') {
                    return None;
                }
                index += 1;
                skip_whitespace(raw, &mut index);
                index = skip_json_value_with_finite_numbers(raw, index)?;
                skip_whitespace(raw, &mut index);
                match raw.get(index) {
                    Some(b',') => {
                        index += 1;
                        skip_whitespace(raw, &mut index);
                        if raw.get(index) == Some(&b'}') {
                            return None;
                        }
                    }
                    Some(b'}') => return Some(index + 1),
                    _ => return None,
                }
            }
        }
        Some(b'[') => {
            index += 1;
            skip_whitespace(raw, &mut index);
            if raw.get(index) == Some(&b']') {
                return Some(index + 1);
            }
            loop {
                index = skip_json_value_with_finite_numbers(raw, index)?;
                skip_whitespace(raw, &mut index);
                match raw.get(index) {
                    Some(b',') => {
                        index += 1;
                        skip_whitespace(raw, &mut index);
                        if raw.get(index) == Some(&b']') {
                            return None;
                        }
                    }
                    Some(b']') => return Some(index + 1),
                    _ => return None,
                }
            }
        }
        _ => {}
    }
    let start = index;
    while raw
        .get(index)
        .is_some_and(|byte| !is_json_whitespace(*byte) && !matches!(byte, b',' | b'}' | b']'))
    {
        index += 1;
    }
    let token = &raw[start..index];
    if is_json_number(token) {
        let value = std::str::from_utf8(token).ok()?.parse::<f64>().ok()?;
        if !value.is_finite() {
            return None;
        }
    }
    is_json_primitive(token).then_some(index)
}

fn is_json_primitive(value: &[u8]) -> bool {
    matches!(value, b"null" | b"true" | b"false") || is_json_number(value)
}

fn is_json_number(value: &[u8]) -> bool {
    let Ok(value) = std::str::from_utf8(value) else {
        return false;
    };
    let bytes = value.as_bytes();
    let mut index = 0;
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    match bytes.get(index) {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
        }
        _ => return false,
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fraction_start {
            return false;
        }
    }
    if matches!(bytes.get(index), Some(b'e') | Some(b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+') | Some(b'-')) {
            index += 1;
        }
        let exponent_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }
    index == bytes.len()
}

fn go_float_text(serialized: &str) -> String {
    let Some((raw_mantissa, exponent)) = serialized
        .split_once('e')
        .or_else(|| serialized.split_once('E'))
    else {
        return serialized
            .strip_suffix(".0")
            .unwrap_or(serialized)
            .to_owned();
    };
    let mantissa = raw_mantissa.strip_suffix(".0").unwrap_or(raw_mantissa);
    let exponent = exponent.parse::<i32>().expect("serde_json exponent");
    if (-6..=20).contains(&exponent) {
        return expand_decimal(mantissa, exponent);
    }
    format!(
        "{mantissa}e{}{exponent}",
        if exponent >= 0 { "+" } else { "" }
    )
}

fn expand_decimal(mantissa: &str, exponent: i32) -> String {
    let negative = mantissa.starts_with('-');
    let digits = mantissa.trim_start_matches('-').replace('.', "");
    let decimal_at = mantissa
        .find('.')
        .unwrap_or(mantissa.len())
        .saturating_sub(negative as usize) as i32
        + exponent;
    let mut output = String::new();
    if negative {
        output.push('-');
    }
    if decimal_at <= 0 {
        output.push_str("0.");
        output.extend(std::iter::repeat_n('0', (-decimal_at) as usize));
        output.push_str(&digits);
    } else if decimal_at as usize >= digits.len() {
        output.push_str(&digits);
        output.extend(std::iter::repeat_n('0', decimal_at as usize - digits.len()));
    } else {
        let split = decimal_at as usize;
        output.push_str(&digits[..split]);
        output.push('.');
        output.push_str(&digits[split..]);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Fixture {
        source_revision: String,
        source_path: String,
        cases: Vec<FixtureCase>,
    }

    #[derive(Deserialize)]
    struct FixtureCase {
        name: String,
        request: Option<String>,
        request_b64: Option<String>,
        response: Option<String>,
        #[serde(default)]
        parse_error: bool,
    }

    fn decode_base64(input: &str) -> Vec<u8> {
        let mut output = Vec::new();
        let mut buffer = 0u32;
        let mut bits = 0;
        for byte in input.bytes() {
            let value = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => break,
                _ => panic!("invalid fixture base64"),
            };
            buffer = (buffer << 6) | u32::from(value);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                output.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        }
        output
    }

    fn response(raw: &str) -> Vec<u8> {
        match initialize(raw.as_bytes()) {
            InitializeOutcome::Response(value) => value,
            other => panic!("expected response, got {other:?}"),
        }
    }

    fn go_json(raw: &str) -> Vec<u8> {
        let mut output = raw.as_bytes().to_vec();
        output.push(b'\n');
        output
    }

    #[test]
    fn initialize_response_is_byte_stable_and_lexically_ordered() {
        assert_eq!(
            response(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#),
            go_json(
                r#"{"jsonrpc":"2.0","result":{"capabilities":{"tools":{}},"protocolVersion":"2025-06-18","serverInfo":{"name":"symeraseme","version":"dev"}},"id":1}"#,
            )
        );
        assert_eq!(
            response(r#"{"jsonrpc":"2.0","id":1.0,"method":"initialize","params":null}"#),
            go_json(
                r#"{"jsonrpc":"2.0","result":{"capabilities":{"tools":{}},"protocolVersion":"2025-06-18","serverInfo":{"name":"symeraseme","version":"dev"}},"id":1}"#,
            )
        );
    }

    #[test]
    fn params_and_id_validation_match_go() {
        assert!(matches!(
            initialize(br#"{"jsonrpc":"2.0","method":"initialize"}"#),
            InitializeOutcome::Notification
        ));
        assert_eq!(
            response(r#"{"jsonrpc":"2.0","id":"x","method":"initialize","params":[]}"#),
            go_json(
                r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"invalid params"},"id":"x"}"#,
            )
        );
        assert_eq!(
            response(r#"{"jsonrpc":"2.0","id":true,"method":"initialize"}"#),
            go_json(
                r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"invalid request"},"id":null}"#,
            )
        );
        assert_eq!(
            response(r#"{"jsonrpc":"1.0","id":1,"method":"initialize"}"#),
            go_json(
                r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"invalid request"},"id":1}"#,
            )
        );
    }

    #[test]
    fn shared_entry_dispatches_tools_list_to_the_tools_list_slice() {
        const REQUEST: &[u8] = include_bytes!(
            "../../../../tests/fixtures/mcp-contract/mcp-002/tools-list.request.jsonl"
        );
        const RESPONSE: &[u8] = include_bytes!(
            "../../../../tests/fixtures/mcp-contract/mcp-002/tools-list.response.json"
        );
        assert_eq!(
            initialize(REQUEST),
            InitializeOutcome::Response(RESPONSE.to_vec())
        );
    }

    #[test]
    fn shared_entry_keeps_tools_list_notifications_silent() {
        const NOTIFICATION: &[u8] = include_bytes!(
            "../../../../tests/fixtures/mcp-contract/mcp-002/tools-list.notification.request.jsonl"
        );
        assert_eq!(initialize(NOTIFICATION), InitializeOutcome::Notification);
    }

    #[test]
    fn source_bound_go_envelope_fixture_matches_through_the_shared_entry() {
        #[derive(Deserialize)]
        struct EnvelopeCase {
            name: String,
            request: Option<String>,
            response: Option<String>,
            #[serde(default)]
            parse_error: bool,
        }

        #[derive(Deserialize)]
        struct EnvelopeFixture {
            cases: Vec<EnvelopeCase>,
        }

        let fixture: EnvelopeFixture = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/mcp-contract/mcp-envelope/cases.json"
        ))
        .expect("envelope fixture");
        for case in fixture.cases {
            let request = case.request.expect("fixture request").into_bytes();
            let actual = match initialize(&request) {
                InitializeOutcome::Response(bytes) => Some(String::from_utf8(bytes).unwrap()),
                InitializeOutcome::Notification => None,
                InitializeOutcome::ParseError => {
                    assert!(
                        case.parse_error,
                        "{} unexpectedly parsed as error",
                        case.name
                    );
                    None
                }
            };
            if !case.parse_error {
                assert_eq!(actual, case.response, "{}", case.name);
            }
        }
    }

    #[test]
    fn malformed_json_is_left_to_the_transport_parser() {
        assert_eq!(initialize(br#"{"#), InitializeOutcome::ParseError);
    }

    #[test]
    fn source_bound_go_initialize_fixture_matches() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/mcp-contract/initialize_cases.json"
        ))
        .expect("initialize fixture");
        assert_eq!(
            fixture.source_revision,
            "a51c7f3c65218924ce1d505ad8389b2216f08c92"
        );
        assert_eq!(
            fixture.source_path,
            "internal/mcp/server.go:180-207,377-389"
        );
        for case in fixture.cases {
            let request = case
                .request_b64
                .as_deref()
                .map(decode_base64)
                .or_else(|| {
                    case.request
                        .as_deref()
                        .map(|request| request.as_bytes().to_vec())
                })
                .expect("fixture request");
            let actual = match initialize(&request) {
                InitializeOutcome::Response(bytes) => Some(String::from_utf8(bytes).unwrap()),
                InitializeOutcome::Notification => None,
                InitializeOutcome::ParseError => {
                    assert!(
                        case.parse_error,
                        "{} unexpectedly parsed as error",
                        case.name
                    );
                    None
                }
            };
            if !case.parse_error {
                assert_eq!(actual, case.response, "{}", case.name);
            }
        }
    }
}
