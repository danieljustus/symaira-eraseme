//! Durable manual-fallback task queue.
//!
//! Ports `internal/manualtasks/manualtasks.go`: creating, listing, completing
//! and cleaning up the tasks produced when web-form automation cannot proceed.
//!
//! Deviations from Go, all deliberate:
//! - The clock is injected instead of read from the environment, matching the
//!   rest of this crate (`chrono` is compiled without its `clock` feature).
//! - `create`/`save_screenshot` therefore take the instant that Go reads from
//!   `time.Now()`.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use regex::Regex;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde_json::{Map, json};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::identity::Profile;
use crate::storage::Store;
use crate::storage::projection::append_and_project_tx;
use crate::storage::types::{EventType, Source};

/// The supported fallback reasons in stable order.
pub const FALLBACK_REASONS: [&str; 9] = [
    "unknown_captcha",
    "captcha_failed",
    "timeout",
    "login_required",
    "multi_step_exceeded",
    "dynamic_form",
    "unknown_field",
    "assertion_failed",
    "generic_error",
];

/// The durable manual-fallback queue record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManualTask {
    pub id: i64,
    pub request_id: Option<i64>,
    pub broker_id: String,
    pub broker_name: String,
    pub form_url: String,
    pub reason: String,
    pub instructions: String,
    pub screenshot_path: String,
    pub html_snapshot_path: String,
    pub form_fields_json: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub notes: String,
}

/// The captured browser state needed to create a task.
#[derive(Debug, Clone, Default)]
pub struct CreateOpts {
    pub request_id: Option<i64>,
    pub broker_id: String,
    pub broker_name: String,
    pub form_url: String,
    pub reason: String,
    pub screenshot_path: String,
    pub html_snapshot: String,
    pub form_fields: BTreeMap<String, String>,
    pub step_index: i64,
    pub total_steps: i64,
    pub error_message: String,
    pub extra_instructions: String,
}

/// Optional queue filters.
#[derive(Debug, Clone, Default)]
pub struct ListOpts {
    pub status: Option<String>,
    pub request_id: Option<i64>,
}

/// Artifact cleanup outcome. Task rows are never deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CleanupResult {
    pub removed: u64,
    pub skipped: u64,
    pub dry_run: bool,
}

/// Flattened response returned by the manual-task service handlers.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceResult {
    pub success: bool,
    pub data: Vec<(String, serde_json::Value)>,
    pub error: Option<String>,
}

impl ServiceResult {
    /// Build the same top-level JSON object as Go's manual-task `Result`.
    pub fn into_value(self) -> serde_json::Value {
        let has_error = self.error.is_some();
        let mut entries = vec![("success".to_owned(), json!(self.success))];
        if let Some(error) = self.error {
            entries.push(("error".to_owned(), json!(error)));
        }
        entries.extend(
            self.data
                .into_iter()
                .filter(|(key, _)| !has_error || key != "message"),
        );
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        serde_json::Value::Object(entries.into_iter().collect())
    }
}

/// Implements Go's `HandleList` presentation over the durable queue.
pub fn handle_list(store: &Store, opts: &ListOpts) -> rusqlite::Result<ServiceResult> {
    let tasks = list(store, opts)?;
    let message = if tasks.is_empty() {
        "No manual tasks found.".to_owned()
    } else {
        let mut message = format!("Manual tasks ({}):", tasks.len());
        for task in &tasks {
            let broker = if task.broker_name.is_empty() {
                task.broker_id.as_str()
            } else {
                task.broker_name.as_str()
            };
            message.push_str(&format!(
                "\n  #{} [{}] {} ({}) @ {}",
                task.id, task.status, broker, task.reason, task.created_at
            ));
        }
        message
    };
    Ok(ServiceResult {
        success: true,
        data: vec![
            (
                "tasks".to_owned(),
                json!(tasks.iter().map(task_value).collect::<Vec<_>>()),
            ),
            ("message".to_owned(), json!(message)),
        ],
        error: None,
    })
}

/// Implements Go's `HandleShow` presentation and not-found response.
pub fn handle_show(store: &Store, task_id: i64) -> rusqlite::Result<ServiceResult> {
    let Some(task) = get(store, task_id)? else {
        return Ok(missing_task(task_id));
    };
    let mut message = format!(
        "Manual task #{}:\n  Broker:     {} ({})\n  URL:        {}\n  Reason:     {}\n  Status:     {}\n  Created:    {}",
        task.id,
        task.broker_name,
        task.broker_id,
        task.form_url,
        task.reason,
        task.status,
        task.created_at
    );
    if let Some(completed_at) = &task.completed_at
        && !completed_at.is_empty()
    {
        message.push_str("\n  Completed:  ");
        message.push_str(completed_at);
    }
    if !task.screenshot_path.is_empty() {
        message.push_str("\n  Screenshot: ");
        message.push_str(&task.screenshot_path);
    }
    if !task.html_snapshot_path.is_empty() {
        message.push_str("\n  HTML:       ");
        message.push_str(&task.html_snapshot_path);
    }
    message.push_str("\n\nInstructions:\n");
    message.push_str(&task.instructions);
    if !task.notes.is_empty() {
        message.push_str("\n\nNotes: ");
        message.push_str(&task.notes);
    }
    let mut data = task_value(&task).as_object().expect("task object").clone();
    data.insert("message".to_owned(), json!(message));
    Ok(ServiceResult {
        success: true,
        data: data.into_iter().collect(),
        error: None,
    })
}

/// Implements Go's `HandleComplete` response around the durable completion.
pub fn handle_complete(
    store: &Store,
    task_id: i64,
    notes: &str,
    now: DateTime<Utc>,
) -> rusqlite::Result<ServiceResult> {
    if complete(store, task_id, notes, true, now)?.is_none() {
        return Ok(missing_task(task_id));
    }
    Ok(ServiceResult {
        success: true,
        data: vec![
            ("task_id".to_owned(), json!(task_id)),
            (
                "message".to_owned(),
                json!(format!("Manual task #{task_id} marked as completed.")),
            ),
        ],
        error: None,
    })
}

/// Implements Go's `HandleCleanup`, including its missing-directory response.
pub fn handle_cleanup(tasks_dir: &Path, dry_run: bool) -> io::Result<ServiceResult> {
    match fs::metadata(tasks_dir) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ServiceResult {
                success: true,
                data: vec![(
                    "message".to_owned(),
                    json!("No manual tasks directory found — nothing to clean up."),
                )],
                error: None,
            });
        }
        Err(error) => return Err(error),
    }
    let outcome = cleanup(tasks_dir, dry_run)?;
    let message = if dry_run {
        format!(
            "Would remove {} artifact(s) from {}. Use --yes to confirm.",
            outcome.skipped,
            tasks_dir.display()
        )
    } else {
        format!(
            "Removed {} artifact(s) from {}.",
            outcome.removed,
            tasks_dir.display()
        )
    };
    Ok(ServiceResult {
        success: true,
        data: vec![
            ("removed".to_owned(), json!(outcome.removed)),
            ("skipped".to_owned(), json!(outcome.skipped)),
            ("dry_run".to_owned(), json!(outcome.dry_run)),
            ("message".to_owned(), json!(message)),
        ],
        error: None,
    })
}

fn missing_task(task_id: i64) -> ServiceResult {
    ServiceResult {
        success: false,
        data: Vec::new(),
        error: Some(format!(
            "Manual task #{task_id} not found. Run 'symeraseme manual-tasks list' to see available tasks."
        )),
    }
}

fn task_value(task: &ManualTask) -> serde_json::Value {
    json!({
        "id": task.id,
        "request_id": task.request_id,
        "broker_id": task.broker_id,
        "broker_name": task.broker_name,
        "form_url": task.form_url,
        "reason": task.reason,
        "instructions": task.instructions,
        "screenshot_path": task.screenshot_path,
        "html_snapshot_path": task.html_snapshot_path,
        "form_fields_json": task.form_fields_json,
        "status": task.status,
        "created_at": task.created_at,
        "completed_at": task.completed_at,
        "notes": task.notes,
    })
}

static EMAIL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z|a-z]{2,}\b")
        .expect("email pattern is valid")
});
static PHONE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d{3}[-.\s]?\d{3}[-.\s]?\d{4}\b").expect("phone pattern is valid")
});

/// Removes profile values from an HTML snapshot. Without a profile it applies
/// the same coarse email/phone fallback as the Python implementation.
pub fn redact_identity_values(html: &str, profile: Option<&Profile>) -> String {
    let Some(profile) = profile else {
        let redacted = EMAIL_PATTERN.replace_all(html, "[REDACTED-EMAIL]");
        return PHONE_PATTERN
            .replace_all(&redacted, "[REDACTED-PHONE]")
            .into_owned();
    };
    let mut redacted = html.to_owned();
    for value in &profile.email_addresses {
        redacted = replace_non_empty(&redacted, value, "[REDACTED-EMAIL]");
    }
    for value in &profile.phone_numbers {
        redacted = replace_non_empty(&redacted, value, "[REDACTED-PHONE]");
    }
    redacted = replace_non_empty(&redacted, &profile.full_name, "[REDACTED-NAME]");
    for value in &profile.name_variants {
        redacted = replace_non_empty(&redacted, value, "[REDACTED-NAME]");
    }
    if let Some(date_of_birth) = &profile.date_of_birth {
        redacted = replace_non_empty(&redacted, date_of_birth, "[REDACTED-DOB]");
    }
    for address in &profile.addresses {
        redacted = replace_non_empty(&redacted, &address.street, "[REDACTED-STREET]");
        redacted = replace_non_empty(&redacted, &address.city, "[REDACTED-CITY]");
        redacted = replace_non_empty(&redacted, &address.postal_code, "[REDACTED-POSTAL]");
        if let Some(state) = &address.state {
            redacted = replace_non_empty(&redacted, state, "[REDACTED-STATE]");
        }
    }
    redacted
}

fn replace_non_empty(value: &str, sensitive: &str, marker: &str) -> String {
    if sensitive.is_empty() {
        return value.to_owned();
    }
    value.replace(sensitive, marker)
}

fn redact_fields(
    fields: &BTreeMap<String, String>,
    profile: Option<&Profile>,
) -> BTreeMap<String, String> {
    fields
        .iter()
        .map(|(key, value)| (key.clone(), redact_identity_values(value, profile)))
        .collect()
}

/// The Python-compatible manual-task artifact directory.
pub fn tasks_dir() -> io::Result<PathBuf> {
    tasks_dir_in(None)
}

/// The artifact directory for an explicit data directory. `None` falls back to
/// `SYMERASEME_DATA_DIR` and then to the platform default, matching Go.
pub fn tasks_dir_in(data_dir: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(root) = data_dir {
        return Ok(root.join("manual_tasks"));
    }
    if let Ok(value) = env::var("SYMERASEME_DATA_DIR")
        && !value.is_empty()
    {
        return Ok(expand_home(&value).join("manual_tasks"));
    }
    let home = user_home_dir()?;
    Ok(home.join(".local/share/symeraseme/manual_tasks"))
}

fn user_home_dir() -> io::Result<PathBuf> {
    match env::var("HOME") {
        Ok(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        _ => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "$HOME is not defined",
        )),
    }
}

fn ensure_tasks_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    ensure_mode_0700(path)
}

/// Go's `expandHome`: only a leading `~` or `~/` is expanded.
fn expand_home(path: &str) -> PathBuf {
    if path == "~"
        && let Ok(home) = user_home_dir()
    {
        return home;
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = user_home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

#[cfg(unix)]
fn ensure_mode_0700(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn ensure_mode_0700(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    fs::write(path, contents)
}
pub fn instructions_for_reason(reason: &str, broker_name: &str) -> String {
    if !FALLBACK_REASONS.contains(&reason) {
        return format!(
            "Please complete the opt-out process for {broker_name} manually by visiting the URL below."
        );
    }
    match reason {
        "unknown_captcha" => format!(
            "The web form for {broker_name} has an unknown CAPTCHA type that could not be solved automatically. Please visit the URL below and complete the CAPTCHA manually, then submit the form."
        ),
        "captcha_failed" => format!(
            "The CAPTCHA solver failed for {broker_name}'s opt-out form. Please visit the URL below and complete the CAPTCHA manually."
        ),
        "timeout" => format!(
            "The web form for {broker_name} timed out during submission. This may indicate a slow server or a multi-step process. Please visit the URL below and complete the opt-out process manually."
        ),
        "login_required" => format!(
            "The web form for {broker_name} requires login or authentication. Automatic form filling cannot proceed. Please log in and complete the opt-out process manually."
        ),
        "multi_step_exceeded" => format!(
            "The web form for {broker_name} requires more steps than the configured limit. Please visit the URL below and follow the opt-out process to completion."
        ),
        "dynamic_form" => format!(
            "The web form for {broker_name} uses dynamic JavaScript fields that could not be filled automatically. Please visit the URL and complete the form manually."
        ),
        "unknown_field" => format!(
            "The web form for {broker_name} contains unknown fields that could not be mapped from the identity profile. Please visit the URL and complete the form manually."
        ),
        "assertion_failed" => format!(
            "The web form for {broker_name} was submitted but the expected confirmation message was not displayed. Please visit the URL and verify whether the opt-out was successful."
        ),
        _ => format!(
            "An unexpected error occurred while processing the web form for {broker_name}. Please visit the URL below and complete the opt-out process manually."
        ),
    }
}

/// Go's `time.Now().UTC().Format("2006-01-02T15:04:05")`.
///
/// This is neither `format_iso` (`+00:00` suffix) nor `format_sql` (space
/// separator), so it is spelled out here rather than reusing either.
pub(crate) fn sql_timestamp(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn non_nil_fields(fields: &BTreeMap<String, String>) -> &BTreeMap<String, String> {
    fields
}

/// Persists a pending task, stores a redacted snapshot and records the
/// `HUMAN_ACTION_REQUIRED` event when a request id is present.
pub fn create(
    store: &Store,
    opts: &CreateOpts,
    profile: Option<&Profile>,
    now: DateTime<Utc>,
) -> rusqlite::Result<ManualTask> {
    let reason = if FALLBACK_REASONS.contains(&opts.reason.as_str()) {
        opts.reason.clone()
    } else {
        "generic_error".to_owned()
    };
    let form_url = redact_identity_values(&opts.form_url, profile);
    let screenshot_path = redact_identity_values(&opts.screenshot_path, profile);
    let mut instructions = instructions_for_reason(&reason, &opts.broker_name);
    if !opts.extra_instructions.is_empty() {
        instructions.push_str("\n\n");
        instructions.push_str(&redact_identity_values(&opts.extra_instructions, profile));
    }
    let form_fields_json =
        serde_json::to_string(&redact_fields(non_nil_fields(&opts.form_fields), profile))
            .expect("form fields serialise");

    let transaction = store.connection().unchecked_transaction()?;
    let request_arg = opts.request_id;
    let existing = transaction
        .query_row(
            "SELECT id, request_id, broker_id, broker_name,
             form_url, reason, instructions, screenshot_path, html_snapshot_path,
             form_fields_json, status, created_at, completed_at, notes
             FROM manual_tasks
             WHERE status = 'pending' AND broker_id = ? AND form_url = ? AND reason = ?
               AND ((request_id IS NULL AND ? IS NULL) OR request_id = ?)
             ORDER BY id LIMIT 1",
            params![opts.broker_id, form_url, reason, request_arg, request_arg],
            scan_task,
        )
        .optional()?;
    if let Some(task) = existing {
        return Ok(task);
    }

    let tasks_dir = tasks_dir().map_err(to_sql_error)?;
    let mut html_path = String::new();
    let mut wrote_html = false;
    if !opts.html_snapshot.is_empty() {
        ensure_tasks_dir(&tasks_dir).map_err(to_sql_error)?;
        let candidate = tasks_dir.join(format!(
            "snapshot_{}.html",
            now.timestamp_nanos_opt().unwrap_or_default()
        ));
        let redacted = redact_identity_values(&opts.html_snapshot, profile);
        match write_private(&candidate, redacted.as_bytes()) {
            // The Python implementation logs snapshot failures and continues
            // with an empty path.
            Ok(()) => {
                html_path = candidate.to_string_lossy().into_owned();
                wrote_html = true;
            }
            Err(_) => html_path.clear(),
        }
    }

    let insert = transaction.execute(
        "INSERT INTO manual_tasks
         (request_id, broker_id, broker_name, form_url, reason, instructions,
          screenshot_path, html_snapshot_path, form_fields_json, status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')",
        params![
            request_arg,
            opts.broker_id,
            opts.broker_name,
            form_url,
            reason,
            instructions,
            screenshot_path,
            html_path,
            form_fields_json
        ],
    );
    let insert = match insert {
        Ok(rows) => rows,
        Err(error) => {
            rollback_and_cleanup(transaction, &html_path, wrote_html);
            return Err(error);
        }
    };
    let _ = insert;
    let task_id = transaction.last_insert_rowid();
    if let Some(request_id) = opts.request_id
        && request_id != 0
    {
        let mut payload = Map::new();
        payload.insert("manual_task_id".to_owned(), json!(task_id));
        payload.insert("reason".to_owned(), json!(reason));
        payload.insert("form_url".to_owned(), json!(form_url));
        payload.insert("instructions".to_owned(), json!(instructions));
        payload.insert("screenshot_path".to_owned(), json!(screenshot_path));
        payload.insert("broker_name".to_owned(), json!(opts.broker_name));
        payload.insert("step_index".to_owned(), json!(opts.step_index));
        payload.insert("total_steps".to_owned(), json!(opts.total_steps));
        payload.insert(
            "error_message".to_owned(),
            json!(redact_identity_values(&opts.error_message, profile)),
        );
        if let Err(error) = append_and_project_tx(
            &transaction,
            request_id,
            &EventType::HumanActionRequired,
            &payload,
            &Source::System,
            now,
        ) {
            rollback_and_cleanup(transaction, &html_path, wrote_html);
            return Err(to_sql_error(error));
        }
    }
    transaction.commit()?;
    Ok(ManualTask {
        id: task_id,
        request_id: opts.request_id,
        broker_id: opts.broker_id.clone(),
        broker_name: opts.broker_name.clone(),
        form_url,
        reason,
        instructions,
        screenshot_path,
        html_snapshot_path: html_path,
        form_fields_json,
        status: "pending".to_owned(),
        created_at: sql_timestamp(now),
        completed_at: None,
        notes: String::new(),
    })
}

/// Best-effort rollback that also removes a snapshot written by this attempt.
/// Consumes the transaction, so it is only called on paths that return.
fn rollback_and_cleanup(transaction: rusqlite::Transaction<'_>, html_path: &str, wrote: bool) {
    let _ = transaction.rollback();
    if wrote && !html_path.is_empty() {
        let _ = fs::remove_file(html_path);
    }
}

fn to_sql_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

const TASK_COLUMNS: &str = "id, request_id, broker_id, broker_name,
     form_url, reason, instructions, screenshot_path, html_snapshot_path,
     form_fields_json, status, created_at, completed_at, notes";

fn scan_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManualTask> {
    let request_id: Option<i64> = row.get(1)?;
    Ok(ManualTask {
        id: row.get(0)?,
        request_id,
        broker_id: row.get(2)?,
        broker_name: row.get(3)?,
        form_url: row.get(4)?,
        reason: row.get(5)?,
        instructions: row.get(6)?,
        screenshot_path: row.get(7)?,
        html_snapshot_path: row.get(8)?,
        form_fields_json: row.get(9)?,
        status: row.get(10)?,
        created_at: driver_timestamp(row.get::<_, String>(11)?),
        completed_at: row.get::<_, Option<String>>(12)?.map(driver_timestamp),
        notes: row.get(13)?,
    })
}

/// The `TIMESTAMP` columns as Go's SQL driver hands them to a `string`.
///
/// `database/sql` converts a declared `TIMESTAMP` into `time.Time` and renders
/// it with `time.RFC3339Nano`, so a stored `2026-01-02 03:04:05` reads back as
/// `2026-01-02T03:04:05Z`. The driver's first two layouts carry a numeric zone
/// offset and differ only in their `T`/space separator, and Go's `time.Parse`
/// also accepts `Z` wherever a layout writes `-07:00`; a parsed offset survives
/// into the rendered value, so only the naive layouts report `Z`. A value none
/// of the layouts accept is passed through unchanged.
fn driver_timestamp(value: String) -> String {
    const NAIVE_LAYOUTS: [&str; 5] = [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d",
    ];
    // `parse_from_rfc3339` is the offset-bearing pair: it accepts both the
    // offset and `Z`, and the space-separated layout reaches it by way of the
    // separator Go's first layout writes.
    let offset_bearing =
        DateTime::parse_from_rfc3339(&value).or_else(|error| match value.split_once(' ') {
            Some((date, time)) => DateTime::parse_from_rfc3339(&format!("{date}T{time}")),
            None => Err(error),
        });
    let instant = if let Ok(parsed) = offset_bearing {
        parsed
    } else if let Some(parsed) = NAIVE_LAYOUTS.iter().find_map(|layout| {
        NaiveDateTime::parse_from_str(&value, layout)
            .ok()
            .or_else(|| {
                NaiveDate::parse_from_str(&value, layout)
                    .ok()
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
            })
    }) {
        parsed.and_utc().fixed_offset()
    } else {
        return value;
    };
    // Go's RFC3339Nano drops the trailing zeros of the fraction and writes a
    // zero offset as `Z`.
    let mut rendered = instant.format("%Y-%m-%dT%H:%M:%S%.9f").to_string();
    if rendered.contains('.') {
        rendered = rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned();
    }
    if instant.offset().local_minus_utc() == 0 {
        rendered.push('Z');
    } else {
        rendered.push_str(&instant.format("%:z").to_string());
    }
    rendered
}

/// Retrieves one task, returning `None` when it does not exist.
pub fn get(store: &Store, task_id: i64) -> rusqlite::Result<Option<ManualTask>> {
    let query = format!("SELECT {TASK_COLUMNS} FROM manual_tasks WHERE id = ?");
    store
        .connection()
        .query_row(&query, params![task_id], scan_task)
        .optional()
}

/// Returns tasks newest first, matching the Python repository ordering.
pub fn list(store: &Store, opts: &ListOpts) -> rusqlite::Result<Vec<ManualTask>> {
    let mut query = format!("SELECT {TASK_COLUMNS} FROM manual_tasks WHERE 1=1");
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(status) = &opts.status
        && !status.is_empty()
    {
        query.push_str(" AND status = ?");
        args.push(Box::new(status.clone()));
    }
    if let Some(request_id) = opts.request_id {
        query.push_str(" AND request_id = ?");
        args.push(Box::new(request_id));
    }
    query.push_str(" ORDER BY created_at DESC");
    let connection = store.connection();
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(
        rusqlite::params_from_iter(args.iter().map(|value| value.as_ref())),
        scan_task,
    )?;
    rows.collect()
}

/// Marks a task completed or cancelled and records `NOTE_ADDED`.
pub fn complete(
    store: &Store,
    task_id: i64,
    notes: &str,
    completed: bool,
    now: DateTime<Utc>,
) -> rusqlite::Result<Option<ManualTask>> {
    let transaction = store.connection().unchecked_transaction()?;
    let query = format!("SELECT {TASK_COLUMNS} FROM manual_tasks WHERE id = ?");
    let existing = transaction
        .query_row(&query, params![task_id], scan_task)
        .optional()?;
    let Some(mut task) = existing else {
        return Ok(None);
    };
    let status = if completed { "completed" } else { "cancelled" };
    let completed_at = sql_timestamp(now);
    transaction.execute(
        "UPDATE manual_tasks SET status = ?, completed_at = ?, notes = ? WHERE id = ?",
        params![status, completed_at, notes, task_id],
    )?;
    if let Some(request_id) = task.request_id
        && request_id != 0
    {
        let mut payload = Map::new();
        payload.insert(
            "note".to_owned(),
            json!(format!("Manual task #{task_id} {status}: {notes}")),
        );
        payload.insert("manual_task_id".to_owned(), json!(task_id));
        payload.insert("form_url".to_owned(), json!(task.form_url));
        append_and_project_tx(
            &transaction,
            request_id,
            &EventType::NoteAdded,
            &payload,
            &Source::User,
            now,
        )
        .map_err(to_sql_error)?;
    }
    transaction.commit()?;
    task.status = status.to_owned();
    task.completed_at = Some(completed_at);
    task.notes = notes.to_owned();
    Ok(Some(task))
}

/// Removes `.png`, `.html` and `.json` artifacts from the task directory. Task
/// rows are intentionally untouched, matching the Python handler.
pub fn cleanup(tasks_dir: &Path, dry_run: bool) -> io::Result<CleanupResult> {
    let mut result = CleanupResult {
        removed: 0,
        skipped: 0,
        dry_run,
    };
    let entries = match fs::read_dir(tasks_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() || !is_artifact(&name) {
            continue;
        }
        if dry_run {
            result.skipped += 1;
            continue;
        }
        fs::remove_file(entry.path())?;
        result.removed += 1;
    }
    Ok(result)
}

fn is_artifact(name: &str) -> bool {
    name.ends_with(".png") || name.ends_with(".html") || name.ends_with(".json")
}

/// Stores a screenshot in the artifact directory with restrictive permissions.
/// An empty screenshot is ignored so evidence capture stays best effort.
pub fn save_screenshot(data: &[u8], now: DateTime<Utc>) -> io::Result<Option<String>> {
    if data.is_empty() {
        return Ok(None);
    }
    let dir = tasks_dir()?;
    ensure_tasks_dir(&dir)?;
    let path = dir.join(format!(
        "screenshot_{}.png",
        now.timestamp_nanos_opt().unwrap_or_default()
    ));
    write_private(&path, data)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
