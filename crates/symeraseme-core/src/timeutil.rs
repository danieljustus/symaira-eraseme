//! Timestamp parsing and formatting shared by the event-store port.
//!
//! The accepted input deliberately follows the Go event-store oracle rather
//! than accepting every format supported by a date-time crate.  In particular,
//! timestamps without an offset are UTC wall-clock values.  Parsing and
//! rendering are spelled out here because the frozen Go bytes do not follow
//! every default of the `time` crate.

use std::fmt;
use time::{
    Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset,
    error::{Parse, ParseFromDescription},
    format_description::well_known::Rfc3339,
    macros::format_description,
};

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

const SECONDS_UTC: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]+00:00");
const SECONDS_SQL: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
const SECONDS: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
const TEMPLATE_UTC: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute] UTC");

/// Parse an accepted timestamp and return its instant in UTC.
///
/// Accepted layouts are the Go oracle's ISO T/space forms with or without an
/// offset, RFC 1123 (numeric or alphabetic zone), RFC 850, and RFC 3339.
/// Go accepts fractional seconds even when a layout omits them; the same
/// extension is retained here.  Alphabetic RFC zones have no portable offset
/// in the Go parser and therefore use its zero-offset result.
pub fn parse(input: &str) -> Result<OffsetDateTime, TimestampError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(TimestampError::Empty);
    }

    let normalized = normalize_comma_fraction(input);
    let normalized_z = normalized
        .strip_suffix('Z')
        .map(|prefix| format!("{prefix}+00:00"));
    for value in normalized_z
        .iter()
        .map(String::as_str)
        .chain([normalized.as_str(), input])
    {
        if let Some(value) = parse_iso(value) {
            return Ok(value);
        }
    }

    // RFC1123Z uses a numeric zone; parse the weekday separately so that
    // the day name is validated but not required to agree with the date,
    // matching time.Parse.
    // Go's HTTP-time layouts accept a comma as the fractional separator too;
    // use the same normalized spelling here as for the ISO forms.
    if let Some(value) = parse_rfc1123_numeric(&normalized) {
        return Ok(value);
    }
    if let Some(value) = parse_rfc1123_alpha(&normalized) {
        return Ok(value);
    }

    // RFC850 uses a two-digit year.  Go's pivot is 69: 00..68 map to
    // 2000..2068 and 69..99 map to 1969..1999.
    if let Some(value) = parse_rfc850(&normalized) {
        return Ok(value);
    }

    Err(TimestampError::Malformed)
}

/// Alias using the name used by the event-store contract.
pub fn parse_timestamp(input: &str) -> Result<OffsetDateTime, TimestampError> {
    parse(input)
}

/// Parse an RFC 3339 value with the separator restriction the Go parser
/// applies. `time`'s `Rfc3339` accepts any single separator character; that
/// extension is not observable through the frozen contract.
pub fn parse_rfc3339(value: &str) -> Result<OffsetDateTime, Parse> {
    if value.len() > 10 && !matches!(value.as_bytes()[10], b'T' | b't' | b' ') {
        return Err(Parse::ParseFromDescription(
            ParseFromDescription::InvalidComponent("separator"),
        ));
    }
    OffsetDateTime::parse(value, &Rfc3339)
}

/// Format a timestamp as canonical UTC ISO text with seconds precision.
pub fn format_iso(timestamp: OffsetDateTime) -> String {
    timestamp
        .to_offset(UtcOffset::UTC)
        .format(&SECONDS_UTC)
        .expect("timestamp fields are in range")
}

/// Format a timestamp as the UTC SQLite wall-clock form with seconds precision.
pub fn format_sql(timestamp: OffsetDateTime) -> String {
    timestamp
        .to_offset(UtcOffset::UTC)
        .format(&SECONDS_SQL)
        .expect("timestamp fields are in range")
}

/// Return the SQL representation as its exact stored bytes.
pub fn format_sql_bytes(timestamp: OffsetDateTime) -> Vec<u8> {
    format_sql(timestamp).into_bytes()
}

/// Go's `time.Now().UTC().Format("2006-01-02T15:04:05")`, used by manual-task
/// columns. Neither `format_iso` nor `format_sql` has these exact bytes.
pub fn format_manual_task_timestamp(timestamp: OffsetDateTime) -> String {
    timestamp
        .to_offset(UtcOffset::UTC)
        .format(&format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second]"
        ))
        .expect("timestamp fields are in range")
}

/// The minijinja `strftime("%Y-%m-%d %H:%M UTC")` form, rendered in UTC.
pub fn format_template_utc(timestamp: OffsetDateTime) -> String {
    timestamp
        .to_offset(UtcOffset::UTC)
        .format(&TEMPLATE_UTC)
        .expect("timestamp fields are in range")
}

/// Go's `time.Time` JSON / RFC3339Nano form: trailing zeros of the fraction
/// are trimmed and a zero offset is written as `Z`.
pub fn format_rfc3339_nano(value: OffsetDateTime) -> String {
    let mut rendered = value
        .format(&SECONDS)
        .expect("timestamp fields are in range");
    let nanos = value.nanosecond();
    if nanos != 0 {
        let fraction = format!(".{nanos:09}");
        rendered.push_str(fraction.trim_end_matches('0'));
    }
    rendered.push_str(&offset_suffix(value.offset(), true));
    rendered
}

/// Go's `RFC3339Nano` with chrono's `AutoSi` digit grouping: 0, 3, 6 or 9
/// digits. This is what the MCP wire and the campaign read path emit.
/// `use_z` mirrors chrono's `to_rfc3339_opts` flag.
pub fn format_rfc3339_auto_si(value: OffsetDateTime, use_z: bool) -> String {
    let mut rendered = value
        .format(&SECONDS)
        .expect("timestamp fields are in range");
    let nanos = value.nanosecond();
    let digits: usize = if nanos == 0 {
        0
    } else if nanos.is_multiple_of(1_000_000) {
        3
    } else if nanos.is_multiple_of(1_000) {
        6
    } else {
        9
    };
    if digits != 0 {
        let scaled = nanos / 10_u32.pow((9 - digits) as u32);
        rendered.push_str(&format!(".{scaled:0digits$}"));
    }
    rendered.push_str(&offset_suffix(value.offset(), use_z));
    rendered
}

fn offset_suffix(offset: UtcOffset, use_z: bool) -> String {
    if use_z && offset == UtcOffset::UTC {
        return "Z".to_owned();
    }
    let sign = if offset.is_negative() { '-' } else { '+' };
    format!(
        "{sign}{:02}:{:02}",
        offset.whole_hours().unsigned_abs(),
        offset.minutes_past_hour().unsigned_abs()
    )
}

/// Parse the ISO wall-clock forms. An absent offset is UTC, as in the Go
/// event-store oracle. Fractions may have any digit count; only the first
/// nine are kept, matching `chrono`'s `%.f`.
fn parse_iso(value: &str) -> Option<OffsetDateTime> {
    let bytes = value.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let slice = value.get(range)?;
        if !slice.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        slice.parse().ok()
    };
    let year = digits(0..4)? as i32;
    if bytes[4] != b'-' {
        return None;
    }
    let month = digits(5..7)? as u8;
    if bytes[7] != b'-' {
        return None;
    }
    let day = digits(8..10)? as u8;
    if !matches!(bytes[10], b'T' | b' ') {
        return None;
    }
    let hour = digits(11..13)? as u8;
    if bytes[13] != b':' {
        return None;
    }
    let minute = digits(14..16)? as u8;
    if bytes[16] != b':' {
        return None;
    }
    let second = digits(17..19)? as u8;
    let (nanos, rest) = parse_fraction(&value[19..])?;
    let offset = if rest.is_empty() {
        UtcOffset::UTC
    } else {
        parse_iso_offset(rest)?
    };
    let date = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
    let time = Time::from_hms_nano(hour, minute, second, nanos).ok()?;
    Some(
        PrimitiveDateTime::new(date, time)
            .assume_offset(offset)
            .to_offset(UtcOffset::UTC),
    )
}

fn parse_fraction(rest: &str) -> Option<(u32, &str)> {
    let Some(after) = rest.strip_prefix('.') else {
        return Some((0, rest));
    };
    let count = after
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if count == 0 {
        return None;
    }
    let mut nanos = 0_u32;
    for byte in after.bytes().take(count.min(9)) {
        nanos = nanos * 10 + u32::from(byte - b'0');
    }
    nanos *= 10_u32.pow((9 - count.min(9)) as u32);
    Some((nanos, &after[count..]))
}

fn parse_iso_offset(rest: &str) -> Option<UtcOffset> {
    let bytes = rest.as_bytes();
    let sign = match bytes.first()? {
        b'+' => 1_i32,
        b'-' => -1_i32,
        _ => return None,
    };
    let (hours, minutes) = match bytes.len() {
        6 if bytes[3] == b':' => (
            rest.get(1..3)?.parse::<i32>().ok()?,
            rest.get(4..6)?.parse::<i32>().ok()?,
        ),
        5 => (
            rest.get(1..3)?.parse::<i32>().ok()?,
            rest.get(3..5)?.parse::<i32>().ok()?,
        ),
        _ => return None,
    };
    if hours > 23 || minutes > 59 {
        return None;
    }
    UtcOffset::from_whole_seconds(sign * (hours * 3600 + minutes * 60)).ok()
}

fn parse_rfc1123_numeric(input: &str) -> Option<OffsetDateTime> {
    let (prefix, zone) = input.rsplit_once(' ')?;
    let prefix = without_weekday(rfc1123_prefix(prefix)?)?;
    let value = parse_rfc1123_datetime(prefix)?;
    let zone = parse_numeric_offset(zone)?;
    Some(
        (value - Duration::seconds(zone))
            .assume_utc()
            .to_offset(UtcOffset::UTC),
    )
}

fn parse_rfc1123_alpha(input: &str) -> Option<OffsetDateTime> {
    let prefix = rfc_prefix(input)?;
    let prefix = without_weekday(rfc1123_prefix(prefix)?)?;
    let value = parse_rfc1123_datetime(prefix)?;
    Some(value.assume_utc())
}

fn parse_rfc850(input: &str) -> Option<OffsetDateTime> {
    let (prefix, zone) = input.rsplit_once(' ')?;
    let prefix = without_weekday(rfc850_prefix(prefix)?)?;
    let value = parse_rfc850_datetime(prefix)?;
    if !valid_rfc850_zone(zone) {
        return None;
    }
    let year = value.year().rem_euclid(100);
    let year = if year >= 69 { 1900 + year } else { 2000 + year };
    let date = value.date().replace_year(year).ok()?;
    Some(PrimitiveDateTime::new(date, value.time()).assume_utc())
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

fn parse_rfc1123_datetime(value: &str) -> Option<PrimitiveDateTime> {
    let mut parts = value.split_whitespace();
    let day: u8 = parts.next()?.parse().ok()?;
    let month = month_number(parts.next()?)?;
    let year: i32 = parts.next()?.parse().ok()?;
    let (clock, extra) = parse_clock(parts.next()?)?;
    if parts.next().is_some() || !extra.is_empty() {
        return None;
    }
    primitive(year, month, day, clock)
}

fn parse_rfc850_datetime(value: &str) -> Option<PrimitiveDateTime> {
    let (date_text, clock_text) = value.split_once(' ')?;
    let mut parts = date_text.split('-');
    let day: u8 = parts.next()?.parse().ok()?;
    let month = month_number(parts.next()?)?;
    let short: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let year = if short >= 69 {
        1900 + short
    } else {
        2000 + short
    };
    let (clock, extra) = parse_clock(clock_text)?;
    if !extra.is_empty() {
        return None;
    }
    primitive(year, month, day, clock)
}

fn primitive(year: i32, month: u8, day: u8, time: Time) -> Option<PrimitiveDateTime> {
    let date = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
    Some(PrimitiveDateTime::new(date, time))
}

fn parse_clock(text: &str) -> Option<(Time, &str)> {
    let bytes = text.as_bytes();
    if bytes.len() < 8 {
        return None;
    }
    let hour = text.get(0..2)?.parse().ok()?;
    if bytes[2] != b':' {
        return None;
    }
    let minute = text.get(3..5)?.parse().ok()?;
    if bytes[5] != b':' {
        return None;
    }
    let second = text.get(6..8)?.parse().ok()?;
    let (nanos, rest) = parse_fraction(&text[8..])?;
    Some((Time::from_hms_nano(hour, minute, second, nanos).ok()?, rest))
}

fn month_number(name: &str) -> Option<u8> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lowered = name.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|month| *month == lowered)
        .map(|index| index as u8 + 1)
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
    use proptest::prelude::*;
    use time::macros::datetime;

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
            assert_eq!(value.offset(), UtcOffset::UTC);
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
        let value = datetime!(2026-08-31 14:34:56 +02:00) + Duration::milliseconds(999);
        assert_eq!(format_iso(value), "2026-08-31T12:34:56+00:00");
        assert_eq!(format_sql(value), "2026-08-31 12:34:56");
        assert_eq!(format_sql_bytes(value), b"2026-08-31 12:34:56");
    }

    #[test]
    fn characterization_go_rfc3339_auto_si_bytes() {
        for (input, expected) in [
            ("2026-08-31T12:34:56Z", "2026-08-31T12:34:56Z"),
            ("2026-08-31T12:34:56.1Z", "2026-08-31T12:34:56.100Z"),
            ("2026-08-31T12:34:56.12Z", "2026-08-31T12:34:56.120Z"),
            ("2026-08-31T12:34:56.123Z", "2026-08-31T12:34:56.123Z"),
            ("2026-08-31T12:34:56.1234Z", "2026-08-31T12:34:56.123400Z"),
            ("2026-08-31T12:34:56.123456Z", "2026-08-31T12:34:56.123456Z"),
            (
                "2026-08-31T12:34:56.1234567Z",
                "2026-08-31T12:34:56.123456700Z",
            ),
            (
                "2026-08-31T12:34:56.123456789Z",
                "2026-08-31T12:34:56.123456789Z",
            ),
            (
                "2026-08-31T14:34:56.123+02:30",
                "2026-08-31T14:34:56.123+02:30",
            ),
            (
                "2026-08-31T12:34:56.123-05:00",
                "2026-08-31T12:34:56.123-05:00",
            ),
        ] {
            let value = parse_rfc3339(input).expect(input);
            assert_eq!(format_rfc3339_auto_si(value, true), expected, "{input}");
        }
    }

    #[test]
    fn characterization_parsed_fractions_keep_their_instant() {
        // The parser accepts a fraction on both separators and after a comma,
        // and drops sub-nanosecond digits. The formatted seconds/truncation
        // helpers must keep the same instant.
        for (input, expected_sql) in [
            ("2026-08-31T12:34:56Z", "2026-08-31 12:34:56"),
            ("2026-08-31 12:34:56.1", "2026-08-31 12:34:56"),
            ("2026-08-31T12:34:56.123456789Z", "2026-08-31 12:34:56"),
            ("2026-08-31 14:34:56,123+02:00", "2026-08-31 12:34:56"),
        ] {
            let value = parse(input).unwrap_or_else(|error| panic!("{input}: {error}"));
            assert_eq!(format_sql(value), expected_sql, "{input}");
            assert_eq!(
                format_iso(value),
                format!("{}+00:00", expected_sql.replacen(' ', "T", 1)),
                "{input}"
            );
        }
    }

    proptest! {
        #[test]
        fn canonical_iso_round_trips(
            year in 1970i32..=2099,
            month in 1u8..=12,
            day in 1u8..=28,
            hour in 0u8..24,
            minute in 0u8..60,
            second in 0u8..60,
        ) {
            let value = primitive(year, month, day, Time::from_hms(hour, minute, second).unwrap())
                .unwrap()
                .assume_utc();
            let encoded = format_iso(value);
            prop_assert_eq!(parse(&encoded), Ok(value));
        }
    }
}
