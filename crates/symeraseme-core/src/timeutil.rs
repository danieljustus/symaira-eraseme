//! Timestamp parsing and formatting shared by the event-store port.
//!
//! The accepted input deliberately follows the Go event-store oracle rather
//! than accepting every format supported by a date-time crate.  In particular,
//! timestamps without an offset are UTC wall-clock values.

use chrono::{DateTime, Datelike, Duration, NaiveDateTime, TimeZone, Utc};
use std::fmt;

/// The two observable classes of timestamp parse failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimestampError {
    /// The input was empty after trimming surrounding whitespace.
    Empty,
    /// The input was non-empty but did not match an accepted layout.
    Malformed,
}

/// Short alias for callers that refer to this as a parse error.
pub type ParseError = TimestampError;

impl fmt::Display for TimestampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty timestamp"),
            Self::Malformed => f.write_str("malformed timestamp"),
        }
    }
}

impl std::error::Error for TimestampError {}

/// Parse an accepted timestamp and return its instant in UTC.
///
/// Accepted layouts are the Go oracle's ISO T/space forms with or without an
/// offset, RFC 1123 (numeric or alphabetic zone), RFC 850, and RFC 3339.
/// Go accepts fractional seconds even when a layout omits them; the same
/// extension is retained here.  Alphabetic RFC zones have no portable offset
/// in the Go parser and therefore use its zero-offset result.
pub fn parse(input: &str) -> Result<DateTime<Utc>, TimestampError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(TimestampError::Empty);
    }

    let normalized_fraction = normalize_comma_fraction(input);
    let normalized_z = normalized_fraction
        .strip_suffix('Z')
        .map(|prefix| format!("{prefix}+00:00"));
    let offset_inputs = normalized_z
        .iter()
        .map(String::as_str)
        .chain([normalized_fraction.as_str(), input]);
    for value in offset_inputs {
        for layout in [
            "%Y-%m-%dT%H:%M:%S%.f%:z",
            "%Y-%m-%d %H:%M:%S%.f%:z",
            "%Y-%m-%dT%H:%M:%S%:z",
            "%Y-%m-%d %H:%M:%S%:z",
        ] {
            if let Ok(value) = DateTime::parse_from_str(value, layout) {
                return Ok(value.with_timezone(&Utc));
            }
        }
    }

    for (value, layout) in [
        (normalized_fraction.as_str(), "%Y-%m-%dT%H:%M:%S%.f"),
        (normalized_fraction.as_str(), "%Y-%m-%d %H:%M:%S%.f"),
        (normalized_fraction.as_str(), "%Y-%m-%dT%H:%M:%S"),
        (normalized_fraction.as_str(), "%Y-%m-%d %H:%M:%S"),
    ] {
        if let Ok(value) = NaiveDateTime::parse_from_str(value, layout) {
            return Ok(Utc.from_utc_datetime(&value));
        }
    }

    // RFC1123Z uses a numeric zone; parse the weekday separately so that
    // the day name is validated but not required to agree with the date,
    // matching time.Parse.
    if let Some(value) = parse_rfc1123_numeric(input) {
        return Ok(value);
    }
    if let Some(value) = parse_rfc1123_alpha(input) {
        return Ok(value);
    }

    // RFC850 uses a two-digit year.  Go's pivot is 69: 00..68 map to
    // 2000..2068 and 69..99 map to 1969..1999.
    if let Some(value) = parse_rfc850(input) {
        return Ok(value);
    }

    Err(TimestampError::Malformed)
}

/// Alias using the name used by the event-store contract.
pub fn parse_timestamp(input: &str) -> Result<DateTime<Utc>, TimestampError> {
    parse(input)
}

/// Format a timestamp as canonical UTC ISO text with seconds precision.
pub fn format_iso<Tz: TimeZone>(timestamp: DateTime<Tz>) -> String
where
    Tz::Offset: fmt::Display,
{
    timestamp
        .with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%S+00:00")
        .to_string()
}

/// Format a timestamp as the UTC SQLite wall-clock form with seconds precision.
pub fn format_sql<Tz: TimeZone>(timestamp: DateTime<Tz>) -> String
where
    Tz::Offset: fmt::Display,
{
    timestamp
        .with_timezone(&Utc)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// Return the SQL representation as its exact stored bytes.
pub fn format_sql_bytes<Tz: TimeZone>(timestamp: DateTime<Tz>) -> Vec<u8>
where
    Tz::Offset: fmt::Display,
{
    format_sql(timestamp).into_bytes()
}

fn parse_rfc1123_numeric(input: &str) -> Option<DateTime<Utc>> {
    let (prefix, zone) = input.rsplit_once(' ')?;
    let prefix = without_weekday(rfc1123_prefix(prefix)?)?;
    let value = parse_rfc1123_datetime(prefix)?;
    let zone = parse_numeric_offset(zone)?;
    Some(Utc.from_utc_datetime(&(value - Duration::seconds(zone))))
}

fn parse_rfc1123_alpha(input: &str) -> Option<DateTime<Utc>> {
    let prefix = rfc_prefix(input)?;
    let prefix = without_weekday(rfc1123_prefix(prefix)?)?;
    let value = parse_rfc1123_datetime(prefix)?;
    Some(Utc.from_utc_datetime(&value))
}

fn parse_rfc850(input: &str) -> Option<DateTime<Utc>> {
    let (prefix, zone) = input.rsplit_once(' ')?;
    let prefix = without_weekday(rfc850_prefix(prefix)?)?;
    let value = parse_rfc850_datetime(prefix)?;
    if !valid_rfc850_zone(zone) {
        return None;
    }
    let year = value.year() % 100;
    let year = if year >= 69 { 1900 + year } else { 2000 + year };
    let date = value.date().with_year(year)?;
    Some(Utc.from_utc_datetime(&date.and_time(value.time())))
}

fn valid_rfc850_zone(zone: &str) -> bool {
    if zone.len() == 3
        && zone
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return true;
    }
    let Some(digits) = zone.strip_prefix(['+', '-']) else {
        return false;
    };
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && digits.parse::<u32>().is_ok_and(|hours| hours <= 23)
}

fn parse_rfc1123_datetime(value: &str) -> Option<NaiveDateTime> {
    let value = value.replace(',', ".");
    NaiveDateTime::parse_from_str(&value, "%d %b %Y %H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(&value, "%d %b %Y %H:%M:%S"))
        .ok()
}

fn parse_rfc850_datetime(value: &str) -> Option<NaiveDateTime> {
    let value = value.replace(',', ".");
    NaiveDateTime::parse_from_str(&value, "%d-%b-%y %H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(&value, "%d-%b-%y %H:%M:%S"))
        .ok()
}

fn without_weekday(prefix: &str) -> Option<&str> {
    Some(prefix.split_once(',')?.1.trim())
}

fn rfc1123_prefix(input: &str) -> Option<&str> {
    let (weekday, _) = input.split_once(',')?;
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        .contains(&weekday)
        .then_some(input)
}

fn rfc850_prefix(input: &str) -> Option<&str> {
    let (weekday, _) = input.split_once(',')?;
    [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ]
    .contains(&weekday)
    .then_some(input)
}

fn rfc_prefix(input: &str) -> Option<&str> {
    let (prefix, zone) = input.rsplit_once(' ')?;
    if zone.is_empty()
        || !zone
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return None;
    }
    // Go's MST layout accepts a three-letter alphabetic abbreviation.
    (zone.len() == 3).then_some(prefix)
}

fn normalize_comma_fraction(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut normalized = input.to_owned();
    for index in 3..bytes.len() {
        if bytes[index] == b','
            && bytes[index - 1].is_ascii_digit()
            && bytes[index - 2].is_ascii_digit()
            && bytes[index - 3] == b':'
        {
            normalized.replace_range(index..=index, ".");
        }
    }
    normalized
}

fn parse_numeric_offset(zone: &str) -> Option<i64> {
    let bytes = zone.as_bytes();
    if bytes.len() != 5 || !matches!(bytes[0], b'+' | b'-') {
        return None;
    }
    let hours = std::str::from_utf8(&bytes[1..3])
        .ok()?
        .parse::<i64>()
        .ok()?;
    let minutes = std::str::from_utf8(&bytes[3..5])
        .ok()?
        .parse::<i64>()
        .ok()?;
    // Go's numeric-zone parser deliberately accepts 24 hours and 60 minutes
    // and normalizes the total offset; retain that observable extension.
    if hours > 24 || minutes > 60 {
        return None;
    }
    let seconds = hours * 60 * 60 + minutes * 60;
    Some(if bytes[0] == b'-' { -seconds } else { seconds })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};
    use proptest::prelude::*;

    #[test]
    fn accepts_documented_and_go_rfc_layouts() {
        for input in [
            "2026-08-31T12:34:56Z",
            "2026-08-31T14:34:56+02:00",
            "2026-08-31 12:34:56Z",
            "2026-08-31 14:34:56+02:00",
            "2026-08-31T12:34:56",
            "2026-08-31 12:34:56",
            "Mon, 31 Aug 2026 12:34:56 +0000",
            "Mon, 31 Aug 2026 12:34:56 UTC",
            "Monday, 31-Aug-26 12:34:56 UTC",
            "2026-08-31T12:34:56.123456Z",
        ] {
            let value = parse(input).unwrap_or_else(|error| panic!("{input:?}: {error}"));
            assert_eq!(value.offset(), &chrono::offset::Utc);
        }
    }

    #[test]
    fn accepts_comma_fractions_and_rfc850_numeric_zones() {
        for input in [
            "2026-08-31T12:34:56,123456Z",
            "2026-08-31T12:34:56,123456",
            "2026-08-31 12:34:56,123456+02:00",
            "Monday, 31-Aug-26 12:34:56 +0001",
            "Monday, 31-Aug-26 12:34:56 -0001",
            "Monday, 31-Aug-26 12:34:56 -0000",
        ] {
            assert!(parse(input).is_ok(), "{input:?} must be accepted");
        }
        assert_eq!(
            format_iso(parse("2026-08-31 12:34:56,123456+02:00").unwrap()),
            "2026-08-31T10:34:56+00:00"
        );
        for input in [
            "Monday, 31-Aug-26 12:34:56 +0024",
            "Monday, 31-Aug-26 12:34:56 -0024",
            "Monday, 31-Aug-26 12:34:56 +0060",
            "Monday, 31-Aug-26 12:34:56 -0060",
        ] {
            assert_eq!(parse(input), Err(TimestampError::Malformed), "{input:?}");
        }
    }

    #[test]
    fn trims_and_distinguishes_empty_from_malformed() {
        assert_eq!(parse(" \t\n "), Err(TimestampError::Empty));
        assert_eq!(parse("not-a-timestamp"), Err(TimestampError::Malformed));
    }

    #[test]
    fn canonical_formatting_is_utc_and_seconds_only() {
        let value = FixedOffset::east_opt(2 * 60 * 60)
            .unwrap()
            .with_ymd_and_hms(2026, 8, 31, 14, 34, 56)
            .unwrap()
            + chrono::Duration::milliseconds(999);
        assert_eq!(format_iso(value), "2026-08-31T12:34:56+00:00");
        assert_eq!(format_sql(value), "2026-08-31 12:34:56");
        assert_eq!(format_sql_bytes(value), b"2026-08-31 12:34:56");
    }

    proptest! {
        #[test]
        fn canonical_iso_round_trips(
            year in 1970i32..=2099,
            month in 1u32..=12,
            day in 1u32..=28,
            hour in 0u32..24,
            minute in 0u32..60,
            second in 0u32..60,
        ) {
            let value = Utc.with_ymd_and_hms(year, month, day, hour, minute, second).unwrap();
            let encoded = format_iso(value);
            prop_assert_eq!(parse(&encoded), Ok(value));
        }
    }
}
