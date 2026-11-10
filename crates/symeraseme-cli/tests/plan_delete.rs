//! `plan delete` (#1297): a post-cutover command with no Go counterpart, so it
//! is tested against its own contract rather than a recorded oracle.

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

fn plan(store: &Store, campaign: &str, brokers: &[&str]) -> Vec<i64> {
    let connection = store.connection();
    connection
        .execute(
            "INSERT OR IGNORE INTO campaigns (id, kind, notes) VALUES (?1, 'initial', '')",
            [campaign],
        )
        .unwrap();
    brokers
        .iter()
        .map(|broker| {
            connection
                .execute(
                    "INSERT INTO removal_requests (broker_id, campaign_id, jurisdiction)
                     VALUES (?1, ?2, 'eu')",
                    [broker, &campaign],
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
            id
        })
        .collect()
}

fn rows(store: &Store) -> Vec<i64> {
    [
        "campaigns",
        "removal_requests",
        "request_events",
        "request_state",
    ]
    .iter()
    .map(|table| {
        store
            .connection()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    })
    .collect()
}

fn seeded() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("symeraseme.db")).unwrap();
    plan(&store, "wrong", &["a", "b", "c"]);
    plan(&store, "keep", &["d", "e"]);
    (dir, store)
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn dry_run_reports_counts_and_leaves_the_database_unchanged() {
    let (dir, store) = seeded();
    let before = rows(&store);
    drop(store);

    let output = run(
        dir.path(),
        &["plan", "delete", "--campaign", "wrong", "--dry-run"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "dry run: would delete campaign wrong: 3 request(s), 3 event(s), 3 state row(s), 1 campaign row(s)\n"
    );

    let output = run(
        dir.path(),
        &[
            "plan",
            "delete",
            "--campaign",
            "wrong",
            "--dry-run",
            "--output",
            "json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["campaign_id"], "wrong");
    assert_eq!(json["counts"]["removal_requests"], 3);

    let store = Store::open(dir.path().join("symeraseme.db")).unwrap();
    assert_eq!(rows(&store), before);
}

#[test]
fn delete_removes_only_the_named_campaign() {
    let (dir, store) = seeded();
    drop(store);

    let output = run(dir.path(), &["plan", "delete", "--campaign", "wrong"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "deleted campaign wrong: 3 request(s), 3 event(s), 3 state row(s), 1 campaign row(s)\n"
    );

    let store = Store::open(dir.path().join("symeraseme.db")).unwrap();
    assert_eq!(rows(&store), vec![1, 2, 2, 2]);
    let left: Vec<String> = store
        .connection()
        .prepare("SELECT DISTINCT campaign_id FROM removal_requests")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(left, ["keep"]);
    drop(store);

    let output = run(
        dir.path(),
        &["plan", "delete", "--campaign", "keep", "--output", "json"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["dry_run"], false);
    assert_eq!(json["counts"]["request_events"], 2);
}

#[test]
fn refuses_unknown_campaign_missing_flag_and_bad_output() {
    let (dir, _store) = seeded();
    let output = run(dir.path(), &["plan", "delete", "--campaign", "nope"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(stderr(&output), "campaign \"nope\" not found\n");

    let output = run(dir.path(), &["plan", "delete"]);
    assert!(!output.status.success());
    assert_eq!(stderr(&output), "--campaign is required\n");

    let output = run(
        dir.path(),
        &["plan", "delete", "--campaign", "wrong", "--output", "xml"],
    );
    assert!(!output.status.success());
    assert!(stderr(&output).contains("invalid output format"));
    let store = Store::open(dir.path().join("symeraseme.db")).unwrap();
    assert_eq!(rows(&store), vec![2, 5, 5, 5], "nothing was deleted");
}

#[test]
fn refuses_sent_requests_and_attached_evidence_without_deleting() {
    for (label, evidence) in [
        ("not PLANNED", Evidence::Sent),
        ("inbox reply", Evidence::Reply),
        ("manual task", Evidence::ManualTask),
    ] {
        let (dir, store) = seeded();
        let ids: Vec<i64> = store
            .connection()
            .prepare("SELECT id FROM removal_requests WHERE campaign_id = 'wrong' ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        match evidence {
            Evidence::Sent => {
                store
                    .append_and_project(
                        ids[1],
                        &EventType::Sent,
                        &Map::new(),
                        &Source::System,
                        Utc.with_ymd_and_hms(2026, 11, 10, 13, 0, 0).unwrap(),
                    )
                    .unwrap();
            }
            Evidence::Reply => {
                store
                    .connection()
                    .execute(
                        "INSERT INTO inbox_replies (request_id, message_id) VALUES (?1, 'm')",
                        [ids[0]],
                    )
                    .unwrap();
            }
            Evidence::ManualTask => {
                store
                    .connection()
                    .execute(
                        "INSERT INTO manual_tasks (request_id, broker_id) VALUES (?1, 'a')",
                        [ids[0]],
                    )
                    .unwrap();
            }
        }
        let before = rows(&store);
        drop(store);

        for extra in [&[][..], &["--dry-run"][..]] {
            let mut args = vec!["plan", "delete", "--campaign", "wrong"];
            args.extend_from_slice(extra);
            let output = run(dir.path(), &args);
            assert!(!output.status.success(), "{label}");
            assert!(output.stdout.is_empty(), "{label}");
            let message = stderr(&output);
            assert!(
                message.starts_with("refusing to delete campaign \"wrong\": "),
                "{label}: {message}"
            );
            assert!(message.contains(label), "{label}: {message}");
        }
        let store = Store::open(dir.path().join("symeraseme.db")).unwrap();
        assert_eq!(rows(&store), before, "{label}");
    }
}

enum Evidence {
    Sent,
    Reply,
    ManualTask,
}
