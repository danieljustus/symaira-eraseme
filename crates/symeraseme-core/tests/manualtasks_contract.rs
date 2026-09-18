//! Manual-task queue conformance.
//!
//! Mirrors the expectations of Go's `internal/manualtasks` tests: reason
//! normalisation, redacted 0600 snapshots, the `HUMAN_ACTION_REQUIRED` event
//! and the resulting request state, list filtering, completion notes, and the
//! cleanup counts.
//!
//! `SYMERASEME_DATA_DIR` is process-wide, so the tests that redirect the
//! artifact directory take a mutex and restore the previous value.

use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use symeraseme_core::manualtasks::{
    CreateOpts, ListOpts, cleanup, complete, create, get, instructions_for_reason, list,
    redact_identity_values, save_screenshot,
};
use symeraseme_core::storage::repository::Repository;
use symeraseme_core::storage::store::Store;
use tempfile::{TempDir, tempdir};

static DATA_DIR_LOCK: Mutex<()> = Mutex::new(());

/// Redirects the artifact directory for the duration of one test.
struct DataDir {
    _guard: MutexGuard<'static, ()>,
    previous: Option<String>,
}

impl DataDir {
    fn set(path: &std::path::Path) -> Self {
        let guard = DATA_DIR_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous = std::env::var("SYMERASEME_DATA_DIR").ok();
        // SAFETY: the mutex serialises every test that touches this variable.
        unsafe { std::env::set_var("SYMERASEME_DATA_DIR", path) };
        Self {
            _guard: guard,
            previous,
        }
    }
}

impl Drop for DataDir {
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var("SYMERASEME_DATA_DIR", value),
                None => std::env::remove_var("SYMERASEME_DATA_DIR"),
            }
        }
    }
}

fn pinned_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-06T12:00:00+00:00")
        .expect("pinned instant")
        .with_timezone(&Utc)
}

fn store_with_request() -> (TempDir, Store, i64) {
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("symeraseme.db")).expect("open store");
    let repository = Repository::new(&store);
    let request_id = repository
        .create_removal_request("test-broker", "web_form", "test", "CCPA", "", "")
        .expect("create removal request");
    (tree, store, request_id)
}

fn read_events(store: &Store, request_id: i64) -> Vec<(String, String, String)> {
    let connection = store.connection();
    let mut statement = connection
        .prepare("SELECT event_type, source, payload_json FROM request_events WHERE request_id = ? ORDER BY id")
        .expect("prepare events query");
    let rows = statement
        .query_map([request_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query events");
    rows.map(|row| row.expect("event row")).collect()
}

#[cfg(unix)]
fn mode_of(path: &str) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .expect("artifact exists")
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn create_persists_redacted_task_and_human_action() {
    let (tree, store, request_id) = store_with_request();
    let _data_dir = DataDir::set(tree.path());

    let task = create(
        &store,
        &CreateOpts {
            request_id: Some(request_id),
            broker_id: "test-broker".to_owned(),
            broker_name: "Test Broker".to_owned(),
            form_url: "https://example.test/optout".to_owned(),
            reason: "made_up_reason".to_owned(),
            html_snapshot: "<html>Contact jane@example.com at 555-123-4567</html>".to_owned(),
            form_fields: [("email".to_owned(), "jane@example.com".to_owned())].into(),
            step_index: 2,
            total_steps: 5,
            error_message: "Timeout waiting for selector".to_owned(),
            ..CreateOpts::default()
        },
        None,
        pinned_now(),
    )
    .expect("create task");

    assert_eq!(task.reason, "generic_error");
    assert_eq!(task.status, "pending");
    assert!(
        !task.html_snapshot_path.is_empty(),
        "expected a snapshot path"
    );
    let snapshot = std::fs::read_to_string(&task.html_snapshot_path).expect("snapshot readable");
    assert!(!snapshot.is_empty());
    assert!(!snapshot.contains("jane@example.com"), "{snapshot}");
    assert!(!snapshot.contains("555-123-4567"), "{snapshot}");
    assert!(snapshot.contains("[REDACTED-EMAIL]"), "{snapshot}");
    assert!(snapshot.contains("[REDACTED-PHONE]"), "{snapshot}");
    #[cfg(unix)]
    assert_eq!(mode_of(&task.html_snapshot_path), 0o600);

    let stored = get(&store, task.id).expect("get").expect("task exists");
    assert_eq!(stored.request_id, Some(request_id));

    let events = read_events(&store, request_id);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].0, "HUMAN_ACTION_REQUIRED");
    assert!(
        events[0]
            .2
            .contains(&format!("\"manual_task_id\":{}", task.id))
    );

    let state = store.rebuild_state(request_id).expect("rebuild state");
    assert_eq!(state.current_status, "AWAITING_USER_ACTION");
}

#[test]
fn list_filters_and_complete_appends_note() {
    let (tree, store, request_id) = store_with_request();
    let _data_dir = DataDir::set(tree.path());

    let first = create(
        &store,
        &CreateOpts {
            request_id: Some(request_id),
            broker_id: "a".to_owned(),
            reason: "timeout".to_owned(),
            ..CreateOpts::default()
        },
        None,
        pinned_now(),
    )
    .expect("first task");
    let second = create(
        &store,
        &CreateOpts {
            broker_id: "b".to_owned(),
            reason: "captcha_failed".to_owned(),
            ..CreateOpts::default()
        },
        None,
        pinned_now(),
    )
    .expect("second task");
    assert_ne!(first.id, second.id);

    let filtered = list(
        &store,
        &ListOpts {
            status: Some("pending".to_owned()),
            request_id: Some(request_id),
        },
    )
    .expect("list");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, first.id);

    let completed = complete(&store, first.id, "completed manually", true, pinned_now())
        .expect("complete")
        .expect("task exists");
    assert_eq!(completed.status, "completed");
    assert!(completed.completed_at.is_some());

    let events = read_events(&store, request_id);
    let last = events.last().expect("note event");
    assert_eq!(last.0, "NOTE_ADDED");
    // Go's `SrcUser` is the wire value "user", not an upper-cased label.
    assert_eq!(last.1, "user");
}

#[test]
fn cleanup_dry_run_then_apply() {
    let tree = tempdir().expect("temp dir");
    for name in ["snap.png", "page.html", "task.json", "keep.txt"] {
        std::fs::write(tree.path().join(name), name).expect("seed artifact");
    }
    let preview = cleanup(tree.path(), true).expect("dry run");
    assert_eq!(preview.removed, 0);
    assert_eq!(preview.skipped, 3);
    assert!(preview.dry_run);

    let applied = cleanup(tree.path(), false).expect("apply");
    assert_eq!(applied.removed, 3);
    assert!(
        tree.path().join("keep.txt").exists(),
        "non-artifact removed"
    );
    assert!(
        !tree.path().join("task.json").exists(),
        ".json is an artifact"
    );
}

#[test]
fn save_screenshot_ignores_empty_and_writes_private_files() {
    let tree = tempdir().expect("temp dir");
    let _data_dir = DataDir::set(tree.path());

    assert_eq!(
        save_screenshot(&[], pinned_now()).expect("empty screenshot"),
        None
    );
    let path = save_screenshot(b"png-bytes", pinned_now())
        .expect("screenshot")
        .expect("path");
    assert!(std::fs::read(path.as_str()).expect("readable") == b"png-bytes");
    #[cfg(unix)]
    assert_eq!(mode_of(&path), 0o600);
}

#[test]
fn instructions_match_the_go_messages() {
    assert_eq!(
        instructions_for_reason("login_required", "Acme"),
        "The web form for Acme requires login or authentication. Automatic form filling cannot proceed. Please log in and complete the opt-out process manually."
    );
    assert_eq!(
        instructions_for_reason("unknown_reason", "Acme"),
        "Please complete the opt-out process for Acme manually by visiting the URL below."
    );
}

#[test]
fn redaction_prefers_profile_values_then_falls_back() {
    let html = "mail jane@example.com or call 555-123-4567; name Jane Doe";
    let fallback = redact_identity_values(html, None);
    assert!(fallback.contains("[REDACTED-EMAIL]"), "{fallback}");
    assert!(fallback.contains("[REDACTED-PHONE]"), "{fallback}");
    // The profile-less fallback is deliberately coarse: email and phone only,
    // so a bare name survives it exactly as it does in Go.
    assert!(fallback.contains("Jane Doe"), "{fallback}");

    let profile = symeraseme_core::identity::Profile {
        full_name: "Jane Doe".to_owned(),
        email_addresses: vec!["jane@example.com".to_owned()],
        ..symeraseme_core::identity::Profile::default()
    };
    let with_profile = redact_identity_values(html, Some(&profile));
    assert!(with_profile.contains("[REDACTED-NAME]"), "{with_profile}");
    assert!(with_profile.contains("[REDACTED-EMAIL]"), "{with_profile}");
}
