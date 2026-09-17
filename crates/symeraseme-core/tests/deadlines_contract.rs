use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use symeraseme_core::{deadlines::actions_for_candidate, storage::TickCandidate};

#[derive(Deserialize)]
struct GoldenTick {
    now: String,
    actions: Vec<Value>,
}

fn at(value: &str) -> DateTime<Utc> {
    symeraseme_core::timeutil::parse_timestamp(value).expect("valid fixture timestamp")
}

#[allow(clippy::too_many_arguments)]
fn candidate(
    id: i64,
    broker_id: &str,
    status: &str,
    jurisdiction: &str,
    sent_at: &str,
    deadline_at: &str,
    resolved_at: &str,
    reminders_sent: i64,
    escalation_level: i64,
) -> TickCandidate {
    TickCandidate {
        id,
        broker_id: broker_id.into(),
        campaign_id: "golden-tick".into(),
        jurisdiction: jurisdiction.into(),
        current_status: status.into(),
        sent_at: sent_at.into(),
        deadline_at: deadline_at.into(),
        next_action_at: String::new(),
        acknowledged_at: String::new(),
        resolved_at: resolved_at.into(),
        reminders_sent,
        escalation_level,
    }
}

#[test]
fn golden_tick_policy_matches_the_go_fixture() {
    let fixture: GoldenTick = serde_json::from_str(include_str!(
        "../../../tests/fixtures/event-store/golden-tick.json"
    ))
    .expect("golden tick fixture");
    let now = at(&fixture.now);
    let candidates = [
        candidate(
            1,
            "broker-a",
            "AWAITING_ACK",
            "GDPR",
            "2026-08-06T10:00:00+00:00",
            "",
            "",
            1,
            0,
        ),
        candidate(
            2,
            "broker-b",
            "AWAITING_RESPONSE",
            "GDPR",
            "2026-07-18T10:00:00+00:00",
            "2026-08-17T10:00:00+00:00",
            "",
            0,
            0,
        ),
        candidate(
            4,
            "broker-d",
            "CONFIRMED",
            "GDPR",
            "2026-05-19T10:00:00+00:00",
            "",
            "2026-05-21T10:00:00+00:00",
            0,
            0,
        ),
        candidate(
            3,
            "broker-c",
            "OVERDUE",
            "GDPR",
            "2026-07-08T10:00:00+00:00",
            "2026-08-07T10:00:00+00:00",
            "",
            0,
            1,
        ),
    ];
    let actual: Vec<Value> = candidates
        .iter()
        .flat_map(|request| actions_for_candidate(request, now, false))
        .map(|action| serde_json::to_value(action).expect("serializable action"))
        .collect();

    assert_eq!(actual, fixture.actions);
}

#[test]
fn policy_preserves_go_fallbacks_and_dry_run() {
    let now = at("2026-08-27T10:00:00+00:00");
    let request = candidate(
        7,
        "broker-ccpa",
        "AWAITING_RESPONSE",
        "CCPA",
        "2026-07-13T10:00:00+00:00",
        "",
        "",
        0,
        0,
    );
    let action = actions_for_candidate(&request, now, true)
        .pop()
        .expect("CCPA deadline is due");
    assert_eq!(action.event_type, "DEADLINE_REACHED");
    assert_eq!(action.payload["deadline_days"], 45);
    assert_eq!(action.payload["deadline_at"], "2026-08-27T10:00:00+00:00");
    assert!(action.dry_run);
}

#[test]
fn action_json_uses_the_go_field_order() {
    let now = at("2026-08-27T10:00:00+00:00");
    let request = candidate(
        1,
        "broker-a",
        "AWAITING_ACK",
        "GDPR",
        "2026-08-06T10:00:00+00:00",
        "",
        "",
        1,
        0,
    );
    let action = actions_for_candidate(&request, now, false)
        .pop()
        .expect("reminder is due");
    let encoded = serde_json::to_string(&action).expect("serializable action");
    assert_eq!(
        encoded,
        r#"{"request_id":1,"broker_id":"broker-a","campaign_id":"golden-tick","current_status":"AWAITING_ACK","action_type":"send_reminder","event_type":"REMINDER_SENT","description":"Send reminder #2 (21d since sent)","payload":{"count":2,"days_since_sent":21},"dry_run":false}"#
    );
}

#[test]
fn reminder_counter_matches_go_i64_wrapping() {
    let now = at("2026-08-27T10:00:00+00:00");
    let cases = [
        (9, -1, "2026-08-20T10:00:00+00:00", 0),
        (10, 61, "2026-08-20T10:00:00+00:00", 62),
        (11, 1, "2026-08-13T10:00:00+00:00", 2),
    ];
    for (id, reminders_sent, sent_at, expected_count) in cases {
        let request = candidate(
            id,
            "broker-counter",
            "AWAITING_ACK",
            "GDPR",
            sent_at,
            "",
            "",
            reminders_sent,
            0,
        );
        let action = actions_for_candidate(&request, now, false)
            .pop()
            .expect("Go wrapping makes this reminder due");
        assert_eq!(action.payload["count"].as_i64(), Some(expected_count));
    }
}

#[test]
fn boundary_contract_table_covers_each_tick_action() {
    struct Boundary {
        name: &'static str,
        request: TickCandidate,
        dry_run: bool,
        event_type: Option<&'static str>,
    }

    let now = at("2026-08-27T10:00:00+00:00");
    let rows = [
        Boundary {
            name: "reminder-7",
            request: candidate(
                20,
                "broker-reminder",
                "AWAITING_ACK",
                "GDPR",
                "2026-08-20T10:00:00+00:00",
                "",
                "",
                0,
                0,
            ),
            dry_run: true,
            event_type: Some("REMINDER_SENT"),
        },
        Boundary {
            name: "reminder-14",
            request: candidate(
                21,
                "broker-reminder",
                "AWAITING_ACK",
                "GDPR",
                "2026-08-13T10:00:00+00:00",
                "",
                "",
                1,
                0,
            ),
            dry_run: false,
            event_type: Some("REMINDER_SENT"),
        },
        Boundary {
            name: "deadline-30",
            request: candidate(
                22,
                "broker-gdpr",
                "AWAITING_RESPONSE",
                "GDPR",
                "2026-07-28T10:00:00+00:00",
                "",
                "",
                0,
                0,
            ),
            dry_run: true,
            event_type: Some("DEADLINE_REACHED"),
        },
        Boundary {
            name: "deadline-45",
            request: candidate(
                23,
                "broker-ccpa",
                "AWAITING_RESPONSE",
                "CCPA",
                "2026-07-13T10:00:00+00:00",
                "",
                "",
                0,
                0,
            ),
            dry_run: false,
            event_type: Some("DEADLINE_REACHED"),
        },
        Boundary {
            name: "deadline-invalid-explicit-fallback",
            request: candidate(
                24,
                "broker-fallback",
                "AWAITING_RESPONSE",
                "GDPR",
                "2026-07-28T10:00:00+00:00",
                "not-a-timestamp",
                "",
                0,
                0,
            ),
            dry_run: false,
            event_type: Some("DEADLINE_REACHED"),
        },
        Boundary {
            name: "dpa-14",
            request: candidate(
                25,
                "broker-dpa",
                "OVERDUE",
                "GDPR",
                "2026-07-08T10:00:00+00:00",
                "2026-08-13T10:00:00+00:00",
                "",
                0,
                1,
            ),
            dry_run: true,
            event_type: Some("DPA_COMPLAINT_DRAFTED"),
        },
        Boundary {
            name: "dpa-escalated",
            request: candidate(
                26,
                "broker-dpa",
                "OVERDUE",
                "GDPR",
                "2026-07-08T10:00:00+00:00",
                "2026-08-13T10:00:00+00:00",
                "",
                0,
                2,
            ),
            dry_run: false,
            event_type: None,
        },
        Boundary {
            name: "rescan-90",
            request: candidate(
                27,
                "broker-rescan",
                "CONFIRMED",
                "GDPR",
                "2026-05-19T10:00:00+00:00",
                "",
                "2026-05-29T10:00:00+00:00",
                0,
                0,
            ),
            dry_run: true,
            event_type: Some("RE_SCAN_TRIGGERED"),
        },
    ];

    for row in rows {
        let actions = actions_for_candidate(&row.request, now, row.dry_run);
        match row.event_type {
            Some(event_type) => {
                let action = actions.first().unwrap_or_else(|| panic!("{}", row.name));
                assert_eq!(action.event_type, event_type, "{}", row.name);
                assert_eq!(action.dry_run, row.dry_run, "{}", row.name);
            }
            None => assert!(actions.is_empty(), "{}", row.name),
        }
    }
}

#[test]
fn malformed_or_non_due_inputs_produce_no_action() {
    let now = at("2026-08-27T10:00:00+00:00");
    let mut request = candidate(
        8,
        "broker-invalid",
        "AWAITING_ACK",
        "UNKNOWN",
        "not-a-timestamp",
        "",
        "",
        0,
        0,
    );
    assert!(actions_for_candidate(&request, now, false).is_empty());

    request.current_status = "CONFIRMED".into();
    request.resolved_at = "2026-08-01T10:00:00+00:00".into();
    assert!(actions_for_candidate(&request, now, false).is_empty());
}
