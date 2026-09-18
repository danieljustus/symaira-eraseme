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

use serde_json::{Map, Value, json};
use symeraseme_core::redaction::{read_workspace_file, redact_bytes};
use symeraseme_core::registry::{load_embedded, load_from_dir};

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
}

impl ContractHandler {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }
}

impl ToolHandler for ContractHandler {
    fn call(&self, name: &str, arguments: &Map<String, Value>) -> Result<Value, ToolError> {
        match name {
            "redact_file" => redact_file(&self.workspace_root, arguments),
            "validate" => validate(&self.workspace_root, arguments),
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
    fn workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("eraseme-mcp-003-{}", std::process::id()));
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
        let root = workspace();
        let handler = ContractHandler::new(&root);
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/mcp-contract/mcp-003/cases.json"
        ))
        .expect("tools/call fixture");
        assert_eq!(
            fixture.source_revision,
            "79bf23e83b31f18d98487101200eaf32749e5a46"
        );
        assert_eq!(fixture.cases.len(), 5, "fixture case count changed");

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
}
