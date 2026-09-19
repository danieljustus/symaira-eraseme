//! JSON forms of the inbox values.
//!
//! The wire text is written by hand instead of going through a serializer,
//! because three properties of Go's `encoding/json` are part of the contract and
//! a generic serializer loses them:
//!
//! * field order — Go marshals these structs field by field (`ID` first), while
//!   a JSON value built from a map would come out sorted;
//! * `<`, `>` and `&` are escaped as `\u003c`, `\u003e`, `\u0026` (Go escapes
//!   HTML by default);
//! * an empty list the caller never received is `null`, not `[]`.
//!
//! `rust-tests/parity/oracle/email` records the Go bytes; `tests/email_parity.rs`
//! compares them character for character.

use crate::email::types::{MatchedMessage, Message};
use chrono::{DateTime, FixedOffset};

/// Go's `time.Time` JSON form: RFC 3339 with nanoseconds, trailing zeros
/// trimmed, and `Z` for a zero offset.
pub fn format_go_timestamp(value: &DateTime<FixedOffset>) -> String {
    let offset = value.offset().local_minus_utc();
    let mut formatted = value.naive_local().format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = value.timestamp_subsec_nanos();
    if nanos != 0 {
        let mut fraction = format!(".{nanos:09}");
        while fraction.ends_with('0') {
            fraction.pop();
        }
        formatted.push_str(&fraction);
    }
    if offset == 0 {
        formatted.push('Z');
    } else {
        let sign = if offset < 0 { '-' } else { '+' };
        let absolute = offset.abs();
        formatted.push_str(&format!(
            "{sign}{:02}:{:02}",
            absolute / 3600,
            (absolute % 3600) / 60
        ));
    }
    formatted
}

/// Go's `encoding/json` string encoding, including HTML escaping and the
/// line-separator escapes.
pub fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

fn write_string_field(out: &mut String, name: &str, value: &str) {
    write_json_string(out, name);
    out.push(':');
    write_json_string(out, value);
}

fn write_flags(out: &mut String, flags: &Option<Vec<String>>) {
    write_json_string(out, "Flags");
    out.push(':');
    match flags {
        Some(flags) => {
            out.push('[');
            for (index, flag) in flags.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json_string(out, flag);
            }
            out.push(']');
        }
        None => out.push_str("null"),
    }
}

pub fn write_message(out: &mut String, message: &Message) {
    out.push('{');
    write_string_field(out, "ID", &message.id);
    out.push(',');
    write_string_field(out, "Subject", &message.subject);
    out.push(',');
    write_string_field(out, "From", &message.from);
    out.push(',');
    write_string_field(out, "To", &message.to);
    out.push(',');
    write_json_string(out, "Date");
    out.push(':');
    match message.date {
        Some(date) => write_json_string(out, &format_go_timestamp(&date)),
        None => out.push_str("null"),
    }
    out.push(',');
    write_string_field(out, "Body", &message.body);
    out.push(',');
    write_flags(out, &message.flags);
    out.push(',');
    write_string_field(out, "MessageID", &message.message_id);
    out.push(',');
    write_string_field(out, "ThreadID", &message.thread_id);
    out.push(',');
    write_json_string(out, "IMAPUID");
    out.push(':');
    out.push_str(&message.imap_uid.to_string());
    out.push('}');
}

fn write_messages(out: &mut String, messages: &[Message]) {
    out.push('[');
    for (index, message) in messages.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_message(out, message);
    }
    out.push(']');
}

pub fn message_json(message: &Message) -> String {
    let mut out = String::new();
    write_message(&mut out, message);
    out
}

pub fn messages_json(messages: &[Message]) -> String {
    let mut out = String::new();
    write_messages(&mut out, messages);
    out
}

/// `None` reproduces Go's nil slice, which marshals as `null`.
pub fn optional_messages_json(messages: Option<&[Message]>) -> String {
    match messages {
        Some(messages) => messages_json(messages),
        None => "null".to_string(),
    }
}

pub fn matched_messages_json(matched: &[MatchedMessage]) -> String {
    let mut out = String::from("[");
    for (index, reply) in matched.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('{');
        write_json_string(&mut out, "Message");
        out.push(':');
        write_message(&mut out, &reply.message);
        out.push(',');
        write_json_string(&mut out, "RequestID");
        out.push(':');
        match reply.request_id {
            Some(request_id) => out.push_str(&request_id.to_string()),
            None => out.push_str("null"),
        }
        out.push(',');
        write_string_field(&mut out, "MatchMethod", reply.match_method.as_str());
        out.push('}');
    }
    out.push(']');
    out
}
