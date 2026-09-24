use symeraseme_core::{confirmation, timeutil};

#[test]
fn pure_input_boundaries_reject_untrusted_links() {
    let timestamp = timeutil::parse("2026-09-24T20:00:00Z").expect("valid UTC instant");
    assert_eq!(timeutil::format_sql(timestamp), "2026-09-24 20:00:00");
    assert!(timeutil::parse("2026-09-24T20:00:00+99:00").is_err());

    let links = confirmation::extract_confirmation_links(
        "https://schufa.de/confirm https://example.com/confirm https://schufa.de/confirm",
    );
    assert_eq!(links, ["https://schufa.de/confirm"]);
}
