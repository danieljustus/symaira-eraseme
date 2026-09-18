//! MCP tool dispatch.
//!
//! Mirrors Go's `ContractHandler`: the MCP layer itself knows nothing about the
//! tools; a handler maps a *validated* `tools/call` to a result, and a handler
//! failure becomes a sanitized `-32603` response.
//!
//! Scope of this slice: only the tools whose Rust cores already exist are
//! wired. `redact_file` is the first one; a catalogue tool that is not wired
//! yet reports that explicitly instead of pretending to be unknown, while a
//! name outside the catalogue (`status`, the legacy alias) reproduces Go's
//! switch default — the handler has no case for it either.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use symeraseme_core::campaign;
use symeraseme_core::config::{ConfigContext, resolve_storage};
use symeraseme_core::manualtasks::{self, ListOpts};
use symeraseme_core::redaction::{read_workspace_file, redact_bytes};
use symeraseme_core::registry::{self, load_embedded, load_from_dir};
use symeraseme_core::storage::Store;
use symeraseme_engine::scheduler;

use super::tools_call::catalogue_has_tool;

/// Go's `ContractHandler` switch default.
const DEFAULT_ERROR: &str = "tool not found";

/// A handler failure. The message reaches the client through
/// [`super::envelope::sanitize_error`], exactly like a Go handler error.
#[derive(Debug, PartialEq, Eq)]
pub struct ToolError(pub String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Maps a validated tool name and its arguments to a result.
pub trait ToolHandler {
    fn call(&self, name: &str, arguments: &Map<String, Value>) -> Result<Value, ToolError>;
}

/// The production handler, mirroring Go's `ContractHandler`.
#[derive(Debug, Clone)]
pub struct ContractHandler {
    /// The workspace the file-reading tools are confined to. Go uses the
    /// process working directory; the caller supplies it here so the guard is
    /// explicit and testable.
    pub workspace_root: PathBuf,
    /// Configuration for the store-backed tools. Go resolves it from the
    /// process environment; the caller supplies it here for the same reason.
    pub config: Option<ConfigContext>,
    /// The instant the writing tools record. Go reads `time.Now()`; `chrono`
    /// has no `clock` feature here, so a tool that needs it fails explicitly
    /// when it was not supplied.
    pub now: Option<DateTime<Utc>>,
    /// The data directory the artifact tools use. Go resolves it from
    /// `SYMERASEME_DATA_DIR`; `None` keeps that behaviour.
    pub data_dir: Option<PathBuf>,
}

impl ContractHandler {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            config: None,
            now: None,
            data_dir: None,
        }
    }

    /// Adds the data directory the artifact tools operate on.
    pub fn with_data_dir(mut self, data_dir: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(data_dir.into());
        self
    }

    /// Adds the configuration the store-backed tools resolve their database
    /// from, plus the instant they record.
    pub fn with_store(mut self, config: ConfigContext, now: DateTime<Utc>) -> Self {
        self.config = Some(config);
        self.now = Some(now);
        self
    }

    /// Opens the store the way Go's `dataStore()` does.
    fn open_store(&self) -> Result<Store, ToolError> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| ToolError("the store-backed tools need a configuration".to_owned()))?;
        let storage = resolve_storage(config).map_err(|error| ToolError(error.to_string()))?;
        std::fs::create_dir_all(&storage.db_dir).map_err(|error| ToolError(error.to_string()))?;
        Store::open(&storage.db_path).map_err(|error| ToolError(error.to_string()))
    }

    fn recorded_instant(&self) -> Result<DateTime<Utc>, ToolError> {
        self.now.ok_or_else(|| {
            ToolError("this tool needs an injected instant and none was supplied".to_owned())
        })
    }

    /// Go's `Result` marshalling: `success`, then `error`, then the flattened
    /// data keys — a `message` is dropped when an error is present.
    fn result_payload(success: bool, error: Option<String>, data: Vec<(&str, Value)>) -> Value {
        let mut payload = Map::new();
        payload.insert("success".to_owned(), json!(success));
        if let Some(error) = &error {
            payload.insert("error".to_owned(), json!(error));
        }
        for (key, value) in data {
            if error.is_some() && key == "message" {
                continue;
            }
            payload.insert(key.to_owned(), value);
        }
        Value::Object(payload)
    }

    /// Go's `HandleList`.
    fn manual_tasks_list(&self, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        let store = self.open_store()?;
        let status = get_str(arguments, "status", "");
        let request_id = get_int(arguments, "request_id", 0);
        let tasks = manualtasks::list(
            &store,
            &ListOpts {
                status: (!status.is_empty()).then_some(status),
                request_id: (request_id != 0).then_some(request_id),
            },
        )
        .map_err(|error| ToolError(error.to_string()))?;

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
        let values: Vec<Value> = tasks.iter().map(task_value).collect();
        Ok(Self::result_payload(
            true,
            None,
            vec![("tasks", json!(values)), ("message", json!(message))],
        ))
    }

    /// Go's `HandleShow`.
    fn manual_tasks_show(&self, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        let store = self.open_store()?;
        let task_id = get_int(arguments, "task_id", 0);
        let task =
            manualtasks::get(&store, task_id).map_err(|error| ToolError(error.to_string()))?;
        let Some(task) = task else {
            return Ok(Self::result_payload(
                false,
                Some(missing_task_message(task_id)),
                Vec::new(),
            ));
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

        let Value::Object(mut data) = task_value(&task) else {
            return Err(ToolError("task payload is an object".to_owned()));
        };
        // Go's Result marshalling flattens the task map next to `success`.
        data.insert("success".to_owned(), json!(true));
        data.insert("message".to_owned(), json!(message));
        Ok(Value::Object(data))
    }

    /// Go's `HandleComplete`.
    fn manual_tasks_complete(&self, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        let store = self.open_store()?;
        let now = self.recorded_instant()?;
        let task_id = get_int(arguments, "task_id", 0);
        let notes = get_str(arguments, "notes", "");
        let task = manualtasks::complete(&store, task_id, &notes, true, now)
            .map_err(|error| ToolError(error.to_string()))?;
        if task.is_none() {
            return Ok(Self::result_payload(
                false,
                Some(missing_task_message(task_id)),
                Vec::new(),
            ));
        }
        Ok(Self::result_payload(
            true,
            None,
            vec![
                ("task_id", json!(task_id)),
                (
                    "message",
                    json!(format!("Manual task #{task_id} marked as completed.")),
                ),
            ],
        ))
    }

    /// Go's `HandleCleanup`.
    fn manual_tasks_cleanup(&self, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        let dry_run = get_bool(arguments, "dry_run", false);
        let directory = manualtasks::tasks_dir_in(self.data_dir.as_deref())
            .map_err(|error| ToolError(error.to_string()))?;
        if !directory.exists() {
            return Ok(Self::result_payload(
                true,
                None,
                vec![(
                    "message",
                    json!("No manual tasks directory found — nothing to clean up."),
                )],
            ));
        }
        let outcome = manualtasks::cleanup(&directory, dry_run)
            .map_err(|error| ToolError(error.to_string()))?;
        let message = if dry_run {
            format!(
                "Would remove {} artifact(s) from {}. Use --yes to confirm.",
                outcome.skipped,
                directory.display()
            )
        } else {
            format!(
                "Removed {} artifact(s) from {}.",
                outcome.removed,
                directory.display()
            )
        };
        Ok(Self::result_payload(
            true,
            None,
            vec![
                ("removed", json!(outcome.removed)),
                ("skipped", json!(outcome.skipped)),
                ("dry_run", json!(outcome.dry_run)),
                ("message", json!(message)),
            ],
        ))
    }

    /// Go's `grant`. The dry-run branch echoes its arguments and touches no
    /// store; the token branches need the consent store and are not part of
    /// this slice, so they report that instead of pretending otherwise.
    fn grant(&self, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        if get_bool(arguments, "dry_run", false) {
            return Ok(json!({
                "success": true,
                "dry_run": true,
                "command": get_str(arguments, "command", "execute"),
                "revoke": get_str(arguments, "revoke", ""),
                "revoke_all": get_bool(arguments, "revoke_all", false),
            }));
        }
        Err(ToolError(
            "the grant token paths are not implemented in this slice".to_owned(),
        ))
    }

    /// Go's `generate_scheduler`. The dry run returns the generated file
    /// contents; writing them to disk is not part of this slice.
    fn generate_scheduler(&self, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        let cfg = scheduler::Config {
            platform: scheduler::Platform::parse(&get_str(arguments, "platform", "")),
            output_dir: get_str(arguments, "output_dir", "./schedules"),
            tick_hour: get_int(arguments, "tick_hour", 10) as i32,
            tick_minute: get_int(arguments, "tick_minute", 0) as i32,
            poll_hours: parse_poll_hours(&get_str(arguments, "poll_hours", ""))?,
            project_dir: get_str(arguments, "project_dir", ""),
            binary_path: get_str(arguments, "symeraseme_bin", ""),
            venv_activate: get_str(arguments, "venv_activate", ""),
        };
        let files = scheduler::generate(&cfg).map_err(|error| ToolError(error.to_string()))?;
        if get_bool(arguments, "dry_run", false) {
            return Ok(json!({
                "success": true,
                "files": files,
                "dry_run": true,
            }));
        }
        Err(ToolError(
            "writing scheduler files is not implemented in this slice".to_owned(),
        ))
    }

    /// Go's `plan_create`: plan against the embedded registry.
    ///
    /// Only the missing-profile branch is implemented — it records an empty
    /// snapshot hash, which is Python's `FileNotFoundError` path. An empty
    /// `profile_path` or an existing profile needs the keyring-backed load and
    /// reports that it is not part of this slice.
    fn plan_create(&self, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        let store = self.open_store()?;
        let now = self.recorded_instant()?;
        let profile_path = get_str(arguments, "profile_path", "");
        if profile_path.is_empty() {
            return Err(ToolError(
                "plan_create without an explicit profile_path is not implemented in this slice"
                    .to_owned(),
            ));
        }
        let resolved = self.workspace_root.join(&profile_path);
        if resolved.exists() {
            return Err(ToolError(
                "reading an existing identity profile is not implemented in this slice".to_owned(),
            ));
        }
        let brokers = load_embedded().map_err(|error| ToolError(error.to_string()))?;
        let result = campaign::plan_campaign(
            &store,
            &brokers,
            "",
            &campaign::PlanOpts {
                campaign_id: get_str(arguments, "campaign_id", ""),
                jurisdiction: get_str(arguments, "jurisdiction", ""),
                law: get_str(arguments, "law", ""),
                priority: get_str(arguments, "priority", ""),
                category: get_str(arguments, "category", ""),
                status: get_str(arguments, "status", "active"),
                include_inactive: get_bool(arguments, "include_inactive", false),
                include_disabled: get_bool(arguments, "include_disabled", false),
                max_brokers: get_int(arguments, "max_brokers", 30),
                notes: get_str(arguments, "notes", ""),
            },
            now,
        )
        .map_err(|error| ToolError(error.to_string()))?;
        serde_json::to_value(result).map_err(|error| ToolError(error.to_string()))
    }

    /// Go's `list_brokers`: the embedded registry through the shared filter.
    ///
    /// Not recorded in the oracle — the answer is the registry itself (1274
    /// brokers, 985 KB with `include_inactive`, 1273 without), and the model and
    /// filter semantics already have byte-exact coverage in the registry
    /// goldens. The handler path is asserted by shape instead.
    fn list_brokers(&self, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        let brokers = load_embedded().map_err(|error| ToolError(error.to_string()))?;
        let status = optional_str(arguments, "status").or_else(|| Some("active".to_owned()));
        let filter = registry::BrokerFilter {
            jurisdiction: optional_str(arguments, "jurisdiction"),
            law: optional_str(arguments, "law"),
            priority: optional_str(arguments, "priority"),
            category: optional_str(arguments, "category"),
            include_disabled: get_bool(arguments, "include_disabled", false),
            status: status.clone(),
            include_inactive: get_bool(arguments, "include_inactive", false),
        };
        let filtered = registry::filter_brokers(&brokers, &filter);
        Ok(json!({
            "schema_version": 1,
            "count": filtered.len(),
            "brokers": filtered,
            "filters": {
                "include_disabled": filter.include_disabled,
                "status": status,
            },
        }))
    }
}

/// Go's `parsePollHours`: empty means "no override", and each value must be a
/// clock hour.
fn parse_poll_hours(raw: &str) -> Result<Vec<i32>, ToolError> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut hours = Vec::new();
    for part in raw.split(',') {
        let trimmed = part.trim();
        let invalid = || {
            ToolError(format!(
                "invalid poll hour {trimmed:?}: choose values from 0 to 23"
            ))
        };
        let value: i32 = trimmed.parse().map_err(|_| invalid())?;
        if !(0..=23).contains(&value) {
            return Err(invalid());
        }
        hours.push(value);
    }
    Ok(hours)
}

impl ToolHandler for ContractHandler {
    fn call(&self, name: &str, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        match name {
            "redact_file" => redact_file(&self.workspace_root, arguments),
            "validate" => validate(&self.workspace_root, arguments),
            "manual_tasks_list" => self.manual_tasks_list(arguments),
            "manual_tasks_show" => self.manual_tasks_show(arguments),
            "manual_tasks_complete" => self.manual_tasks_complete(arguments),
            "manual_tasks_cleanup" => self.manual_tasks_cleanup(arguments),
            "grant" => self.grant(arguments),
            "generate_scheduler" => self.generate_scheduler(arguments),
            "plan_create" => self.plan_create(arguments),
            "list_brokers" => self.list_brokers(arguments),
            other if !catalogue_has_tool(other) => Err(ToolError(DEFAULT_ERROR.to_owned())),
            other => Err(ToolError(format!(
                "tool {other} is not implemented in this slice"
            ))),
        }
    }
}

/// Go's `redact_file`: read through the workspace guard, then redact with the
/// default rules. Read failures stay opaque, matching Go's error text.
fn redact_file(root: &Path, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
    let path = get_str(arguments, "path", "");
    let content = read_workspace_file(Path::new(&path), Some(root))
        .map_err(|error| ToolError(error.to_string()))?;
    let redacted = redact_bytes(&content, None).map_err(|error| ToolError(error.to_string()))?;
    Ok(Value::String(
        String::from_utf8_lossy(&redacted).into_owned(),
    ))
}

/// Go's `getStr`: a value of another type falls back to the default.
fn get_str(arguments: &Map<String, Value>, key: &str, default: &str) -> String {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_owned()
}

/// Go's `getStr` with an empty default, kept as an option: an absent or
/// non-string value means "no filter".
fn optional_str(arguments: &Map<String, Value>, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Go's `getInt`: a missing or non-numeric value falls back to the default.
fn get_int(arguments: &Map<String, Value>, key: &str, default: i64) -> i64 {
    arguments
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or(default)
}

/// Go's `getBool`: a value of another type falls back to the default.
fn get_bool(arguments: &Map<String, Value>, key: &str, default: bool) -> bool {
    arguments
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

/// Go's `taskMap`.
fn task_value(task: &manualtasks::ManualTask) -> Value {
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

/// The shared not-found text of the manual-task tools.
fn missing_task_message(task_id: i64) -> String {
    format!(
        "Manual task #{task_id} not found. Run 'symeraseme manual-tasks list' to see available tasks."
    )
}

/// Go's `validate`: load (and thereby validate) a registry directory, or the
/// embedded registry when no directory is given. A relative directory resolves
/// against the workspace root, which stands in for Go's process working
/// directory.
///
/// The response shape mirrors Go; only the success path is pinned against the
/// oracle so far.
fn validate(root: &Path, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
    let directory = get_str(arguments, "registry_dir", "");
    let brokers = if directory.is_empty() {
        load_embedded()
    } else {
        let requested = Path::new(&directory);
        let resolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            root.join(requested)
        };
        load_from_dir(&resolved)
    }
    .map_err(|error| ToolError(error.to_string()))?;
    Ok(json!({
        "schema_version": 1,
        "ok": true,
        "totals": {
            "valid": brokers.len(),
            "failed": 0,
            "duplicate_ids": 0,
        },
    }))
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{ToolError, ToolHandler};
    use serde_json::{Map, Value};

    /// Reproduces Go's handler-less default answer, so tests that only exercise
    /// the JSON-RPC envelope keep the expectations pinned for that
    /// configuration.
    pub(crate) struct NoBackendHandler;

    impl ToolHandler for NoBackendHandler {
        fn call(&self, _name: &str, _arguments: &Map<String, Value>) -> Result<Value, ToolError> {
            Err(ToolError("tool backend is not available".to_owned()))
        }
    }

    pub(crate) fn no_backend_handler() -> NoBackendHandler {
        NoBackendHandler
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::protocol::{InitializeOutcome, initialize};
    use serde::Deserialize;
    use std::fs;

    #[derive(Deserialize)]
    struct Case {
        name: String,
        request: String,
        response: Option<String>,
    }

    #[derive(Deserialize)]
    struct Fixture {
        source_revision: String,
        cases: Vec<Case>,
    }

    /// A workspace holding the files the fixture reads. Go resolves the
    /// workspace to the process working directory; here the root is injected.
    /// Each test gets its own root so parallel runs cannot delete each other's
    /// files.
    fn workspace(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("eraseme-mcp-003-{}-{name}", std::process::id()));
        fs::create_dir_all(&root).expect("workspace");
        fs::write(
            root.join("pii.txt"),
            "Contact jane.doe@example.com or 555-123-4567.\n",
        )
        .expect("pii fixture");
        fs::write(root.join("clean.txt"), "No personal data here.\n").expect("clean fixture");

        // The `validate` cases point at this relative directory, mirroring the
        // oracle's workspace.
        let registry = root.join("registry");
        for sub in ["schemas", "brokers/eu", "brokers/uk", "brokers/us"] {
            fs::create_dir_all(registry.join(sub)).expect("registry dir");
        }
        fs::write(
            registry.join("manifest.json"),
            r#"{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}"#,
        )
        .expect("manifest");
        fs::write(
            registry.join("schemas/broker.schema.json"),
            r#"{"schema_version":1}"#,
        )
        .expect("schema");
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/registry-contract");
        for (sub, file) in [
            ("eu", "golden-email-eu.yaml"),
            ("uk", "golden-multi-uk.yaml"),
            ("us", "golden-webform-us.yaml"),
        ] {
            fs::copy(
                fixtures.join(file),
                registry.join("brokers").join(sub).join(file),
            )
            .expect("copy broker fixture");
        }
        root
    }

    /// Every case is a response Go's real `ContractHandler` produced, so these
    /// bytes are product behaviour rather than a stub's answer.
    #[test]
    fn source_bound_go_tools_call_fixture_matches() {
        let root = workspace("tools-call");
        let handler = ContractHandler::new(&root);
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/mcp-contract/mcp-003/cases.json"
        ))
        .expect("tools/call fixture");
        assert_eq!(
            fixture.source_revision,
            "79bf23e83b31f18d98487101200eaf32749e5a46"
        );
        assert_eq!(fixture.cases.len(), 8, "fixture case count changed");

        for case in fixture.cases {
            let actual = match initialize(case.request.as_bytes(), &handler) {
                InitializeOutcome::Response(bytes) => Some(String::from_utf8(bytes).unwrap()),
                InitializeOutcome::Notification => None,
                InitializeOutcome::ParseError => {
                    panic!("{} unexpectedly parsed as error", case.name)
                }
            };
            assert_eq!(actual, case.response, "{}", case.name);
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// The manual-task tools answer from an isolated store; the fixture holds
    /// only the cases that carry no wall-clock value (Go fills `created_at`
    /// from `time.Now()` with no injection point, so `list` and the `show`
    /// detail block cannot be pinned).
    #[test]
    fn source_bound_go_manual_task_fixture_matches() {
        use std::collections::BTreeMap;
        use symeraseme_core::config::ConfigContext;
        use symeraseme_core::manualtasks;
        use symeraseme_core::storage::repository::Repository;

        let root = workspace("manual-tasks");
        let data_dir = root.join("data");
        fs::create_dir_all(&data_dir).expect("data dir");
        let now = DateTime::parse_from_rfc3339("2026-08-06T12:00:00+00:00")
            .expect("pinned instant")
            .with_timezone(&Utc);

        let mut environment = BTreeMap::new();
        environment.insert(
            "SYMERASEME_DATA_DIR".to_owned(),
            data_dir.to_string_lossy().into_owned(),
        );
        let config = ConfigContext::new(root.clone(), root.clone(), environment);

        // Seed what the oracle seeded: one request and one pending task.
        let store = Store::open(data_dir.join("symeraseme.db")).expect("open store");
        let request_id = Repository::new(&store)
            .create_removal_request("broker-a", "web_form", "mcp-003", "CCPA", "", "")
            .expect("request");
        let mut opts = manualtasks::CreateOpts {
            request_id: Some(request_id),
            broker_id: "broker-a".to_owned(),
            broker_name: "Broker A".to_owned(),
            form_url: "https://broker-a.example/optout".to_owned(),
            reason: "captcha_failed".to_owned(),
            ..manualtasks::CreateOpts::default()
        };
        opts.html_snapshot.clear();
        let task = manualtasks::create(&store, &opts, None, now).expect("seed task");
        assert_eq!(task.id, 1, "fixture expects the first task id");
        drop(store);

        let handler = ContractHandler::new(&root)
            .with_store(config, now)
            .with_data_dir(&data_dir);

        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/mcp-contract/mcp-manual-tasks/cases.json"
        ))
        .expect("manual-task fixture");
        assert_eq!(
            fixture.source_revision,
            "42614bc27527711baec9b3e9d2805ce1e1dee185"
        );
        assert_eq!(fixture.cases.len(), 3, "fixture case count changed");

        for case in fixture.cases {
            let actual = match initialize(case.request.as_bytes(), &handler) {
                InitializeOutcome::Response(bytes) => Some(String::from_utf8(bytes).unwrap()),
                InitializeOutcome::Notification => None,
                InitializeOutcome::ParseError => {
                    panic!("{} unexpectedly parsed as error", case.name)
                }
            };
            assert_eq!(actual, case.response, "{}", case.name);
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// The paths the oracle cannot pin — `list` and the `show` detail block
    /// carry a wall-clock `created_at` — are asserted by shape instead, plus the
    /// branches the fixture does not reach.
    #[test]
    fn manual_task_shapes_and_unreached_branches() {
        use std::collections::BTreeMap;
        use symeraseme_core::config::ConfigContext;
        use symeraseme_core::manualtasks;
        use symeraseme_core::storage::repository::Repository;

        let root = workspace("manual-shapes");
        let data_dir = root.join("data");
        fs::create_dir_all(&data_dir).expect("data dir");
        let now = DateTime::parse_from_rfc3339("2026-08-06T12:00:00+00:00")
            .expect("pinned instant")
            .with_timezone(&Utc);
        let mut environment = BTreeMap::new();
        environment.insert(
            "SYMERASEME_DATA_DIR".to_owned(),
            data_dir.to_string_lossy().into_owned(),
        );
        let config = ConfigContext::new(root.clone(), root.clone(), environment);

        let store = Store::open(data_dir.join("symeraseme.db")).expect("open store");
        let request_id = Repository::new(&store)
            .create_removal_request("broker-a", "web_form", "mcp-003", "CCPA", "", "")
            .expect("request");
        let opts = manualtasks::CreateOpts {
            request_id: Some(request_id),
            broker_id: "broker-a".to_owned(),
            broker_name: "Broker A".to_owned(),
            form_url: "https://broker-a.example/optout".to_owned(),
            reason: "captcha_failed".to_owned(),
            ..manualtasks::CreateOpts::default()
        };
        manualtasks::create(&store, &opts, None, now).expect("seed task");
        drop(store);

        let handler = ContractHandler::new(&root)
            .with_store(config, now)
            .with_data_dir(&data_dir)
            .clone();

        let call_envelope = |arguments: &str, name: &str| -> Value {
            // Build the request through serde_json so argument values are
            // escaped correctly — a Windows path contains backslashes, which
            // manual string formatting turns into invalid JSON.
            let arguments: Value = serde_json::from_str(arguments).expect("arguments are JSON");
            let request = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            })
            .to_string();
            match initialize(request.as_bytes(), &handler) {
                InitializeOutcome::Response(bytes) => {
                    serde_json::from_slice(&bytes).expect("envelope JSON")
                }
                other => panic!("expected a response for {name}, got {other:?}"),
            }
        };
        // The tool payload is a JSON string inside the content envelope.
        let call = |arguments: &str, name: &str| -> Value {
            let envelope = call_envelope(arguments, name);
            let text = envelope["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("expected a payload, got {envelope}"));
            serde_json::from_str(text).expect("payload JSON")
        };

        // `list`: shape only, because the message embeds created_at.
        let listed = call("{}", "manual_tasks_list");
        assert_eq!(listed["success"], true, "list payload: {listed}");
        assert_eq!(listed["tasks"].as_array().expect("tasks").len(), 1);
        assert_eq!(listed["tasks"][0]["broker_name"], "Broker A");
        assert_eq!(listed["tasks"][0]["reason"], "captcha_failed");
        assert!(
            listed["message"]
                .as_str()
                .expect("message")
                .starts_with("Manual tasks (1):")
        );

        // `show` detail: the message block plus the task map.
        let shown = call(r#"{"task_id":1}"#, "manual_tasks_show");
        assert_eq!(shown["success"], true);
        assert_eq!(shown["id"], 1);
        let message = shown["message"].as_str().expect("message");
        assert!(
            message.contains("Broker:     Broker A (broker-a)"),
            "{message}"
        );
        assert!(message.contains("URL:        https://broker-a.example/optout"));
        assert!(message.contains("Instructions:"));

        // `complete` on a missing task: the shared not-found text.
        let missing = call(r#"{"task_id":999}"#, "manual_tasks_complete");
        assert_eq!(missing["success"], false);
        assert_eq!(
            missing["error"],
            "Manual task #999 not found. Run 'symeraseme manual-tasks list' to see available tasks."
        );
        assert!(missing.get("message").is_none(), "message must be dropped");

        // `cleanup` with an existing directory: counts plus the dry-run text.
        let tasks_dir = data_dir.join("manual_tasks");
        fs::create_dir_all(&tasks_dir).expect("tasks dir");
        fs::write(tasks_dir.join("a.png"), b"png").expect("artifact");
        let cleaned = call(r#"{"dry_run":true}"#, "manual_tasks_cleanup");
        assert_eq!(cleaned["success"], true);
        assert_eq!(cleaned["skipped"], 1);
        assert_eq!(cleaned["removed"], 0);
        assert_eq!(cleaned["dry_run"], true);
        assert!(
            cleaned["message"]
                .as_str()
                .expect("message")
                .starts_with("Would remove 1 artifact(s) from")
        );

        // `validate` without a directory uses the embedded registry, and an
        // absolute directory is honoured as given.
        let embedded = call("{}", "validate");
        assert_eq!(embedded["ok"], true);
        assert!(embedded["totals"]["valid"].as_i64().expect("valid") > 0);
        // The path goes through serde_json so a Windows backslash cannot
        // produce invalid JSON.
        let absolute_arguments =
            json!({"registry_dir": root.join("registry").to_string_lossy()}).to_string();
        let absolute = call(&absolute_arguments, "validate");
        assert_eq!(absolute["totals"]["valid"], 3);

        // A catalogue tool that is not wired yet says so instead of pretending
        // to be unknown.
        // `execute` is in the catalogue but not wired in this slice. Its required
        // argument has to be supplied, otherwise validation answers first — which
        // is the correct order.
        let unwired = call_envelope(r#"{"campaign_id":"c1"}"#, "execute");
        assert!(
            unwired["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("not implemented in this slice"),
            "envelope was {unwired}"
        );

        // The writing tool fails explicitly when no instant was injected.
        let without_clock = ContractHandler::new(&root);
        let outcome = initialize(
            br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"manual_tasks_complete","arguments":{"task_id":1}}}"#,
            &without_clock,
        );
        match outcome {
            InitializeOutcome::Response(bytes) => {
                let text = String::from_utf8(bytes).expect("UTF-8");
                assert!(text.contains("configuration"), "{text}");
            }
            other => panic!("expected a response, got {other:?}"),
        }

        let _ = fs::remove_dir_all(&root);
    }

    /// `plan_create` answers with the removal-request ids it just created, so
    /// the oracle could not record it: its store isolation did not hold and the
    /// ids came from the developer's real store. The shape is asserted here
    /// instead, including both branches this slice does not implement.
    #[test]
    fn plan_create_shape_and_unimplemented_branches() {
        use std::collections::BTreeMap;
        use symeraseme_core::config::ConfigContext;

        let root = workspace("plan-create");
        let data_dir = root.join("data");
        fs::create_dir_all(&data_dir).expect("data dir");
        let now = DateTime::parse_from_rfc3339("2026-08-06T12:00:00+00:00")
            .expect("pinned instant")
            .with_timezone(&Utc);
        let mut environment = BTreeMap::new();
        environment.insert(
            "SYMERASEME_DATA_DIR".to_owned(),
            data_dir.to_string_lossy().into_owned(),
        );
        let config = ConfigContext::new(root.clone(), root.clone(), environment);
        let handler = ContractHandler::new(&root).with_store(config, now);

        let call = |arguments: &str, name: &str| -> Value {
            let arguments: Value = serde_json::from_str(arguments).expect("arguments");
            let request = json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            })
            .to_string();
            match initialize(request.as_bytes(), &handler) {
                InitializeOutcome::Response(bytes) => {
                    serde_json::from_slice(&bytes).expect("envelope JSON")
                }
                other => panic!("expected a response, got {other:?}"),
            }
        };

        let planned = call(
            r#"{"campaign_id":"mcp-plan","max_brokers":2,"profile_path":"missing-profile.enc"}"#,
            "plan_create",
        );
        let payload = &planned["result"]["content"][0]["text"];
        let payload: Value =
            serde_json::from_str(payload.as_str().expect("payload is a JSON string"))
                .expect("payload JSON");
        assert_eq!(payload["campaign_id"], "mcp-plan");
        assert!(payload["total_brokers"].as_i64().expect("total") > 1000);
        assert_eq!(payload["planned"], 2);
        let requests = payload["requests"].as_array().expect("requests");
        assert_eq!(requests.len(), 2);
        // The embedded registry is ordered, so the first two brokers are fixed.
        assert_eq!(requests[0]["broker_id"], "0ptimus-analytics-us");
        assert_eq!(requests[0]["request_id"], 1);
        assert_eq!(requests[1]["request_id"], 2);

        // An empty profile path and an existing profile both need the
        // keyring-backed load, which this slice does not implement.
        for (arguments, expected) in [
            (
                r#"{"campaign_id":"x"}"#,
                "without an explicit profile_path is not implemented",
            ),
            (
                r#"{"campaign_id":"x","profile_path":"registry/manifest.json"}"#,
                "reading an existing identity profile is not implemented",
            ),
        ] {
            let rejected = call(arguments, "plan_create");
            assert!(
                rejected["error"]["message"]
                    .as_str()
                    .expect("error")
                    .contains(expected),
                "{rejected}"
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    /// `list_brokers` answers with the registry itself, so the oracle could not
    /// record it without duplicating ~1 MB of embedded data. The counts and the
    /// filter echo below are the values Go's real handler produced when the
    /// oracle measured them.
    #[test]
    fn list_brokers_shape_matches_the_measured_handler_answers() {
        let root = workspace("list-brokers");
        let handler = ContractHandler::new(&root);
        let call = |arguments: &str| -> Value {
            let arguments: Value = serde_json::from_str(arguments).expect("arguments");
            let request = json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "list_brokers", "arguments": arguments},
            })
            .to_string();
            match initialize(request.as_bytes(), &handler) {
                InitializeOutcome::Response(bytes) => {
                    let envelope: Value = serde_json::from_slice(&bytes).expect("envelope JSON");
                    let text = envelope["result"]["content"][0]["text"]
                        .as_str()
                        .expect("payload text");
                    serde_json::from_str(text).expect("payload JSON")
                }
                other => panic!("expected a response, got {other:?}"),
            }
        };

        // include_inactive makes Go ignore the status filter, so both statuses
        // come back.
        let all = call(r#"{"status":"out-of-business","include_inactive":true}"#);
        assert_eq!(all["schema_version"], 1);
        assert_eq!(all["count"], 1274);
        assert_eq!(all["filters"]["status"], "out-of-business");
        assert_eq!(all["filters"]["include_disabled"], false);
        assert_eq!(all["brokers"][0]["id"], "0ptimus-analytics-us");

        // The default is the active registry.
        let active = call("{}");
        assert_eq!(active["count"], 1273);
        assert_eq!(active["filters"]["status"], "active");

        let _ = fs::remove_dir_all(&root);
    }
}
