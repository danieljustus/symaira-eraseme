use regex::RegexBuilder as TextRegexBuilder;
use regex::bytes::{Regex as ByteRegex, RegexBuilder};
use std::fmt;
use std::sync::OnceLock;

pub const MAX_INPUT_BYTES: usize = 16 << 20;
pub const MAX_MATCHES: usize = 100_000;
pub const MAX_OUTPUT_BYTES: usize = 32 << 20;
pub const MAX_PROFILE_LITERAL_BYTES: usize = 16 << 10;
pub const MAX_PROFILE_LITERAL_COUNT: usize = 4_096;
pub const MAX_PROFILE_TOTAL_BYTES: usize = 1 << 20;
pub const MAX_PROFILE_ADDRESSES: usize = 4_096;
pub const MAX_PROFILE_VECTOR_ENTRIES: usize = 4_096;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Address {
    pub street: String,
    pub city: String,
    pub postal_code: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RedactionProfile {
    pub full_name: String,
    pub name_variants: Vec<String>,
    pub email_addresses: Vec<String>,
    pub phone_numbers: Vec<String>,
    pub addresses: Vec<Address>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedactionError {
    InputTooLarge,
    MatchLimit,
    OutputTooLarge,
    InvalidProfileLiteral,
    InvalidMatchRange,
}

impl fmt::Display for RedactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InputTooLarge => "redaction input exceeds the configured limit",
            Self::MatchLimit => "redaction match limit exceeded",
            Self::OutputTooLarge => "redaction output exceeds the configured limit",
            Self::InvalidProfileLiteral => "redaction profile literal is invalid",
            Self::InvalidMatchRange => "redaction match range is invalid",
        })
    }
}
impl std::error::Error for RedactionError {}

#[derive(Clone, Debug)]
pub struct Rule {
    pub name: &'static str,
    regex: ByteRegex,
    replacement: Replacement,
}

#[derive(Clone, Debug)]
enum Replacement {
    Email,
    Phone,
    Ssn,
    Iban,
    GermanId,
    FrenchId,
    SpanishId,
    Passport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub value: Vec<u8>,
    replacement: Vec<u8>,
}

impl Match {
    pub fn new(
        name: impl Into<String>,
        start: usize,
        end: usize,
        value: Vec<u8>,
        replacement: Vec<u8>,
    ) -> Self {
        Self {
            name: name.into(),
            start,
            end,
            value,
            replacement,
        }
    }

    pub fn replacement(&self) -> &[u8] {
        &self.replacement
    }
}

fn regex(pattern: &str) -> ByteRegex {
    RegexBuilder::new(pattern)
        .unicode(false)
        .build()
        .expect("built-in redaction regex")
}

fn passport_regex() -> &'static ByteRegex {
    static REGEX: OnceLock<ByteRegex> = OnceLock::new();
    REGEX.get_or_init(|| {
        regex(
            r"(?i)(passport|travel\s*document|reisedokument)\s*(#|no|num|number)?\s*[:.]?\s*([A-Z0-9]{6,9})\b",
        )
    })
}

fn default_rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "IBAN",
            regex: regex(r"\b[A-Z]{2}[0-9]{2}[A-Z0-9]{11,30}\b"),
            replacement: Replacement::Iban,
        },
        Rule {
            name: "German ID",
            regex: regex(r"\b[A-L][0-9]{8}[A-Z]?\b"),
            replacement: Replacement::GermanId,
        },
        Rule {
            name: "French ID",
            regex: regex(r"\b[12][0-9]{2}(0[1-9]|1[0-2])[0-9]{5}[0-9]{3}([0-9]{2})?\b"),
            replacement: Replacement::FrenchId,
        },
        Rule {
            name: "Spanish ID",
            regex: regex(r"\b[0-9]{8}[A-HJ-NP-TV-Z]\b"),
            replacement: Replacement::SpanishId,
        },
        Rule {
            name: "Passport",
            regex: passport_regex().clone(),
            replacement: Replacement::Passport,
        },
        Rule {
            name: "SSN",
            regex: regex(r"\b[0-9]{3}[- ]?[0-9]{2}[- ]?[0-9]{4}\b"),
            replacement: Replacement::Ssn,
        },
        Rule {
            name: "Email",
            regex: regex(
                r"[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]{1,64}@[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?){0,130}",
            ),
            replacement: Replacement::Email,
        },
        Rule {
            name: "Phone",
            regex: regex(r"(\+?1[\s.-]?)?\(?[0-9]{3}\)?[\s.-]?[0-9]{3}[\s.-]?[0-9]{4}"),
            replacement: Replacement::Phone,
        },
    ]
}

fn builtin_rules() -> &'static Vec<Rule> {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(default_rules)
}

pub fn rules() -> Vec<Rule> {
    builtin_rules().clone()
}

fn validate_profile(profile: &RedactionProfile) -> Result<(), RedactionError> {
    if profile.name_variants.len() > MAX_PROFILE_VECTOR_ENTRIES
        || profile.email_addresses.len() > MAX_PROFILE_VECTOR_ENTRIES
        || profile.phone_numbers.len() > MAX_PROFILE_VECTOR_ENTRIES
        || profile.addresses.len() > MAX_PROFILE_ADDRESSES
    {
        return Err(RedactionError::InvalidProfileLiteral);
    }
    let mut count = 0usize;
    let mut total = 0usize;
    let mut check = |value: &str| -> Result<(), RedactionError> {
        if value.is_empty() {
            return Ok(());
        }
        if value.len() > MAX_PROFILE_LITERAL_BYTES || value.as_bytes().contains(&0) {
            return Err(RedactionError::InvalidProfileLiteral);
        }
        count = count
            .checked_add(1)
            .ok_or(RedactionError::InvalidProfileLiteral)?;
        total = total
            .checked_add(value.len())
            .ok_or(RedactionError::InvalidProfileLiteral)?;
        if count > MAX_PROFILE_LITERAL_COUNT || total > MAX_PROFILE_TOTAL_BYTES {
            return Err(RedactionError::InvalidProfileLiteral);
        }
        Ok(())
    };
    check(&profile.full_name)?;
    for value in &profile.name_variants {
        check(value)?;
    }
    for value in &profile.email_addresses {
        check(value)?;
    }
    for value in &profile.phone_numbers {
        check(value)?;
    }
    for address in &profile.addresses {
        check(&address.street)?;
        check(&address.city)?;
        check(&address.postal_code)?;
    }
    Ok(())
}
pub fn collect_matches(
    input: &[u8],
    profile: Option<&RedactionProfile>,
) -> Result<Vec<Match>, RedactionError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(RedactionError::InputTooLarge);
    }
    if let Some(profile) = profile {
        validate_profile(profile)?;
    }
    let mut found = Vec::new();
    if let Some(profile) = profile {
        for value in &profile.email_addresses {
            append_literal(
                &mut found,
                input,
                value,
                "Profile Email",
                b"[REDACTED-EMAIL]",
            )?;
        }
        for value in &profile.phone_numbers {
            append_literal(
                &mut found,
                input,
                value,
                "Profile Phone",
                b"[REDACTED-PHONE]",
            )?;
        }
        append_literal(
            &mut found,
            input,
            &profile.full_name,
            "Profile Name",
            b"[REDACTED-NAME]",
        )?;
        for value in &profile.name_variants {
            append_literal(&mut found, input, value, "Profile Name", b"[REDACTED-NAME]")?;
        }
        for address in &profile.addresses {
            append_literal(
                &mut found,
                input,
                &address.street,
                "Profile Street",
                b"[REDACTED-STREET]",
            )?;
            append_literal(
                &mut found,
                input,
                &address.city,
                "Profile City",
                b"[REDACTED-CITY]",
            )?;
            append_literal(
                &mut found,
                input,
                &address.postal_code,
                "Profile Postal Code",
                b"[REDACTED-POSTAL]",
            )?;
        }
    }
    for rule in builtin_rules() {
        for capture in rule.regex.find_iter(input) {
            let value = capture.as_bytes();
            if rule.name == "SSN" && invalid_ssn(value) {
                continue;
            }
            if rule.name == "Email" && !valid_email(value) {
                continue;
            }
            let replacement = replace(&rule.replacement, value);
            push_match(
                &mut found,
                Match {
                    name: rule.name.to_owned(),
                    start: capture.start(),
                    end: capture.end(),
                    value: value.to_vec(),
                    replacement,
                },
            )?;
        }
    }
    found.sort_by(|a, b| {
        a.start.cmp(&b.start).then_with(|| {
            b.end
                .saturating_sub(b.start)
                .cmp(&a.end.saturating_sub(a.start))
        })
    });
    let mut accepted = Vec::with_capacity(found.len());
    let mut last_end = 0;
    for candidate in found {
        if accepted.is_empty() || candidate.start >= last_end {
            last_end = candidate.end;
            accepted.push(candidate);
        }
    }
    Ok(accepted)
}

fn append_literal(
    out: &mut Vec<Match>,
    input: &[u8],
    value: &str,
    name: &'static str,
    replacement: &[u8],
) -> Result<(), RedactionError> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > MAX_PROFILE_LITERAL_BYTES || value.as_bytes().contains(&0) {
        return Err(RedactionError::InvalidProfileLiteral);
    }
    if value.is_ascii() {
        let escaped = regex::escape(value);
        let pattern = RegexBuilder::new(&escaped)
            .case_insensitive(true)
            .unicode(false)
            .build()
            .map_err(|_| RedactionError::InvalidProfileLiteral)?;
        for capture in pattern.find_iter(input) {
            push_match(
                out,
                Match {
                    name: name.to_owned(),
                    start: capture.start(),
                    end: capture.end(),
                    value: capture.as_bytes().to_vec(),
                    replacement: replacement.to_vec(),
                },
            )?;
        }
        return Ok(());
    }
    let escaped = regex::escape(value);
    let pattern = TextRegexBuilder::new(&escaped)
        .case_insensitive(true)
        .build()
        .map_err(|_| RedactionError::InvalidProfileLiteral)?;
    let mut offset = 0;
    while offset < input.len() {
        let remainder = &input[offset..];
        let (valid_len, invalid_len) = match std::str::from_utf8(remainder) {
            Ok(text) => {
                append_text_literal_matches(out, input, offset, text, &pattern, name, replacement)?;
                break;
            }
            Err(error) => (
                error.valid_up_to(),
                error
                    .error_len()
                    .unwrap_or(remainder.len().saturating_sub(error.valid_up_to())),
            ),
        };
        if valid_len > 0 {
            let text = std::str::from_utf8(&remainder[..valid_len])
                .map_err(|_| RedactionError::InvalidProfileLiteral)?;
            append_text_literal_matches(out, input, offset, text, &pattern, name, replacement)?;
        }
        let advance = valid_len.saturating_add(invalid_len.max(1));
        offset = offset.saturating_add(advance).min(input.len());
    }
    Ok(())
}

fn append_text_literal_matches(
    out: &mut Vec<Match>,
    input: &[u8],
    offset: usize,
    text: &str,
    pattern: &regex::Regex,
    name: &'static str,
    replacement: &[u8],
) -> Result<(), RedactionError> {
    for capture in pattern.find_iter(text) {
        let start = offset + capture.start();
        let end = offset + capture.end();
        push_match(
            out,
            Match {
                name: name.to_owned(),
                start,
                end,
                value: input[start..end].to_vec(),
                replacement: replacement.to_vec(),
            },
        )?;
    }
    Ok(())
}

fn push_match(out: &mut Vec<Match>, value: Match) -> Result<(), RedactionError> {
    if out.len() >= MAX_MATCHES {
        return Err(RedactionError::MatchLimit);
    }
    out.push(value);
    Ok(())
}

fn invalid_ssn(value: &[u8]) -> bool {
    let digits: Vec<u8> = value.iter().copied().filter(u8::is_ascii_digit).collect();
    digits.len() == 9
        && (digits.starts_with(b"000")
            || digits.starts_with(b"666")
            || digits[0] == b'9'
            || digits[3..5] == *b"00"
            || digits[5..] == *b"0000")
}

fn valid_email(value: &[u8]) -> bool {
    let Some(at) = value.iter().position(|byte| *byte == b'@') else {
        return false;
    };
    if at == 0 || at + 1 >= value.len() || value[at + 1..].contains(&b'@') {
        return false;
    }
    let labels = value[at + 1..]
        .split(|byte| *byte == b'.')
        .collect::<Vec<_>>();
    labels.len() <= 127
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label[0] != b'-'
                && label[label.len() - 1] != b'-'
        })
}

fn replace(kind: &Replacement, value: &[u8]) -> Vec<u8> {
    match kind {
        Replacement::Ssn => b"***-**-****".to_vec(),
        Replacement::Email => scrub_email(value),
        Replacement::Phone => scrub_phone(value),
        Replacement::Iban => mask_parts(value, 2, 4, b"**"),
        Replacement::GermanId => suffix_mask(value, 2, 7),
        Replacement::FrenchId => suffix_mask(value, 3, 3),
        Replacement::SpanishId => {
            let mut out = b"****-****-".to_vec();
            out.extend_from_slice(&[value.last().unwrap_or(&b'?').to_ascii_uppercase()]);
            out
        }
        Replacement::Passport => scrub_passport(value),
    }
}
fn suffix_mask(value: &[u8], suffix: usize, stars: usize) -> Vec<u8> {
    if value.len() < suffix {
        return value.to_vec();
    }
    let mut out = vec![b'*'; stars];
    out.extend_from_slice(&value[value.len() - suffix..]);
    out
}
fn mask_parts(value: &[u8], prefix: usize, suffix: usize, middle: &[u8]) -> Vec<u8> {
    if value.len() < prefix + suffix {
        return value.to_vec();
    }
    let mut out = value[..prefix].to_vec();
    out.extend(std::iter::repeat_n(
        b'*',
        value.len() - suffix + middle.len(),
    ));
    out.extend_from_slice(&value[value.len() - suffix..]);
    out
}
fn scrub_email(value: &[u8]) -> Vec<u8> {
    let Some(at) = value.iter().position(|byte| *byte == b'@') else {
        return value.to_vec();
    };
    if at == 0 {
        return value.to_vec();
    }
    let mut out = vec![value[0]];
    if at > 2 {
        out.extend(std::iter::repeat_n(b'*', at - 2));
        out.push(value[at - 1]);
    }
    out.push(b'@');
    let domain = &value[at + 1..];
    if domain.is_empty() {
        return value.to_vec();
    }
    out.push(domain[0]);
    if let Some(dot) = domain.iter().position(|byte| *byte == b'.') {
        out.push(b'*');
        out.push(b'.');
        out.extend_from_slice(&domain[dot + 1..]);
    } else {
        out.push(b'.');
        out.push(b'*');
    }
    out
}
fn scrub_phone(value: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = value.iter().copied().filter(u8::is_ascii_digit).collect();
    if digits.len() < 4 {
        return value.to_vec();
    }
    let mut out = if digits.len() == 11 {
        b"+1-***-***-".to_vec()
    } else {
        b"***-***-".to_vec()
    };
    out.extend_from_slice(&digits[digits.len() - 4..]);
    out
}
fn scrub_passport(value: &[u8]) -> Vec<u8> {
    let Some(identifier) = passport_regex()
        .captures(value)
        .and_then(|captures| captures.get(3))
    else {
        return value.to_vec();
    };
    if identifier.len() < 2 {
        return value.to_vec();
    }
    let mut out = value.to_vec();
    for byte in &mut out[identifier.start()..identifier.end() - 2] {
        *byte = b'*';
    }
    out
}

pub fn redact_bytes(
    input: &[u8],
    profile: Option<&RedactionProfile>,
) -> Result<Vec<u8>, RedactionError> {
    let matches = collect_matches(input, profile)?;
    let mut output_len = input.len();
    for item in &matches {
        output_len = output_len
            .checked_sub(item.end - item.start)
            .and_then(|n| n.checked_add(item.replacement.len()))
            .ok_or(RedactionError::OutputTooLarge)?;
        if output_len > MAX_OUTPUT_BYTES {
            return Err(RedactionError::OutputTooLarge);
        }
    }
    let mut output = Vec::with_capacity(output_len);
    let mut position = 0;
    for item in matches {
        let Some(prefix) = input.get(position..item.start) else {
            return Err(RedactionError::InvalidMatchRange);
        };
        output.extend_from_slice(prefix);
        output.extend_from_slice(&item.replacement);
        position = item.end;
    }
    output.extend_from_slice(
        input
            .get(position..)
            .ok_or(RedactionError::InvalidMatchRange)?,
    );
    Ok(output)
}

pub fn redact_text(
    input: &str,
    profile: Option<&RedactionProfile>,
) -> Result<String, RedactionError> {
    String::from_utf8(redact_bytes(input.as_bytes(), profile)?)
        .map_err(|_| RedactionError::InvalidMatchRange)
}
