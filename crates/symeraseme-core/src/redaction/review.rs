use super::pii::{MAX_INPUT_BYTES, MAX_MATCHES, MAX_OUTPUT_BYTES, Match, RedactionError};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Keep,
    Redact,
    Skip,
    Quit,
}
pub type Decision = Action;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewResult {
    pub output: Vec<u8>,
    pub changed: bool,
    pub quit: bool,
    pub redacted: usize,
    pub kept: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewError {
    InvalidMatch(usize),
    InvalidAction,
    OutputTooLarge,
    Redaction(RedactionError),
}
impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMatch(index) => write!(f, "invalid redaction match {index}"),
            Self::InvalidAction => f.write_str("invalid review action"),
            Self::OutputTooLarge => f.write_str("review output exceeds the configured limit"),
            Self::Redaction(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for ReviewError {}

pub fn review_bytes<F>(
    input: &[u8],
    matches: &[Match],
    mut decide: Option<F>,
) -> Result<ReviewResult, ReviewError>
where
    F: FnMut(&Match) -> Action,
{
    if input.len() > MAX_INPUT_BYTES {
        return Err(ReviewError::Redaction(RedactionError::InputTooLarge));
    }
    if matches.len() > MAX_MATCHES {
        return Err(ReviewError::Redaction(RedactionError::MatchLimit));
    }
    validate_matches(input, matches)?;
    let mut result = ReviewResult {
        output: input.to_vec(),
        changed: false,
        quit: false,
        redacted: 0,
        kept: 0,
        skipped: 0,
    };
    if matches.is_empty() {
        return Ok(result);
    }
    let mut position = 0;
    let mut output = Vec::with_capacity(input.len());
    for (index, item) in matches.iter().enumerate() {
        let action = decide
            .as_mut()
            .map(|callback| callback(item))
            .unwrap_or(Action::Keep);
        match action {
            Action::Keep => result.kept += 1,
            Action::Skip => result.skipped += 1,
            Action::Redact => {
                let prefix_len = item.start - position;
                let new_len = output
                    .len()
                    .checked_add(prefix_len)
                    .and_then(|length| length.checked_add(item.replacement().len()))
                    .ok_or(ReviewError::OutputTooLarge)?;
                if new_len > MAX_OUTPUT_BYTES {
                    return Err(ReviewError::OutputTooLarge);
                }
                output.extend_from_slice(&input[position..item.start]);
                output.extend_from_slice(item.replacement());
                position = item.end;
                result.redacted += 1;
                result.changed = true;
            }
            Action::Quit => {
                result.quit = true;
                break;
            }
        }
        if index + 1 == matches.len() {
            break;
        }
    }
    if result.changed {
        output.extend_from_slice(&input[position..]);
        if output.len() > MAX_OUTPUT_BYTES {
            return Err(ReviewError::OutputTooLarge);
        }
        result.output = output;
    }
    Ok(result)
}

fn validate_matches(input: &[u8], matches: &[Match]) -> Result<(), ReviewError> {
    let mut last_end = 0;
    for (index, item) in matches.iter().enumerate() {
        if item.start < last_end
            || item.start > item.end
            || item.end > input.len()
            || input.get(item.start..item.end) != Some(item.value.as_slice())
            || item.replacement().len() > MAX_OUTPUT_BYTES
        {
            return Err(ReviewError::InvalidMatch(index));
        }
        last_end = item.end;
    }
    Ok(())
}

pub fn review_text<F>(
    input: &str,
    matches: &[Match],
    decide: Option<F>,
) -> Result<(String, ReviewResult), ReviewError>
where
    F: FnMut(&Match) -> Action,
{
    let result = review_bytes(input.as_bytes(), matches, decide)?;
    let text =
        String::from_utf8(result.output.clone()).map_err(|_| ReviewError::InvalidMatch(0))?;
    Ok((text, result))
}
