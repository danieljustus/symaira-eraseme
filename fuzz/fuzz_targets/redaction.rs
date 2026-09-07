#![no_main]

use libfuzzer_sys::fuzz_target;
use symeraseme_core::redaction::{
    Action, Match, RedactionProfile, collect_matches, redact_bytes, review_bytes,
};

fn redact_decision(_: &Match) -> Action {
    Action::Redact
}

fuzz_target!(|input: &[u8]| {
    let profile = RedactionProfile {
        full_name: "Jürgen Müller".to_owned(),
        name_variants: vec!["Jane Doe".to_owned()],
        email_addresses: vec!["jane.doe@example.com".to_owned()],
        phone_numbers: vec!["555-123-4567".to_owned()],
        addresses: vec![],
    };
    let Ok(matches) = collect_matches(input, Some(&profile)) else {
        return;
    };
    let mut end = 0;
    for item in &matches {
        assert!(item.start >= end && item.start <= item.end && item.end <= input.len());
        assert_eq!(&input[item.start..item.end], item.value.as_slice());
        end = item.end;
    }
    let Ok(output) = redact_bytes(input, Some(&profile)) else {
        return;
    };
    let Ok(reviewed) = review_bytes(input, &matches, Some(redact_decision)) else {
        return;
    };
    assert_eq!(output, reviewed.output);
    for item in &matches {
        assert!(
            !output
                .windows(item.value.len())
                .any(|window| window == item.value)
        );
    }
});
