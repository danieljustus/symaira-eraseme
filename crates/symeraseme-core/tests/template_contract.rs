use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use symeraseme_core::templating::{
    Address, FrozenDateTime, RenderContext, embedded_template_sources, list_template_names, render,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_data() -> Value {
    json!({
        "total_requests": 3,
        "planned": 1,
        "sent": 1,
        "awaiting_ack": 0,
        "awaiting_response": 0,
        "confirmed": 1,
        "rejected": 0,
        "overdue": 0,
        "campaigns": [{
            "campaign_id": "q3-2026", "id": "q3-2026", "kind": "initial",
            "created_at": "2026-07-01T09:00:00+00:00", "total": 3,
            "confirmation_rate": 33, "planned": 1, "sent": 1,
            "awaiting_ack": 0, "awaiting_response": 0, "confirmed": 1,
            "rejected": 0, "overdue": 0, "total_reminders_sent": 4,
            "avg_response_time_days": 12,
            "requests": [
                {"id": 1, "broker_id": "acxiom-eu", "jurisdiction": "DE", "current_status": "CONFIRMED", "sent_at": "2026-07-02T08:00:00+00:00", "resolved_at": "2026-07-20T10:30:00+00:00", "reminders_sent": 2},
                {"id": 2, "broker_id": "oracle-data", "jurisdiction": "US", "current_status": "PLANNED", "sent_at": "", "resolved_at": "", "reminders_sent": 0}
            ]
        }],
        "broker_status": [{"broker_id": "acxiom-eu", "total": 2, "confirmed": 1, "pending": 0, "overdue": 0, "rejected": 0}],
        "recent_events": [
            {"occurred_at": "2026-07-20T10:30:00+00:00", "event_type": "CONFIRMED", "request_id": 1, "broker_id": "acxiom-eu", "source": "inbox"},
            {"occurred_at": "2026-07-02T08:00:00+00:00", "event_type": "SENT", "request_id": 1, "broker_id": "acxiom-eu", "source": "system"}
        ],
        "success_metrics": {"overall_confirmation_rate": 33, "overall_rejection_rate": 0, "overdue_rate": 0, "avg_response_time_days": 12, "median_response_time_days": 12},
        "historical_comparison": {"requests_change": "+10", "confirmation_rate_change": 5, "rejection_rate_change": -2},
        "broker_leaderboard": [{"broker_id": "acxiom-eu", "total": 3, "confirmed": 2, "rejected": 0, "overdue": 0, "success_rate": 66, "avg_response_time_days": 12}],
        "jurisdiction_stats": [{"jurisdiction": "DE", "total": 2, "confirmed": 2, "rejected": 0, "overdue": 0, "confirmation_rate": 100}],
        "timeline": [{"date": "2026-07-20", "total_events": 3, "events": ["ACK", "CONFIRMED", "NOTE_ADDED"]}]
    })
}

fn fixture_context() -> RenderContext {
    RenderContext {
        full_name: "Max Mustermann".into(),
        name_variants: vec!["Max M.".into(), "M. Mustermann".into()],
        date_of_birth: Some("1990-06-15".into()),
        addresses: vec![
            Address {
                street: "Teststrasse 1".into(),
                city: "Berlin".into(),
                postal_code: "10115".into(),
                country: "DE".into(),
            },
            Address {
                street: "Alte Adresse 2".into(),
                city: "Muenchen".into(),
                postal_code: "80331".into(),
                country: "DE".into(),
            },
        ],
        email_addresses: vec!["max@example.com".into(), "max.m@example.org".into()],
        phone_numbers: vec!["+49 30 123456".into()],
        jurisdictions: vec!["DE".into(), "EU".into()],
        broker_name: "Acme Data Corp".into(),
        broker_website: "https://acme.example.com".into(),
        brokers: Vec::new(),
        data: fixture_data(),
        now: FrozenDateTime::from_rfc3339("2026-08-27T14:30:00+00:00").unwrap(),
        extra: BTreeMap::new(),
    }
}

fn golden() -> BTreeMap<String, String> {
    let path = repo_root().join("tests/fixtures/event-store/golden-templates.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn context_for(name: &str) -> RenderContext {
    let mut context = fixture_context();
    match name {
        "templates/dashboard.html.j2" => {
            context
                .extra
                .insert("auto_refresh_seconds".into(), json!(0));
        }
        "templates/report.html.j2" => {}
        _ => {
            context
                .extra
                .insert("original_request_date".into(), json!("2026-07-20"));
            context
                .extra
                .insert("request_id".into(), json!("REQ-12345"));
            context.extra.insert(
                "broker_reply_snippet".into(),
                json!("We could not verify your identity."),
            );
        }
    }
    context
}

#[test]
fn every_canonical_template_matches_the_external_golden_byte_for_byte() {
    let golden = golden();
    assert_eq!(golden.len(), 11);
    assert_eq!(list_template_names().len(), golden.len());
    for (public_name, expected) in golden {
        let bare = public_name
            .split_once('/')
            .map_or(public_name.as_str(), |(_, name)| name);
        let actual = render(bare, &context_for(&public_name))
            .unwrap_or_else(|error| panic!("{bare}: {error}"));
        if actual.as_bytes() != expected.as_bytes() {
            let first = actual
                .as_bytes()
                .iter()
                .zip(expected.as_bytes())
                .position(|(got, want)| got != want)
                .unwrap_or(actual.len().min(expected.len()));
            panic!(
                "{public_name}: len got={} want={} first_diff={} got={:?} want={:?}",
                actual.len(),
                expected.len(),
                first,
                &actual.as_bytes()[first.saturating_sub(32)..actual.len().min(first + 64)],
                &expected.as_bytes()[first.saturating_sub(32)..expected.len().min(first + 64)]
            );
        }
    }
}

#[test]
fn canonical_sources_are_embedded_without_drift() {
    let names: Vec<_> = embedded_template_sources().collect();
    assert_eq!(names.len(), 11);
    for (name, embedded) in names {
        let relative = if name.ends_with(".html.j2") {
            "templates"
        } else {
            "laws"
        };
        let source =
            fs::read_to_string(repo_root().join("registry").join(relative).join(name)).unwrap();
        assert_eq!(embedded.as_bytes(), source.as_bytes(), "{name}");
    }
}

#[test]
fn aliases_and_names_are_bounded_and_sorted() {
    let names = list_template_names();
    assert_eq!(
        names,
        vec![
            "ccpa-deletion.en.md.j2",
            "ccpa-opt-out.en.md.j2",
            "ccpa-rebuttal-deletion.md.j2",
            "dashboard.html.j2",
            "gdpr-art17.de.md.j2",
            "gdpr-art17.en.md.j2",
            "gdpr-rebuttal-address.md.j2",
            "gdpr-rebuttal-identity.md.j2",
            "gdpr-rebuttal-rejected.en.md.j2",
            "gdpr-rebuttal-verification.en.md.j2",
            "report.html.j2",
        ]
    );
    assert!(render("laws/gdpr-art17.en.md.j2", &fixture_context()).is_ok());
    assert!(render("templates/report.html.j2", &fixture_context()).is_ok());
    for invalid in [
        "../report.html.j2",
        "/report.html.j2",
        "C:/report.html.j2",
        "templates/../report.html.j2",
        "report\\html.j2",
        "report\0.html.j2",
        "unknown.md.j2",
    ] {
        assert!(render(invalid, &fixture_context()).is_err(), "{invalid:?}");
    }
}

#[test]
fn context_precedence_unicode_and_markdown_behavior_are_stable() {
    let mut context = fixture_context();
    context
        .extra
        .insert("full_name".into(), json!("Override Ü😀"));
    let output = render("gdpr-art17.en.md.j2", &context).unwrap();
    assert!(output.contains("Override Ü😀"));

    context
        .extra
        .insert("broker_name".into(), json!("<sentinel>"));
    let markdown = render("gdpr-rebuttal-rejected.en.md.j2", &context).unwrap();
    assert!(markdown.contains("regarding <sentinel>"));

    let mut report = fixture_context();
    report.data["broker_leaderboard"][0]["broker_id"] = json!("<sentinel> &");
    let html = render("report.html.j2", &report).unwrap();
    assert!(html.contains("&lt;sentinel&gt; &amp;"));
    assert!(!html.contains("<sentinel> &"));
}

#[test]
fn malformed_or_missing_values_fail_without_echoing_context() {
    let mut context = fixture_context();
    context.data = json!("malformed sentinel PII");
    let error = render("templates/report.html.j2", &context)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("malformed sentinel PII"));

    let mut missing = fixture_context();
    missing.extra.clear();
    missing.data = Value::Null;
    let error = render("templates/dashboard.html.j2", &missing)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("Max Mustermann"));
    assert!(error.len() <= 256);
}

#[test]
fn empty_lists_are_supported_deterministically() {
    let mut context = fixture_context();
    context.email_addresses.clear();
    context.phone_numbers.clear();
    context.addresses.clear();
    context.date_of_birth = None;
    let output = render("ccpa-deletion.en.md.j2", &context).unwrap();
    assert!(!output.contains("email address on file"));
    assert!(!output.contains("address on file"));
}
