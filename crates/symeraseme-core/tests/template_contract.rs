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
fn arbitrary_precision_numbers_are_exact_or_rejected() {
    for (input, expected) in [
        (
            "-170141183460469231731687303715884105728",
            "-170141183460469231731687303715884105728",
        ),
        ("18446744073709551615", "18446744073709551615"),
        (
            "170141183460469231731687303715884105727",
            "170141183460469231731687303715884105727",
        ),
        (
            "340282366920938463463374607431768211455",
            "340282366920938463463374607431768211455",
        ),
        ("1.25", "1.25"),
        ("1e3", "1000.0"),
    ] {
        let mut context = context_for("templates/dashboard.html.j2");
        context.data["total_requests"] = serde_json::from_str(input).unwrap();
        let rendered = render("dashboard.html.j2", &context).unwrap();
        assert!(
            rendered.contains(&format!("<div class=\"count\">{expected}</div>")),
            "number was not rendered as expected: {input}"
        );
    }

    let mut context = context_for("templates/dashboard.html.j2");
    context.data["total_requests"] =
        serde_json::from_str("340282366920938463463374607431768211456").unwrap();
    let error = render("dashboard.html.j2", &context).unwrap_err();
    assert_eq!(error.to_string(), "invalid render context");
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
fn bounded_rendering_rejects_adversarial_contexts_without_echoing_values() {
    let sentinel = "template security sentinel";

    let mut direct = fixture_context();
    direct.full_name = format!("{sentinel}{}", "x".repeat(64 * 1024 + 1));
    let error = render("gdpr-art17.en.md.j2", &direct)
        .unwrap_err()
        .to_string();
    assert!(error.len() <= 256);
    assert!(!error.contains(sentinel));

    let mut nested = fixture_context();
    nested.data = json!({"nested": {"value": format!("{sentinel}{}", "x".repeat(64 * 1024 + 1))}});
    let error = render("templates/report.html.j2", &nested)
        .unwrap_err()
        .to_string();
    assert!(error.len() <= 256);
    assert!(!error.contains(sentinel));
}

#[test]
fn bounded_rendering_rejects_json_depth_collections_and_nodes() {
    let sentinel = "structure security sentinel";

    let mut too_deep = fixture_context();
    let mut nested = json!({"value": sentinel});
    for _ in 0..40 {
        nested = json!({"nested": nested});
    }
    too_deep.data = nested;
    let error = render("templates/report.html.j2", &too_deep)
        .unwrap_err()
        .to_string();
    assert!(error.len() <= 256);
    assert!(!error.contains(sentinel));

    let mut too_many_items = fixture_context();
    too_many_items.name_variants = vec![sentinel.to_owned(); 4097];
    let error = render("gdpr-art17.en.md.j2", &too_many_items)
        .unwrap_err()
        .to_string();
    assert!(error.len() <= 256);
    assert!(!error.contains(sentinel));

    let mut too_many_map_entries = fixture_context();
    for index in 0..4097 {
        too_many_map_entries
            .extra
            .insert(format!("extra-{index}"), json!(sentinel));
    }
    let error = render("gdpr-art17.en.md.j2", &too_many_map_entries)
        .unwrap_err()
        .to_string();
    assert!(error.len() <= 256);
    assert!(!error.contains(sentinel));

    let mut object = serde_json::Map::new();
    for index in 0..4096 {
        object.insert(
            format!("entry-{index}"),
            Value::Array(vec![Value::Null; 15]),
        );
    }
    let mut too_many_nodes = fixture_context();
    too_many_nodes.data = Value::Object(object);
    let error = render("templates/report.html.j2", &too_many_nodes)
        .unwrap_err()
        .to_string();
    assert!(error.len() <= 256);
    assert!(!error.contains(sentinel));
}

#[test]
fn bounded_rendering_rejects_string_aggregate_and_output_amplification() {
    let sentinel = "aggregate and output security sentinel";

    let mut too_many_string_bytes = fixture_context();
    let aggregate = format!("{sentinel}{}", "x".repeat(64_000));
    too_many_string_bytes.name_variants = vec![aggregate; 33];
    let error = render("gdpr-art17.en.md.j2", &too_many_string_bytes)
        .unwrap_err()
        .to_string();
    assert!(error.len() <= 256);
    assert!(!error.contains(sentinel));

    let mut amplified = fixture_context();
    let requests = amplified.data["campaigns"][0]["requests"]
        .as_array_mut()
        .expect("fixture requests array");
    requests.clear();
    let broker_id = format!("{sentinel}{}", "x".repeat(12_000));
    for id in 0..100 {
        requests.push(json!({
            "id": id,
            "broker_id": broker_id.clone(),
            "jurisdiction": "US",
            "current_status": "CONFIRMED",
            "sent_at": "2026-07-02T08:00:00+00:00",
            "resolved_at": "2026-07-20T10:30:00+00:00",
            "reminders_sent": 0
        }));
    }
    let error = render("templates/report.html.j2", &amplified)
        .unwrap_err()
        .to_string();
    assert!(error.len() <= 256);
    assert!(!error.contains(sentinel));
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
