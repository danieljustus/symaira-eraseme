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
    let mut output_len = input.len();
    for (index, item) in matches.iter().enumerate() {
        if item.start < last_end
            || item.start >= item.end
            || item.end > input.len()
            || input.get(item.start..item.end) != Some(item.value.as_slice())
            || item.replacement().len() > MAX_OUTPUT_BYTES
        {
            return Err(ReviewError::InvalidMatch(index));
        }
        let span = item.end - item.start;
        output_len = output_len
            .checked_sub(span)
            .and_then(|length| length.checked_add(item.replacement().len()))
            .ok_or(ReviewError::OutputTooLarge)?;
        if output_len > MAX_OUTPUT_BYTES {
            return Err(ReviewError::OutputTooLarge);
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

#[cfg(test)]
mod hardening_tests {
    use super::*;

    #[test]
    fn review_diagnostics_and_decision_counts_preserve_the_public_contract() {
        for (error, expected) in [
            (ReviewError::InvalidMatch(7), "invalid redaction match 7"),
            (ReviewError::InvalidAction, "invalid review action"),
            (
                ReviewError::OutputTooLarge,
                "review output exceeds the configured limit",
            ),
            (
                ReviewError::Redaction(RedactionError::InputTooLarge),
                "redaction input exceeds the configured limit",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
        }
        let matches = [
            Match::new("redact", 0, 1, b"a".to_vec(), b"X".to_vec()),
            Match::new("keep", 2, 3, b"c".to_vec(), b"Y".to_vec()),
            Match::new("skip", 3, 4, b"d".to_vec(), b"Y".to_vec()),
            Match::new("redact", 5, 6, b"f".to_vec(), b"Z".to_vec()),
        ];
        let result = review_bytes(
            b"abcdef",
            &matches,
            Some(|item: &Match| match item.name.as_str() {
                "keep" => Action::Keep,
                "skip" => Action::Skip,
                _ => Action::Redact,
            }),
        )
        .unwrap();
        assert_eq!(result.output, b"XbcdeZ");
        assert_eq!((result.redacted, result.kept, result.skipped), (2, 1, 1));
        assert!(result.changed);
        assert!(!result.quit);
    }

    #[test]
    fn review_input_and_match_limits_are_inclusive() {
        let input = vec![b'x'; 16_777_216];
        let result = review_bytes(&input, &[], None::<fn(&Match) -> Action>).unwrap();
        assert_eq!(result.output.len(), 16_777_216);
        assert_eq!(
            review_bytes(&vec![b'x'; 16_777_217], &[], None::<fn(&Match) -> Action>),
            Err(ReviewError::Redaction(RedactionError::InputTooLarge))
        );
        for length in [100_000, 100_001] {
            let input = vec![b'x'; length];
            let matches = (0..length)
                .map(|i| Match::new("bounded", i, i + 1, vec![b'x'], Vec::new()))
                .collect::<Vec<_>>();
            let result = review_bytes(&input, &matches, None::<fn(&Match) -> Action>);
            if length == 100_000 {
                let result = result.unwrap();
                assert_eq!(result.kept, 100_000);
                assert_eq!(result.output, input);
            } else {
                assert_eq!(
                    result,
                    Err(ReviewError::Redaction(RedactionError::MatchLimit))
                );
            }
        }
    }

    #[test]
    fn replacement_and_projected_output_limits_keep_exact_boundaries() {
        const LIMIT: usize = 33_554_432;
        for length in [LIMIT, LIMIT + 1] {
            let matches = [Match::new(
                "bounded",
                0,
                1,
                b"x".to_vec(),
                vec![b'z'; length],
            )];
            let result = review_bytes(b"x", &matches, None::<fn(&Match) -> Action>);
            if length == LIMIT {
                assert_eq!(result.unwrap().output, b"x");
            } else {
                assert_eq!(result, Err(ReviewError::InvalidMatch(0)));
            }
        }
        for length in [LIMIT - 1, LIMIT] {
            let matches = [Match::new(
                "bounded",
                0,
                1,
                b"x".to_vec(),
                vec![b'z'; length],
            )];
            assert_eq!(
                validate_matches(b"xy", &matches),
                if length == LIMIT - 1 {
                    Ok(())
                } else {
                    Err(ReviewError::OutputTooLarge)
                }
            );
        }
    }

    #[test]
    fn actual_output_growth_stops_before_another_decision_and_checks_the_tail() {
        const LIMIT: usize = 33_554_432;
        let matches = [
            Match::new("shrink", 0, 1, b"a".to_vec(), Vec::new()),
            Match::new("expand", 2, 3, b"c".to_vec(), vec![b'z'; LIMIT - 1]),
        ];
        let result = review_bytes(b"abc", &matches, Some(|_: &Match| Action::Redact)).unwrap();
        assert_eq!(result.output.len(), LIMIT);
        assert_eq!(result.output[0], b'b');
        assert!(result.output[1..].iter().all(|byte| *byte == b'z'));
        drop(result);
        drop(matches);

        let matches = [
            Match::new("shrink", 0, 1, b"a".to_vec(), Vec::new()),
            Match::new("expand", 1, 2, b"b".to_vec(), vec![b'z'; LIMIT - 1]),
        ];
        assert_eq!(
            review_bytes(
                b"abc",
                &matches,
                Some(|item: &Match| if item.name == "shrink" {
                    Action::Skip
                } else {
                    Action::Redact
                })
            ),
            Err(ReviewError::OutputTooLarge)
        );
        drop(matches);

        let matches = [
            Match::new("shrink", 0, 2, b"ab".to_vec(), Vec::new()),
            Match::new("expand", 2, 3, b"c".to_vec(), vec![b'z'; LIMIT - 1]),
            Match::new("tail", 3, 4, b"d".to_vec(), Vec::new()),
        ];
        assert_eq!(
            review_bytes(
                b"abcd",
                &matches,
                Some(|item: &Match| match item.name.as_str() {
                    "shrink" => Action::Skip,
                    "expand" => Action::Redact,
                    _ => panic!("the output limit must stop review before the next decision"),
                })
            ),
            Err(ReviewError::OutputTooLarge)
        );
    }
}
