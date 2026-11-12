//! Post-cutover contract change #1302: read-only commands print their payload
//! in text mode instead of the Go CLI's bare `success`, and #1303: every
//! command lists a description in `--help`. `--output json` stays unchanged.
//! Neither is a Go behaviour, so there is no recorded oracle: the contract is
//! asserted here against seeded stores.

use chrono::{TimeZone, Utc};
use serde_json::{Map, Value};
use std::path::Path;
use std::process::{Command, Output};
use symeraseme_core::storage::Store;
use symeraseme_core::storage::types::{EventType, Source};

fn run(data_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(args)
        .env_clear()
        .env("HOME", data_dir)
        .env("SYMERASEME_DATA_DIR", data_dir)
        .output()
        .expect("run symeraseme")
}

fn stdout(data_dir: &Path, args: &[&str]) -> String {
    let output = run(data_dir, args);
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "{args:?} wrote to stderr");
    String::from_utf8(output.stdout).expect("UTF-8 stdout")
}

/// Two planned requests (the first one also sent) and one pending manual task.
fn seeded() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("symeraseme.db")).unwrap();
    let connection = store.connection();
    connection
        .execute(
            "INSERT INTO campaigns (id, kind, notes) VALUES ('pop', 'initial', '')",
            [],
        )
        .unwrap();
    for broker in ["acme", "beta"] {
        connection
            .execute(
                "INSERT INTO removal_requests (broker_id, campaign_id, jurisdiction, created_at)
                 VALUES (?1, 'pop', 'eu', '2026-11-10T12:00:00Z')",
                [broker],
            )
            .unwrap();
        let id = connection.last_insert_rowid();
        store
            .append_and_project(
                id,
                &EventType::Planned,
                &Map::new(),
                &Source::System,
                Utc.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap(),
            )
            .unwrap();
        if broker == "acme" {
            store
                .append_and_project(
                    id,
                    &EventType::Sent,
                    &Map::new(),
                    &Source::User,
                    Utc.with_ymd_and_hms(2026, 11, 10, 12, 5, 0).unwrap(),
                )
                .unwrap();
        }
    }
    connection
        .execute_batch(
            "INSERT INTO manual_tasks \
             (id,request_id,broker_id,broker_name,form_url,reason,instructions,screenshot_path,\
              html_snapshot_path,form_fields_json,status,created_at,completed_at,notes) \
             VALUES (1,NULL,'acme','Acme Data','https://example.test/form','captcha','do it',\
              '','','{}','pending','2026-01-02T03:04:05Z',NULL,'');",
        )
        .unwrap();
    drop(store);
    dir
}

#[test]
fn manual_tasks_text_shows_the_tasks_and_json_is_unchanged() {
    let dir = seeded();
    assert_eq!(
        stdout(dir.path(), &["manual-tasks", "list"]),
        "Manual tasks (1):\n  #1 [pending] Acme Data (captcha) @ 2026-01-02T03:04:05Z\n"
    );
    let show = stdout(dir.path(), &["manual-tasks", "show", "1"]);
    assert!(show.starts_with("Manual task #1:\n  Broker:     Acme Data (acme)\n"));
    assert!(show.ends_with("\n\nInstructions:\ndo it\n"));

    let json: Value = serde_json::from_str(&stdout(
        dir.path(),
        &["manual-tasks", "list", "--output", "json"],
    ))
    .unwrap();
    assert_eq!(json["tasks"][0]["broker_name"], "Acme Data");
    assert_eq!(json["success"], true);
}

#[test]
fn a_failed_read_prints_its_error_instead_of_success() {
    let dir = seeded();
    let missing = stdout(dir.path(), &["manual-tasks", "show", "99"]);
    assert_eq!(
        missing,
        "Manual task #99 not found. Run 'symeraseme manual-tasks list' to see available tasks.\n"
    );
    // The JSON payload for the same failure keeps its shape and exit status.
    let json: Value = serde_json::from_str(&stdout(
        dir.path(),
        &["manual-tasks", "show", "99", "--output", "json"],
    ))
    .unwrap();
    assert_eq!(json["success"], false);
    // A transport failure still goes to stderr with a failing status.
    let invalid = run(dir.path(), &["manual-tasks", "show", "abc"]);
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    // A bad output format is still rejected.
    let format = run(dir.path(), &["requests", "list", "--output", "yaml"]);
    assert_eq!(format.status.code(), Some(1));
    assert!(format.stdout.is_empty());
}

#[test]
fn requests_events_calendar_and_dashboard_print_their_data() {
    let dir = seeded();
    let requests = stdout(dir.path(), &["requests", "list"]);
    let lines = requests.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "Requests: 2 of 2 (page 1, 100 per page)");
    assert_eq!(lines.len(), 3, "{requests}");
    assert!(
        lines[1].starts_with("  #1 [AWAITING_ACK] acme via "),
        "{requests}"
    );
    assert!(lines[1].contains("campaign=pop created=2026-11-10T12:00:00Z"));
    assert!(
        lines[2].starts_with("  #2 [PLANNED] beta via "),
        "{requests}"
    );

    let events = stdout(dir.path(), &["events", "show", "1"]);
    let lines = events.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "Events (2):");
    assert!(
        lines[1].starts_with("  #1 2026-11-10T12:00:00") && lines[1].ends_with("PLANNED (system)")
    );
    assert!(lines[2].starts_with("  #2 2026-11-10T12:05:00") && lines[2].ends_with("SENT (user)"));
    assert_eq!(
        stdout(dir.path(), &["events", "show", "77"]),
        "No events found.\n"
    );

    let calendar = stdout(dir.path(), &["calendar"]);
    assert!(calendar.starts_with("Calendar for all: "), "{calendar}");
    assert!(calendar.contains("entries,"), "{calendar}");

    let dashboard = stdout(dir.path(), &["dashboard"]);
    assert!(
        dashboard.starts_with("Dashboard: 2 requests ("),
        "{dashboard}"
    );
    assert!(dashboard.ends_with("Campaigns: 1\n"), "{dashboard}");

    for (argv, key) in [
        (vec!["requests", "list", "--output", "json"], "requests"),
        (vec!["calendar", "--output", "json"], "weeks"),
        (vec!["dashboard", "--output", "json"], "campaigns"),
    ] {
        let json: Value = serde_json::from_str(&stdout(dir.path(), &argv)).unwrap();
        assert!(json.get(key).is_some(), "{argv:?} lost {key}");
    }
}

#[test]
fn schedule_status_text_lists_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    let status = stdout(dir.path(), &["schedule", "status", "--platform", "cron"]);
    assert!(
        status.starts_with("Scheduler platform: cron\n  cron: "),
        "{status}"
    );
    assert_ne!(status, "success\n");

    let launchd = stdout(dir.path(), &["schedule", "status", "--platform", "launchd"]);
    let lines = launchd.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "Scheduler platform: launchd");
    for (line, label) in
        lines[1..]
            .iter()
            .zip(["symeraseme-tick", "symeraseme-poll", "symeraseme-rescan"])
    {
        assert!(
            line.starts_with(&format!("  {label}: not installed, inactive")),
            "{launchd}"
        );
    }
    assert_eq!(lines.len(), 4, "{launchd}");
}

#[test]
fn mutating_commands_keep_the_success_line() {
    let dir = seeded();
    assert_eq!(
        stdout(dir.path(), &["manual-tasks", "cleanup", "--dry-run"]),
        "success\n"
    );
}

/// The `Available Commands:` rows of one `--help` output as (name, description).
fn command_rows(help: &str) -> Vec<(String, String)> {
    help.split("Available Commands:\n")
        .nth(1)
        .map(|rest| rest.split("\n\n").next().unwrap_or_default())
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let line = line.trim_start();
            let (name, description) = line.split_once(' ').unwrap_or((line, ""));
            (name.to_owned(), description.trim().to_owned())
        })
        .collect()
}

#[test]
fn help_lists_a_description_for_every_command() {
    let dir = tempfile::tempdir().unwrap();
    let root = stdout(dir.path(), &["--help"]);
    let top = command_rows(&root);
    assert!(top.len() >= 30, "root lists {} commands", top.len());
    let mut checked = 0;
    for (name, description) in &top {
        assert!(
            !description.is_empty(),
            "root row {name} has no description"
        );
        let help = stdout(dir.path(), &[name, "--help"]);
        // A leaf names itself above `Usage:`; a group lists described rows.
        assert!(
            !help.starts_with("Usage:"),
            "`{name} --help` has no description heading"
        );
        for (child, description) in command_rows(&help) {
            assert!(
                !description.is_empty(),
                "`{name}` row {child} has no description"
            );
            let leaf = stdout(dir.path(), &[name, &child, "--help"]);
            assert!(
                !leaf.starts_with("Usage:"),
                "`{name} {child} --help` has no description heading"
            );
            checked += 1;
        }
        checked += 1;
    }
    assert!(checked >= 50, "only {checked} commands were checked");
}
