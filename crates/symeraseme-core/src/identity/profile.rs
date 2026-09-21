//! Read-only Go identity profile discovery and authenticated loading.
//!
//! This is the identity JSON-header/AES-GCM envelope, NOT the event-store
//! encryption format. The caller supplies paths and an existing-key resolver;
//! loading never initializes a key, creates directories, or rewrites a profile.

use super::{KeyringBackend, MasterKeyError, MasterKeyResolver};
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// The complete Go profile fields, including normalized empty collections.
/// Deliberately has no `Debug` implementation: profiles contain personal data.
#[derive(Clone, Default, Eq, PartialEq, Serialize)]
pub struct Profile {
    pub full_name: String,
    #[serde(serialize_with = "serialize_go_cloned_slice")]
    pub name_variants: Vec<String>,
    pub date_of_birth: Option<String>,
    pub addresses: Vec<ProfileAddress>,
    #[serde(serialize_with = "serialize_go_cloned_slice")]
    pub email_addresses: Vec<String>,
    #[serde(serialize_with = "serialize_go_cloned_slice")]
    pub phone_numbers: Vec<String>,
    #[serde(serialize_with = "serialize_go_cloned_slice")]
    pub jurisdictions: Vec<String>,
}

// LoadProfile normalizes slices and then clone() uses append([]string(nil),
// values...), returning nil for empty string slices. Addresses use make and
// remain []. Preserve the measured public loader result, not normalize alone.
fn serialize_go_cloned_slice<S: serde::Serializer>(
    values: &[String],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if values.is_empty() {
        serializer.serialize_none()
    } else {
        values.serialize(serializer)
    }
}

/// Address fields retained by the Go identity model, including optional dates.
#[derive(Clone, Default, Eq, PartialEq, Serialize)]
pub struct ProfileAddress {
    pub street: String,
    pub city: String,
    pub postal_code: String,
    pub country: String,
    pub state: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

// Go encoding/json matches tagged names case-insensitively, processes duplicate
// fields in input order, ignores unknown fields, and accepts null zero values.
// Ordinary serde derives reject duplicates/null and are not equivalent.
macro_rules! read_field {
    ($map:ident, $target:expr, scalar) => {
        if let Some(value) = $map.next_value::<Option<String>>()? {
            $target = value;
        }
    };
    ($map:ident, $target:expr, optional) => {
        $target = $map.next_value::<Option<String>>()?;
    };
    ($map:ident, $target:expr, strings) => {
        $target = $map
            .next_value::<Option<Vec<Option<String>>>>()?
            .unwrap_or_default()
            .into_iter()
            .map(Option::unwrap_or_default)
            .collect();
    };
    ($map:ident, $target:expr, addresses) => {
        $target = $map
            .next_value::<Option<Vec<ProfileAddress>>>()?
            .unwrap_or_default();
    };
    ($map:ident, $target:expr, integer) => {
        if let Some(value) = $map.next_value::<Option<i64>>()? {
            $target = value;
        }
    };
}

macro_rules! go_record {
    ($record:ident { $($field:ident: $kind:ident),* $(,)? }) => {
        impl<'de> Deserialize<'de> for $record {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct RecordVisitor;
                impl<'de> Visitor<'de> for RecordVisitor {
                    type Value = $record;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str("an identity object or null")
                    }
                    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                        Ok($record::default())
                    }
                    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                        let mut record = $record::default();
                        while let Some(name) = map.next_key::<String>()? {
                            match name.to_lowercase().replace('ſ', "s").as_str() {
                                $(stringify!($field) => { read_field!(map, record.$field, $kind); })*
                                _ => { map.next_value::<IgnoredAny>()?; }
                            }
                        }
                        Ok(record)
                    }
                }
                deserializer.deserialize_any(RecordVisitor)
            }
        }
    };
}

go_record!(Profile {
    full_name: scalar,
    name_variants: strings,
    date_of_birth: optional,
    addresses: addresses,
    email_addresses: strings,
    phone_numbers: strings,
    jurisdictions: strings,
});
go_record!(ProfileAddress {
    street: scalar,
    city: scalar,
    postal_code: scalar,
    country: scalar,
    state: optional,
    valid_from: optional,
    valid_to: optional,
});

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Envelope {
    pub version: i64,
    pub nonce: String,
    pub algorithm: String,
}
go_record!(Envelope {
    version: integer,
    nonce: scalar,
    algorithm: scalar
});

const GO_MAX_JSON_NESTING_DEPTH: usize = 10_000;

#[derive(Clone, Copy)]
enum ProfileFieldKind {
    Scalar,
    Optional,
    Strings,
    Addresses,
}

fn profile_field_kind(name: &str, address: bool) -> Option<ProfileFieldKind> {
    let name = name.to_lowercase().replace('ſ', "s");
    if address {
        return match name.as_str() {
            "street" | "city" | "postal_code" | "country" => Some(ProfileFieldKind::Scalar),
            "state" | "valid_from" | "valid_to" => Some(ProfileFieldKind::Optional),
            _ => None,
        };
    }
    match name.as_str() {
        "full_name" => Some(ProfileFieldKind::Scalar),
        "name_variants" | "email_addresses" | "phone_numbers" | "jurisdictions" => {
            Some(ProfileFieldKind::Strings)
        }
        "date_of_birth" => Some(ProfileFieldKind::Optional),
        "addresses" => Some(ProfileFieldKind::Addresses),
        _ => None,
    }
}

/// Normalize the two JSON input behaviors where Go's encoding/json is more
/// permissive than serde_json: invalid UTF-8 in strings and lone surrogates.
/// The structural walk also enforces Go's 10,000-container nesting limit.
fn normalize_go_json(input: &[u8]) -> Result<Vec<u8>, ()> {
    let mut normalized = Vec::with_capacity(input.len());
    let mut position = 0;
    let mut depth: usize = 0;
    while position < input.len() {
        match input[position] {
            b'"' => {
                position = normalize_json_string(input, position, &mut normalized)?;
            }
            b'{' | b'[' => {
                depth = depth.checked_add(1).ok_or(())?;
                if depth > GO_MAX_JSON_NESTING_DEPTH {
                    return Err(());
                }
                normalized.push(input[position]);
                position += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                normalized.push(input[position]);
                position += 1;
            }
            byte => {
                normalized.push(byte);
                position += 1;
            }
        }
    }
    Ok(normalized)
}

fn normalize_json_string(input: &[u8], start: usize, output: &mut Vec<u8>) -> Result<usize, ()> {
    if input.get(start) != Some(&b'"') {
        return Err(());
    }
    output.push(b'"');
    let mut position = start + 1;
    while let Some(&byte) = input.get(position) {
        match byte {
            b'"' => {
                output.push(byte);
                return Ok(position + 1);
            }
            b'\\' => {
                let escape = *input.get(position + 1).ok_or(())?;
                if escape == b'u' {
                    let code = parse_hex_quad(input, position + 2).ok_or(())?;
                    if (0xd800..=0xdbff).contains(&code) {
                        let pair = if input.get(position + 6) == Some(&b'\\')
                            && input.get(position + 7) == Some(&b'u')
                        {
                            parse_hex_quad(input, position + 8)
                        } else {
                            None
                        };
                        if pair.is_some_and(|value| (0xdc00..=0xdfff).contains(&value)) {
                            output.extend_from_slice(&input[position..position + 12]);
                            position += 12;
                            continue;
                        }
                        output.extend_from_slice(b"\\ufffd");
                    } else if (0xdc00..=0xdfff).contains(&code) {
                        output.extend_from_slice(b"\\ufffd");
                    } else {
                        output.extend_from_slice(&input[position..position + 6]);
                    }
                    position += 6;
                    continue;
                }
                if !matches!(
                    escape,
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't'
                ) {
                    return Err(());
                }
                output.extend_from_slice(&input[position..position + 2]);
                position += 2;
            }
            byte if byte < 0x20 => return Err(()),
            byte if byte < 0x80 => {
                output.push(byte);
                position += 1;
            }
            _ => {
                if let Some(width) = valid_utf8_width(input, position) {
                    output.extend_from_slice(&input[position..position + width]);
                    position += width;
                } else {
                    output.extend_from_slice(b"\xef\xbf\xbd");
                    position += 1;
                }
            }
        }
    }
    Err(())
}

fn parse_hex_quad(input: &[u8], start: usize) -> Option<u16> {
    let mut value = 0_u16;
    for offset in 0..4 {
        value = value.checked_mul(16)?;
        value = value.checked_add(hex_value(input.get(start + offset).copied()?)? as u16)?;
    }
    Some(value)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn valid_utf8_width(input: &[u8], position: usize) -> Option<usize> {
    let first = *input.get(position)?;
    let next = |offset| input.get(position + offset).copied();
    let continuation = |byte: Option<u8>| byte.is_some_and(|value| (0x80..=0xbf).contains(&value));
    match first {
        0xc2..=0xdf if continuation(next(1)) => Some(2),
        0xe0 if matches!(next(1), Some(0xa0..=0xbf)) && continuation(next(2)) => Some(3),
        0xed if matches!(next(1), Some(0x80..=0x9f)) && continuation(next(2)) => Some(3),
        value
            if matches!(value, 0xe1..=0xec | 0xee..=0xef)
                && continuation(next(1))
                && continuation(next(2)) =>
        {
            Some(3)
        }
        0xf0 if matches!(next(1), Some(0x90..=0xbf))
            && continuation(next(2))
            && continuation(next(3)) =>
        {
            Some(4)
        }
        0xf4 if matches!(next(1), Some(0x80..=0x8f))
            && continuation(next(2))
            && continuation(next(3)) =>
        {
            Some(4)
        }
        value
            if matches!(value, 0xf1..=0xf3)
                && continuation(next(1))
                && continuation(next(2))
                && continuation(next(3)) =>
        {
            Some(4)
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum ArrayState {
    ValueOrEnd,
    Value,
    CommaOrEnd,
}

#[derive(Clone, Copy)]
enum ObjectState {
    KeyOrEnd,
    Key,
    Value,
    CommaOrEnd,
}

#[derive(Clone, Copy)]
enum JsonFrame {
    Array(ArrayState),
    Object(ObjectState),
}

struct JsonCursor<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> JsonCursor<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    fn current(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn skip_ws(&mut self) {
        while self
            .current()
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.position += 1;
        }
    }

    fn parse_string(&mut self) -> Result<Range<usize>, ()> {
        let start = self.position;
        if self.current() != Some(b'"') {
            return Err(());
        }
        self.position += 1;
        while let Some(&byte) = self.input.get(self.position) {
            match byte {
                b'"' => {
                    self.position += 1;
                    return Ok(start..self.position);
                }
                b'\\' => {
                    let escape = *self.input.get(self.position + 1).ok_or(())?;
                    if escape == b'u' {
                        parse_hex_quad(self.input, self.position + 2).ok_or(())?;
                        self.position += 6;
                    } else if matches!(
                        escape,
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't'
                    ) {
                        self.position += 2;
                    } else {
                        return Err(());
                    }
                }
                value if value < 0x20 => return Err(()),
                value if value < 0x80 => self.position += 1,
                _ => {
                    let width = valid_utf8_width(self.input, self.position).ok_or(())?;
                    self.position += width;
                }
            }
        }
        Err(())
    }

    fn parse_literal(&mut self, literal: &[u8]) -> Result<(), ()> {
        let end = self.position.checked_add(literal.len()).ok_or(())?;
        if self.input.get(self.position..end) != Some(literal) {
            return Err(());
        }
        self.position = end;
        Ok(())
    }

    fn parse_number(&mut self) -> Result<(), ()> {
        let start = self.position;
        if self.current() == Some(b'-') {
            self.position += 1;
        }
        match self.current() {
            Some(b'0') => self.position += 1,
            Some(b'1'..=b'9') => {
                self.position += 1;
                while self.current().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.position += 1;
                }
            }
            _ => return Err(()),
        }
        if self.current() == Some(b'.') {
            self.position += 1;
            let digits = self.position;
            while self.current().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
            if self.position == digits {
                return Err(());
            }
        }
        if self
            .current()
            .is_some_and(|byte| matches!(byte, b'e' | b'E'))
        {
            self.position += 1;
            if self
                .current()
                .is_some_and(|byte| matches!(byte, b'+' | b'-'))
            {
                self.position += 1;
            }
            let digits = self.position;
            while self.current().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
            if self.position == digits {
                return Err(());
            }
        }
        if self.position == start || !is_value_delimiter(self.current()) {
            return Err(());
        }
        Ok(())
    }

    fn start_value(&mut self, frames: &mut Vec<JsonFrame>) -> Result<(), ()> {
        self.skip_ws();
        match self.current() {
            Some(b'"') => {
                self.parse_string()?;
            }
            Some(b'[') => {
                if frames.len() >= GO_MAX_JSON_NESTING_DEPTH {
                    return Err(());
                }
                self.position += 1;
                frames.push(JsonFrame::Array(ArrayState::ValueOrEnd));
            }
            Some(b'{') => {
                if frames.len() >= GO_MAX_JSON_NESTING_DEPTH {
                    return Err(());
                }
                self.position += 1;
                frames.push(JsonFrame::Object(ObjectState::KeyOrEnd));
            }
            Some(b't') => self.parse_literal(b"true")?,
            Some(b'f') => self.parse_literal(b"false")?,
            Some(b'n') => self.parse_literal(b"null")?,
            Some(b'-' | b'0'..=b'9') => self.parse_number()?,
            _ => return Err(()),
        }
        Ok(())
    }

    /// Skip one JSON value without recursive calls, so Go-accepted depths do
    /// not consume the Rust call stack. The returned range excludes trailing
    /// whitespace and leaves the cursor at the next delimiter.
    fn skip_value(&mut self) -> Result<Range<usize>, ()> {
        self.skip_ws();
        let start = self.position;
        let mut frames = Vec::new();
        self.start_value(&mut frames)?;
        loop {
            let Some(frame) = frames.last().copied() else {
                return Ok(start..self.position);
            };
            let index = frames.len() - 1;
            match frame {
                JsonFrame::Array(ArrayState::ValueOrEnd) => {
                    self.skip_ws();
                    if self.current() == Some(b']') {
                        self.position += 1;
                        frames.pop();
                    } else {
                        frames[index] = JsonFrame::Array(ArrayState::CommaOrEnd);
                        self.start_value(&mut frames)?;
                    }
                }
                JsonFrame::Array(ArrayState::Value) => {
                    self.skip_ws();
                    if self.current() == Some(b']') {
                        return Err(());
                    }
                    frames[index] = JsonFrame::Array(ArrayState::CommaOrEnd);
                    self.start_value(&mut frames)?;
                }
                JsonFrame::Array(ArrayState::CommaOrEnd) => {
                    self.skip_ws();
                    match self.current() {
                        Some(b',') => {
                            self.position += 1;
                            frames[index] = JsonFrame::Array(ArrayState::Value);
                        }
                        Some(b']') => {
                            self.position += 1;
                            frames.pop();
                        }
                        _ => return Err(()),
                    }
                }
                JsonFrame::Object(ObjectState::KeyOrEnd) => {
                    self.skip_ws();
                    if self.current() == Some(b'}') {
                        self.position += 1;
                        frames.pop();
                    } else {
                        self.parse_string()?;
                        self.skip_ws();
                        if self.current() != Some(b':') {
                            return Err(());
                        }
                        self.position += 1;
                        frames[index] = JsonFrame::Object(ObjectState::Value);
                    }
                }
                JsonFrame::Object(ObjectState::Key) => {
                    self.skip_ws();
                    if self.current() == Some(b'}') {
                        return Err(());
                    }
                    self.parse_string()?;
                    self.skip_ws();
                    if self.current() != Some(b':') {
                        return Err(());
                    }
                    self.position += 1;
                    frames[index] = JsonFrame::Object(ObjectState::Value);
                }
                JsonFrame::Object(ObjectState::Value) => {
                    frames[index] = JsonFrame::Object(ObjectState::CommaOrEnd);
                    self.start_value(&mut frames)?;
                }
                JsonFrame::Object(ObjectState::CommaOrEnd) => {
                    self.skip_ws();
                    match self.current() {
                        Some(b',') => {
                            self.position += 1;
                            frames[index] = JsonFrame::Object(ObjectState::Key);
                        }
                        Some(b'}') => {
                            self.position += 1;
                            frames.pop();
                        }
                        _ => return Err(()),
                    }
                }
            }
        }
    }

    fn at_end(&mut self) -> bool {
        self.skip_ws();
        self.position == self.input.len()
    }
}

fn is_value_delimiter(byte: Option<u8>) -> bool {
    byte.is_none_or(|value| matches!(value, b' ' | b'\n' | b'\r' | b'\t' | b',' | b']' | b'}'))
}

fn decode_go_profile_json(input: &[u8]) -> Result<Profile, ()> {
    let normalized = normalize_go_json(input)?;
    let reduced = reduce_profile_json(&normalized)?;
    serde_json::from_slice(&reduced).map_err(|_| ())
}

/// Remove unknown values before serde sees them. Go ignores unknown object
/// fields, so this preserves profile semantics while allowing arbitrary valid
/// unknown JSON nesting up to Go's bound without recursive Rust deserialization.
fn reduce_profile_json(input: &[u8]) -> Result<Vec<u8>, ()> {
    let mut cursor = JsonCursor::new(input);
    cursor.skip_ws();
    let value = if cursor.current() == Some(b'n') {
        let range = cursor.skip_value()?;
        if input.get(range.clone()) != Some(b"null") {
            return Err(());
        }
        input[range].to_vec()
    } else if cursor.current() == Some(b'{') {
        reduce_object(&mut cursor, false)?
    } else {
        return Err(());
    };
    if !cursor.at_end() {
        return Err(());
    }
    Ok(value)
}

fn reduce_object(cursor: &mut JsonCursor<'_>, address: bool) -> Result<Vec<u8>, ()> {
    if cursor.current() != Some(b'{') {
        return Err(());
    }
    cursor.position += 1;
    cursor.skip_ws();
    if cursor.current() == Some(b'}') {
        cursor.position += 1;
        return Ok(b"{}".to_vec());
    }
    let mut output = vec![b'{'];
    let mut has_field = false;
    loop {
        cursor.skip_ws();
        let key_range = cursor.parse_string()?;
        let key: String =
            serde_json::from_slice(&cursor.input[key_range.clone()]).map_err(|_| ())?;
        cursor.skip_ws();
        if cursor.current() != Some(b':') {
            return Err(());
        }
        cursor.position += 1;
        if let Some(kind) = profile_field_kind(&key, address) {
            let value = if matches!(kind, ProfileFieldKind::Addresses) {
                reduce_address_array(cursor)?
            } else {
                let range = cursor.skip_value()?;
                let raw = &cursor.input[range];
                if !validate_simple_value(raw, kind) {
                    return Err(());
                }
                raw.to_vec()
            };
            if has_field {
                output.push(b',');
            }
            output.extend_from_slice(&cursor.input[key_range]);
            output.push(b':');
            output.extend_from_slice(&value);
            has_field = true;
        } else {
            cursor.skip_value()?;
        }
        cursor.skip_ws();
        match cursor.current() {
            Some(b',') => {
                cursor.position += 1;
                cursor.skip_ws();
                if cursor.current() == Some(b'}') {
                    return Err(());
                }
            }
            Some(b'}') => {
                cursor.position += 1;
                output.push(b'}');
                return Ok(output);
            }
            _ => return Err(()),
        }
    }
}

fn reduce_address_array(cursor: &mut JsonCursor<'_>) -> Result<Vec<u8>, ()> {
    cursor.skip_ws();
    if cursor.current() == Some(b'n') {
        let range = cursor.skip_value()?;
        if cursor.input.get(range) == Some(b"null") {
            return Ok(b"null".to_vec());
        }
        return Err(());
    }
    if cursor.current() != Some(b'[') {
        return Err(());
    }
    cursor.position += 1;
    cursor.skip_ws();
    if cursor.current() == Some(b']') {
        cursor.position += 1;
        return Ok(b"[]".to_vec());
    }
    let mut output = vec![b'['];
    let mut first = true;
    loop {
        cursor.skip_ws();
        if !first {
            output.push(b',');
        }
        match cursor.current() {
            Some(b'{') => output.extend_from_slice(&reduce_object(cursor, true)?),
            Some(b'n') => {
                let range = cursor.skip_value()?;
                if cursor.input.get(range) != Some(b"null") {
                    return Err(());
                }
                output.extend_from_slice(b"null");
            }
            _ => return Err(()),
        }
        first = false;
        cursor.skip_ws();
        match cursor.current() {
            Some(b',') => {
                cursor.position += 1;
                cursor.skip_ws();
                if cursor.current() == Some(b']') {
                    return Err(());
                }
            }
            Some(b']') => {
                cursor.position += 1;
                output.push(b']');
                return Ok(output);
            }
            _ => return Err(()),
        }
    }
}

fn validate_simple_value(input: &[u8], kind: ProfileFieldKind) -> bool {
    let mut cursor = JsonCursor::new(input);
    cursor.skip_ws();
    let valid = match kind {
        ProfileFieldKind::Scalar | ProfileFieldKind::Optional => {
            if cursor.current() == Some(b'"') {
                cursor.parse_string().is_ok()
            } else if cursor.current() == Some(b'n') {
                cursor
                    .skip_value()
                    .is_ok_and(|range| input.get(range) == Some(b"null"))
            } else {
                false
            }
        }
        ProfileFieldKind::Strings => {
            if cursor.current() == Some(b'n') {
                cursor
                    .skip_value()
                    .is_ok_and(|range| input.get(range) == Some(b"null"))
            } else if cursor.current() != Some(b'[') {
                false
            } else {
                cursor.position += 1;
                cursor.skip_ws();
                if cursor.current() == Some(b']') {
                    cursor.position += 1;
                    cursor.skip_ws();
                    return cursor.at_end();
                }
                let mut valid = true;
                loop {
                    cursor.skip_ws();
                    if cursor.current() == Some(b'"') {
                        if cursor.parse_string().is_err() {
                            valid = false;
                            break;
                        }
                    } else if cursor.current() == Some(b'n') {
                        let Ok(range) = cursor.skip_value() else {
                            valid = false;
                            break;
                        };
                        if input.get(range) != Some(b"null") {
                            valid = false;
                            break;
                        }
                    } else {
                        valid = false;
                        break;
                    }
                    cursor.skip_ws();
                    match cursor.current() {
                        Some(b',') => {
                            cursor.position += 1;
                            cursor.skip_ws();
                            if cursor.current() == Some(b']') {
                                valid = false;
                                break;
                            }
                        }
                        Some(b']') => {
                            cursor.position += 1;
                            break;
                        }
                        _ => {
                            valid = false;
                            break;
                        }
                    }
                }
                valid && cursor.at_end()
            }
        }
        ProfileFieldKind::Addresses => false,
    };
    valid && cursor.at_end()
}

#[derive(Debug, Eq, PartialEq)]
pub enum ProfileError {
    NotFound,
    Stat,
    Read,
    NoSeparator,
    Header,
    LegacyV0,
    Key(MasterKeyError),
    Nonce,
    Authentication,
    Json,
    Mkdir,
    Write,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFound => "identity: profile not found",
            Self::Stat => "identity: stat profile failed",
            Self::Read => "identity: read profile failed",
            Self::NoSeparator => "identity: profile corrupt: no header separator",
            Self::Header => "identity: profile corrupt: header",
            Self::LegacyV0 => "identity: legacy v0 profile (no AAD) is no longer supported",
            Self::Key(error) => return error.fmt(f),
            Self::Nonce => "identity: profile corrupt: nonce",
            Self::Authentication => {
                "identity: profile corrupt: cipher: message authentication failed"
            }
            Self::Json => "identity: profile corrupt",
            Self::Mkdir => "identity: mkdir failed",
            Self::Write => "identity: write profile failed",
        })
    }
}
impl std::error::Error for ProfileError {}

/// Explicit environment/home snapshot for profile discovery. No filesystem
/// operations occur during construction. Tests need not mutate process globals.
#[derive(Clone, Default)]
pub struct ProfilePaths {
    home: Option<PathBuf>,
    environment: BTreeMap<String, String>,
}

impl ProfilePaths {
    pub fn new(home: Option<PathBuf>, environment: BTreeMap<String, String>) -> Self {
        Self { home, environment }
    }

    /// Process adapter, equivalent to Go's `os.UserHomeDir` and path overrides.
    pub fn from_process() -> Self {
        let home_variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let home = std::env::var_os(home_variable)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let environment = [
            "SYMERASEME_IDENTITY_PATH",
            "SYMERASEME_DATA_DIR",
            "SYMERASEME_CONFIG_DIR",
        ]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok().map(|v| (name.to_owned(), v)))
        .collect();
        Self::new(home, environment)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.environment
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    fn expand(&self, path: &Path) -> PathBuf {
        if let Some(home) = &self.home {
            if path == Path::new("~") {
                return home.clone();
            }
            if let Some(value) = path.to_str().and_then(|v| v.strip_prefix("~/")) {
                return home.join(value);
            }
        }
        path.to_owned()
    }

    /// Explicit path, identity override, data override, then config directory.
    /// The write path never falls back to the historical basename.
    pub fn resolve_write(&self, path: &Path) -> PathBuf {
        if !path.as_os_str().is_empty() {
            self.expand(path)
        } else if let Some(value) = self.value("SYMERASEME_IDENTITY_PATH") {
            self.expand(Path::new(value))
        } else {
            let directory = self
                .value("SYMERASEME_DATA_DIR")
                .or_else(|| self.value("SYMERASEME_CONFIG_DIR"))
                .unwrap_or("~/.config/symeraseme");
            self.expand(Path::new(directory)).join("identity.encrypted")
        }
    }

    /// Explicit path, identity override, data override, then config directory.
    /// Only the two historical identity basenames are eligible for fallback.
    pub fn resolve(&self, path: &Path) -> PathBuf {
        let target = self.resolve_write(path);
        // Do not fallback from permission or other I/O failures to a different
        // identity. This fail-closed distinction is intentional.
        if let Err(error) = std::fs::metadata(&target)
            && error.kind() == std::io::ErrorKind::NotFound
        {
            let alternative = match target.file_name().and_then(|v| v.to_str()) {
                Some("identity.encrypted") => Some("identity.enc"),
                Some("identity.enc") => Some("identity.encrypted"),
                _ => None,
            };
            if let Some(name) = alternative {
                let alternate = target.with_file_name(name);
                if std::fs::metadata(&alternate).is_ok() {
                    return alternate;
                }
            }
        }
        target
    }
}

/// Go-compatible existence probe (directories also exist); does not resolve keys.
pub fn profile_exists(path: &Path, paths: &ProfilePaths) -> bool {
    std::fs::metadata(paths.resolve(path)).is_ok()
}

/// Authenticate and decrypt a raw profile envelope with an explicit key.
pub fn decrypt_profile_with_key(
    raw: &[u8],
    key: &[u8],
) -> Result<(Vec<u8>, Envelope), ProfileError> {
    if key.len() != 32 {
        return Err(ProfileError::Key(MasterKeyError::InvalidLength {
            source: "master key",
            actual: key.len(),
        }));
    }
    let separator = raw
        .iter()
        .position(|&byte| byte == b'\n')
        .ok_or(ProfileError::NoSeparator)?;
    let header_bytes = &raw[..separator];
    let header: Envelope =
        serde_json::from_slice(header_bytes).map_err(|_| ProfileError::Header)?;
    if header.version == 0 {
        return Err(ProfileError::LegacyV0);
    }
    let nonce = hex::decode(&header.nonce).map_err(|_| ProfileError::Nonce)?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| ProfileError::Nonce)?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| ProfileError::Authentication)?;
    let plaintext = cipher
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &raw[separator + 1..],
                aad: header_bytes,
            },
        )
        .map_err(|_| ProfileError::Authentication)?;
    Ok((plaintext, header))
}

/// Encrypt plaintext into a version-2 AES-256-GCM envelope with an explicit nonce.
fn encrypt_profile_with_nonce(
    plaintext: &[u8],
    key: &[u8],
    nonce: &[u8; 12],
) -> Result<Vec<u8>, ProfileError> {
    if key.len() != 32 {
        return Err(ProfileError::Key(MasterKeyError::InvalidLength {
            source: "master key",
            actual: key.len(),
        }));
    }
    let header = Envelope {
        version: 2,
        nonce: hex::encode(nonce),
        algorithm: "AES-256-GCM".to_owned(),
    };
    let header_bytes = serde_json::to_vec(&header).map_err(|_| ProfileError::Header)?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| ProfileError::Authentication)?;
    let ciphertext = cipher
        .encrypt(
            &Nonce::from(*nonce),
            Payload {
                msg: plaintext,
                aad: &header_bytes,
            },
        )
        .map_err(|_| ProfileError::Authentication)?;
    let mut output = Vec::with_capacity(header_bytes.len() + 1 + ciphertext.len());
    output.extend_from_slice(&header_bytes);
    output.push(b'\n');
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Encrypt plaintext into a version-2 AES-256-GCM envelope using the OS CSPRNG.
pub fn encrypt_profile(plaintext: &[u8], key: &[u8]) -> Result<Vec<u8>, ProfileError> {
    use rand::Rng;

    let mut nonce = [0_u8; 12];
    rand::rng().fill_bytes(&mut nonce);
    encrypt_profile_with_nonce(plaintext, key, &nonce)
}

fn write_canonical_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\x08' => output.push_str("\\b"),
            '\x0c' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character < ' ' || character == '\u{7f}' => {
                use std::fmt::Write;
                write!(output, "\\u{:04x}", character as u32).unwrap();
            }
            character if character.is_ascii() => output.push(character),
            character if (character as u32) <= 0xffff => {
                use std::fmt::Write;
                write!(output, "\\u{:04x}", character as u32).unwrap();
            }
            character => {
                use std::fmt::Write;
                let value = character as u32 - 0x1_0000;
                write!(
                    output,
                    "\\u{:04x}\\u{:04x}",
                    0xd800 + (value >> 10),
                    0xdc00 + (value & 0x3ff)
                )
                .unwrap();
            }
        }
    }
    output.push('"');
}

fn write_canonical_number(output: &mut String, value: &serde_json::Number) {
    let Some(value) = value.as_f64() else {
        output.push_str(&value.to_string());
        return;
    };
    if value.is_finite() {
        if value.fract() == 0.0 {
            output.push_str(&(value as i64).to_string());
        } else if value.abs() >= 1_000_000.0 || value.abs() < 0.0001 {
            let scientific = format!("{value:e}");
            let (mantissa, exponent) = scientific
                .split_once('e')
                .expect("scientific formatting includes an exponent");
            output.push_str(mantissa);
            output.push('e');
            let exponent: i32 = exponent.parse().expect("Rust exponent is an integer");
            if exponent >= 0 {
                output.push('+');
            } else {
                output.push('-');
            }
            use std::fmt::Write;
            write!(output, "{:02}", exponent.unsigned_abs()).unwrap();
        } else {
            output.push_str(&value.to_string());
        }
    } else {
        output.push_str(&value.to_string());
    }
}

fn write_canonical_json(output: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::Null => output.push_str("null"),
        serde_json::Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        serde_json::Value::Number(value) => write_canonical_number(output, value),
        serde_json::Value::String(value) => write_canonical_string(output, value),
        serde_json::Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                write_canonical_json(output, value);
            }
            output.push(']');
        }
        serde_json::Value::Object(values) => {
            output.push('{');
            let mut keys: Vec<&String> = values.keys().collect();
            keys.sort();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                write_canonical_string(output, key);
                output.push_str(": ");
                write_canonical_json(output, &values[key]);
            }
            output.push('}');
        }
    }
}

/// Serialize arbitrary JSON using Go/Python-compatible sorted-key JSON.
pub fn canonical_generic_json(value: &serde_json::Value) -> String {
    let mut output = String::new();
    write_canonical_json(&mut output, value);
    output
}

#[derive(Serialize)]
struct ProfilePayload<'a> {
    full_name: &'a str,
    name_variants: &'a [String],
    date_of_birth: &'a Option<String>,
    addresses: &'a [ProfileAddress],
    email_addresses: &'a [String],
    phone_numbers: &'a [String],
    jurisdictions: &'a [String],
}

/// Serialize a profile in the canonical form used for Go audit hashes.
pub fn canonical_json(profile: &Profile) -> String {
    let payload = ProfilePayload {
        full_name: &profile.full_name,
        name_variants: &profile.name_variants,
        date_of_birth: &profile.date_of_birth,
        addresses: &profile.addresses,
        email_addresses: &profile.email_addresses,
        phone_numbers: &profile.phone_numbers,
        jurisdictions: &profile.jurisdictions,
    };
    let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    canonical_generic_json(&value)
}

/// Return the lowercase SHA-256 digest of a profile's canonical JSON.
pub fn hash_profile(profile: &Profile) -> String {
    hex::encode(Sha256::digest(canonical_json(profile).as_bytes()))
}

/// Read and authenticate a profile with existing key sources only.
///
/// Empty `path` selects discovery. This read primitive deliberately re-reads on
/// each call; callers retaining a profile own its lifetime instead of a global
/// plaintext cache. Parser/I/O errors are redacted, not raw Go JSON/OS excerpts.
pub fn load_profile<K: KeyringBackend>(
    path: &Path,
    paths: &ProfilePaths,
    keys: &mut MasterKeyResolver<K>,
) -> Result<Profile, ProfileError> {
    let target = paths.resolve(path);
    let metadata = std::fs::metadata(&target).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProfileError::NotFound
        } else {
            ProfileError::Stat
        }
    })?;
    // Avoid opening blocking special files. Directories report the read error
    // they produce in Go without ever attempting key resolution.
    if !metadata.is_file() {
        return Err(ProfileError::Read);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A regular file swapped for a FIFO between stat/open must not block.
        options.custom_flags(nix::libc::O_NONBLOCK);
    }
    let mut file = options.open(target).map_err(|_| ProfileError::Read)?;
    if !file.metadata().map_err(|_| ProfileError::Read)?.is_file() {
        return Err(ProfileError::Read);
    }
    let mut raw = Vec::new();
    file.read_to_end(&mut raw).map_err(|_| ProfileError::Read)?;
    let separator = raw
        .iter()
        .position(|&v| v == b'\n')
        .ok_or(ProfileError::NoSeparator)?;
    let header_bytes = &raw[..separator];
    let header: Envelope =
        serde_json::from_slice(header_bytes).map_err(|_| ProfileError::Header)?;
    if header.version == 0 {
        return Err(ProfileError::LegacyV0);
    }
    // Go intentionally does not dispatch on algorithm or reject nonzero
    // versions: the original header bytes are authenticated without rewriting.
    let key = keys.resolve_existing().map_err(ProfileError::Key)?;
    let nonce = hex::decode(header.nonce).map_err(|_| ProfileError::Nonce)?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| ProfileError::Nonce)?;
    let cipher =
        Aes256Gcm::new_from_slice(key.as_bytes()).map_err(|_| ProfileError::Authentication)?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: &raw[separator + 1..],
                    aad: header_bytes,
                },
            )
            .map_err(|_| ProfileError::Authentication)?,
    );
    decode_go_profile_json(&plain).map_err(|_| ProfileError::Json)
}

/// Serialize the Go profile plaintext: every field present, empty lists as `[]`.
fn profile_plaintext(profile: &Profile) -> Result<Vec<u8>, ProfileError> {
    let payload = ProfilePayload {
        full_name: &profile.full_name,
        name_variants: &profile.name_variants,
        date_of_birth: &profile.date_of_birth,
        addresses: &profile.addresses,
        email_addresses: &profile.email_addresses,
        phone_numbers: &profile.phone_numbers,
        jurisdictions: &profile.jurisdictions,
    };
    serde_json::to_vec_pretty(&payload).map_err(|_| ProfileError::Write)
}

/// Encrypt and durably write a profile with an already-initialized key.
///
/// The write is a same-directory temporary file with mode 0600, fsync, atomic
/// rename and a directory fsync, so a crash never truncates an existing profile.
pub fn save_profile<K: KeyringBackend>(
    profile: &Profile,
    path: &Path,
    paths: &ProfilePaths,
    keys: &mut MasterKeyResolver<K>,
) -> Result<PathBuf, ProfileError> {
    let target = paths.resolve_write(path);
    let directory = target.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(directory).map_err(|_| ProfileError::Mkdir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700));
    }
    let key = keys.resolve_existing().map_err(ProfileError::Key)?;
    let plaintext = Zeroizing::new(profile_plaintext(profile)?);
    let encrypted = encrypt_profile(&plaintext, key.as_bytes())?;

    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ProfileError::Write)?;
    let temporary = directory.join(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = options.open(&temporary)?;
        file.write_all(&encrypted)?;
        file.sync_all()
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(ProfileError::Write);
    }
    if std::fs::rename(&temporary, &target).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(ProfileError::Write);
    }
    if let Ok(handle) = std::fs::File::open(directory) {
        let _ = handle.sync_all();
    }
    Ok(target)
}

/// Initialize the master key, then save the profile.
///
/// A key minted by this call is rolled back when the save fails, so a failed
/// initialization never strands an existing profile behind an unrelated key.
pub fn init_profile<K: KeyringBackend>(
    profile: &Profile,
    path: &Path,
    paths: &ProfilePaths,
    keys: &mut MasterKeyResolver<K>,
) -> Result<PathBuf, ProfileError> {
    let key_existed = keys.resolve_existing().is_ok();
    keys.init().map_err(ProfileError::Key)?;
    match save_profile(profile, path, paths, keys) {
        Ok(target) => Ok(target),
        Err(error) => {
            if !key_existed {
                let _ = keys.delete();
            }
            Err(error)
        }
    }
}
