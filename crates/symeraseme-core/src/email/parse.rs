//! Header and encoded-word decoding.
//!
//! Both algorithms are ports of the Go standard library, because the header
//! bytes a broker sends end up in the wire contract:
//!
//! * the header block is read like `net/mail`'s `readHeader` on top of
//!   `net/textproto`'s continued-line reader (trim, then continuations joined
//!   with a single space after their leading whitespace is skipped);
//! * `decode_header` follows `mime.WordDecoder.DecodeHeader`, including its
//!   quirks: whitespace between two encoded words disappears, a word whose
//!   encoding does not decode is kept verbatim, and an unhandled charset makes
//!   the caller fall back to the raw header value.

use crate::email::session::FetchedMessage;
use crate::email::types::Message;
use base64::Engine;
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};
use regex::Regex;
use std::sync::OnceLock;

/// Reads one header block and returns its fields in order.
fn read_header(header: &[u8]) -> Result<Vec<(String, String)>, String> {
    let text = String::from_utf8_lossy(header);
    let mut lines: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        lines.push(line.strip_suffix('\r').unwrap_or(line));
    }

    let mut fields = Vec::new();
    let mut index = 0;
    if let Some(first) = lines.first()
        && (first.starts_with(' ') || first.starts_with('\t'))
    {
        return Err(format!("malformed initial line: {first}"));
    }
    while index < lines.len() {
        if lines[index].is_empty() {
            break;
        }
        let mut folded = trim_spaces(lines[index]).to_string();
        index += 1;
        while index < lines.len() {
            let continuation = lines[index];
            if !(continuation.starts_with(' ') || continuation.starts_with('\t')) {
                break;
            }
            folded.push(' ');
            folded.push_str(trim_spaces(continuation));
            index += 1;
        }
        let Some((key, value)) = folded.split_once(':') else {
            return Err(format!("malformed header line: {folded}"));
        };
        if key.is_empty() {
            continue;
        }
        fields.push((key.to_string(), trim_spaces(value).to_string()));
    }
    Ok(fields)
}

fn trim_spaces(value: &str) -> &str {
    value.trim_matches(|c| c == ' ' || c == '\t')
}

fn header_value<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Replaces invalid UTF-8 bytes the way Go's JSON encoder does: one U+FFFD per
/// offending byte, so a body cut mid-character survives identically.
pub fn to_valid_utf8_lossy_per_byte(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut remaining = bytes;
    loop {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                out.push_str(valid);
                return out;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                out.push_str(std::str::from_utf8(&remaining[..valid_up_to]).expect("valid prefix"));
                out.push('\u{FFFD}');
                remaining = &remaining[valid_up_to + 1..];
                if remaining.is_empty() {
                    return out;
                }
            }
        }
    }
}

/// `mime.WordDecoder.DecodeHeader`: `None` means the caller must keep the raw
/// header value (Go's unhandled-charset error, which the caller swallows).
pub fn decode_header(value: &str) -> Option<String> {
    let start_of_word = value.find("=?");
    let Some(offset_start) = start_of_word else {
        return Some(value.to_string());
    };
    let mut out = String::new();
    out.push_str(&value[..offset_start]);
    let mut header = &value[offset_start..];
    let mut between_words = false;

    while let Some(start) = header.find("=?") {
        let mut cursor = start + 2;
        let Some(charset_end) = header[cursor..].find('?') else {
            break;
        };
        let charset = &header[cursor..cursor + charset_end];
        cursor += charset_end + 1;
        if header.len() < cursor + 3 {
            break;
        }
        let encoding = header.as_bytes()[cursor];
        cursor += 1;
        if header.as_bytes()[cursor] != b'?' {
            break;
        }
        cursor += 1;
        let Some(text_end) = header[cursor..].find("?=") else {
            break;
        };
        let text = &header[cursor..cursor + text_end];
        let end = cursor + text_end + 2;

        match decode_word(encoding, text) {
            None => {
                between_words = false;
                out.push_str(&header[..end]);
                header = &header[end..];
                continue;
            }
            Some(content) => {
                let prefix = &header[..start];
                if start > 0 && (!between_words || has_non_whitespace(prefix)) {
                    out.push_str(prefix);
                }
                let converted = convert_charset(charset, &content)?;
                out.push_str(&converted);
                header = &header[end..];
                between_words = true;
            }
        }
    }
    if !header.is_empty() {
        out.push_str(header);
    }
    Some(out)
}

fn has_non_whitespace(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
}

fn decode_word(encoding: u8, text: &str) -> Option<Vec<u8>> {
    match encoding {
        b'B' | b'b' => base64::engine::general_purpose::STANDARD.decode(text).ok(),
        b'Q' | b'q' => quoted_printable_decode(text),
        _ => None,
    }
}

/// `mime.qDecode`: `_` is a space, `=XX` is a byte, control bytes are refused.
fn quoted_printable_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'_' => out.push(b' '),
            b'=' => {
                if index + 2 >= bytes.len() {
                    return None;
                }
                let high = from_hex(bytes[index + 1])?;
                let low = from_hex(bytes[index + 2])?;
                out.push(high << 4 | low);
                index += 2;
            }
            byte if (b' '..=b'~').contains(&byte) || matches!(byte, b'\n' | b'\r' | b'\t') => {
                out.push(byte)
            }
            _ => return None,
        }
        index += 1;
    }
    Some(out)
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// `mime.WordDecoder.convert` with a nil CharsetReader: UTF-8, ISO-8859-1 and
/// US-ASCII are handled, anything else is an unhandled charset.
fn convert_charset(charset: &str, content: &[u8]) -> Option<String> {
    if charset.eq_ignore_ascii_case("utf-8") {
        return Some(to_valid_utf8_lossy_per_byte(content));
    }
    if charset.eq_ignore_ascii_case("iso-8859-1") {
        return Some(content.iter().map(|byte| *byte as char).collect());
    }
    if charset.eq_ignore_ascii_case("us-ascii") {
        return Some(
            content
                .iter()
                .map(|byte| {
                    if *byte >= 0x80 {
                        '\u{FFFD}'
                    } else {
                        *byte as char
                    }
                })
                .collect(),
        );
    }
    None
}

fn message_id_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"<[^>]+>").expect("static pattern"))
}

fn first_message_id(value: &str) -> String {
    message_id_pattern()
        .find(value)
        .map(|found| found.as_str().to_string())
        .unwrap_or_default()
}

/// `email.ParseFetchedMessage`. Errors carry Go's text so a fixture can compare
/// them byte for byte.
pub fn parse_fetched_message(fetched: &FetchedMessage) -> Result<Message, String> {
    let fields = read_header(&fetched.header)?;
    let decoded = |name: &str| -> Option<String> {
        let raw = header_value(&fields, name)?;
        if raw.is_empty() {
            return Some(String::new());
        }
        decode_header(raw)
    };

    let message_id = decoded("Message-ID").unwrap_or_else(|| {
        header_value(&fields, "Message-ID")
            .unwrap_or_default()
            .to_string()
    });
    let references = decoded("References").unwrap_or_else(|| {
        header_value(&fields, "References")
            .unwrap_or_default()
            .to_string()
    });
    let mut thread_id = first_message_id(&references);
    if thread_id.is_empty() {
        let in_reply_to = decoded("In-Reply-To").unwrap_or_else(|| {
            header_value(&fields, "In-Reply-To")
                .unwrap_or_default()
                .to_string()
        });
        thread_id = first_message_id(&in_reply_to);
    }
    if thread_id.is_empty() {
        thread_id = first_message_id(&message_id);
    }

    let mut date = None;
    if let Some(value) = header_value(&fields, "Date")
        && !value.is_empty()
    {
        date = parse_date(value);
    }
    if date.is_none()
        && let Some(internal) = fetched.internal_date
    {
        date = Some(internal.fixed_offset());
    }

    Ok(Message {
        id: fetched.uid.to_string(),
        subject: decoded("Subject").unwrap_or_else(|| {
            header_value(&fields, "Subject")
                .unwrap_or_default()
                .to_string()
        }),
        from: decoded("From").unwrap_or_else(|| {
            header_value(&fields, "From")
                .unwrap_or_default()
                .to_string()
        }),
        to: decoded("To")
            .unwrap_or_else(|| header_value(&fields, "To").unwrap_or_default().to_string()),
        date,
        body: to_valid_utf8_lossy_per_byte(&fetched.body),
        flags: fetched.flags.clone(),
        message_id,
        thread_id,
        imap_uid: fetched.uid,
    })
}

/// `net/mail`'s date layouts: `[Mon, ]D Mon [YY]YY HH:MM[:SS] zone`, where the
/// zone is a numeric offset, a three-letter name, or `UT`.
fn parse_date(value: &str) -> Option<DateTime<FixedOffset>> {
    let text = value.trim();
    let mut rest = text;
    if let Some(comma) = rest.find(',') {
        rest = rest[comma + 1..].trim_start();
    }
    let mut parts = rest.split_whitespace();
    let day: u32 = parts.next()?.parse().ok()?;
    let month = month_number(parts.next()?)?;
    let year_text = parts.next()?;
    let (year, clock_text) =
        if year_text.len() == 4 && year_text.chars().all(|c| c.is_ascii_digit()) {
            (year_text.parse::<i32>().ok()?, parts.next()?.to_string())
        } else {
            // A two-digit year: 00-68 is 2000s, 69-99 is 1900s.
            let short: i32 = year_text.parse().ok()?;
            let year = if short >= 69 {
                1900 + short
            } else {
                2000 + short
            };
            (year, parts.next()?.to_string())
        };
    let mut clock_parts = clock_text.split(':');
    let hour: u32 = clock_parts.next()?.parse().ok()?;
    let minute: u32 = clock_parts.next()?.parse().ok()?;
    let second: u32 = match clock_parts.next() {
        Some(value) => value.parse().ok()?,
        None => 0,
    };
    let zone_text = parts.next()?;
    let offset = parse_zone(zone_text)?;
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let naive = date.and_hms_opt(hour, minute, second)?;
    offset.from_local_datetime(&naive).single()
}

fn month_number(name: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lowered = name.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|month| *month == lowered)
        .map(|index| index as u32 + 1)
}

fn parse_zone(value: &str) -> Option<FixedOffset> {
    let bytes = value.as_bytes();
    let sign = match bytes.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => {
            let letters: String = value.chars().filter(|c| c.is_ascii_alphabetic()).collect();
            return match letters.to_ascii_uppercase().as_str() {
                // "MST" is the layout's three-letter zone; Go maps unknown
                // abbreviations to a zero offset, as do UT/GMT/UTC.
                name if name.len() == 3 || name == "GMT" || name == "UTC" => {
                    FixedOffset::east_opt(0)
                }
                _ => None,
            };
        }
    };
    if bytes.len() < 5 {
        return None;
    }
    let hours: i32 = value[1..3].parse().ok()?;
    let minutes: i32 = value[3..5].parse().ok()?;
    FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))
}
