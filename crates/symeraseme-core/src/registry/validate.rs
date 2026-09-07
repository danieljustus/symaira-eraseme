use super::model::{Broker, BrokerWire, Channel, FormSpec, FormStep, SolveCaptcha};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File};
use serde_yaml::Value;
use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use yaml_rust2::scanner::{Scanner, Token, TokenType};

const MAX_DOCUMENT_BYTES: usize = 1 << 20;
const MAX_METADATA_BYTES: usize = 64 << 10;
const SUPPORTED_REGISTRY_SCHEMA_VERSION: u64 = 1;
const BROKER_SCHEMA_PATH: &str = "schemas/broker.schema.json";
const MAX_YAML_NODES: usize = 16_384;
const MAX_YAML_DEPTH: usize = 64;
const MAX_DIRECTORY_DEPTH: usize = 8;
/// Maximum number of filesystem entries traversed by one registry load.
const MAX_DIRECTORY_ENTRIES: usize = 16_384;
/// Maximum number of broker YAML files accepted by one registry load.
const MAX_BROKER_FILES: usize = 4_096;
/// Maximum number of brokers returned by one registry load.
const MAX_OUTPUT_BROKERS: usize = 4_096;
/// Maximum aggregate UTF-8 input read by one registry load.
const MAX_INPUT_BYTES: usize = 16 << 20;
/// Maximum aggregate decoded YAML nodes across one registry load.
const MAX_AGGREGATE_YAML_NODES: usize = 1 << 20;

const BROKER_KEYS: &[&str] = &[
    "id",
    "name",
    "website",
    "category",
    "jurisdictions",
    "laws",
    "data_sensitivity",
    "priority",
    "opt_out",
    "verification",
    "disabled",
    "added_date",
    "source",
    "status",
    "notes",
];
const COMMON_CHANNEL_KEYS: &[&str] = &[
    "type",
    "template",
    "locale",
    "required_fields",
    "supports_suppression",
    "expected_response_days",
    "disabled",
];
const EMAIL_CHANNEL_KEYS: &[&str] = &["endpoint"];
const WEB_FORM_CHANNEL_KEYS: &[&str] = &["url", "form_spec"];
const FORM_SPEC_KEYS: &[&str] = &["steps", "timeout_seconds", "rate_limit_delay", "headless"];
const FORM_STEP_KEYS: &[&str] = &[
    "goto",
    "fill",
    "select",
    "click",
    "wait_for",
    "wait_seconds",
    "screenshot",
    "assert_text",
    "solve_captcha",
];
const CAPTCHA_KEYS: &[&str] = &[
    "type",
    "site_key",
    "provider",
    "action",
    "min_score",
    "is_invisible",
];

/// Errors returned while reading, decoding, or validating registry data.
#[derive(Debug)]
pub enum RegistryError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Yaml(serde_yaml::Error),
    Validation {
        field: String,
        message: String,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "registry I/O at {}: {source}", path.display())
            }
            Self::Yaml(error) => write!(formatter, "registry YAML: {error}"),
            Self::Validation { field, message } => {
                write!(formatter, "registry validation {field:?}: {message}")
            }
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Yaml(error) => Some(error),
            Self::Validation { .. } => None,
        }
    }
}

#[derive(Default)]
struct LoadBudget {
    directory_entries: usize,
    broker_files: usize,
    input_bytes: usize,
    yaml_nodes: usize,
}

/// Loads every non-documentation broker YAML under `registry/brokers`.
pub fn load_from_dir(root: impl AsRef<Path>) -> Result<Vec<Broker>, RegistryError> {
    let root_path = root.as_ref();
    let root_dir =
        Dir::open_ambient_dir(root_path, cap_std::ambient_authority()).map_err(|source| {
            RegistryError::Io {
                path: root_path.to_owned(),
                source,
            }
        })?;
    validate_registry_metadata(&root_dir)?;
    let brokers_dir = root_dir
        .open_dir("brokers")
        .map_err(|source| RegistryError::Io {
            path: root_path.join("brokers"),
            source,
        })?;
    let mut files = Vec::new();
    let mut budget = LoadBudget::default();
    collect_yaml(
        &brokers_dir,
        Path::new("brokers"),
        &mut files,
        &mut budget,
        0,
    )?;

    files.sort_by(|left, right| left.display_path.cmp(&right.display_path));
    let mut seen_ids: HashMap<String, PathBuf> = HashMap::new();
    let mut brokers = Vec::with_capacity(files.len().min(MAX_OUTPUT_BROKERS));
    for pending in files {
        if brokers.len() >= MAX_OUTPUT_BROKERS {
            return Err(validation(
                "registry",
                format!("output broker limit {MAX_OUTPUT_BROKERS} exceeded"),
            ));
        }
        if let Some(previous) = seen_ids.get(&pending.stem) {
            return Err(validation(
                "id",
                format!(
                    "duplicate broker id {:?} in {} and {}",
                    pending.stem,
                    previous.display(),
                    pending.display_path.display()
                ),
            ));
        }
        seen_ids.insert(pending.stem.clone(), pending.display_path.clone());
        let (broker, nodes) = decode_with_metrics(&pending.stem, &pending.content)
            .map_err(|error| with_path(error, &pending.display_path))?;
        budget.yaml_nodes = budget
            .yaml_nodes
            .checked_add(nodes)
            .ok_or_else(|| validation("registry", "aggregate YAML node counter overflowed"))?;
        if budget.yaml_nodes > MAX_AGGREGATE_YAML_NODES {
            return Err(validation(
                "registry",
                format!("aggregate YAML node limit {MAX_AGGREGATE_YAML_NODES} exceeded"),
            ));
        }
        brokers.push(broker);
    }
    brokers.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(brokers)
}

#[derive(serde::Deserialize)]
struct RegistryManifest {
    schema_version: u64,
    schemas: ManifestSchemas,
}

#[derive(serde::Deserialize)]
struct ManifestSchemas {
    broker: String,
}

#[derive(serde::Deserialize)]
struct RegistrySchemaMetadata {
    schema_version: u64,
}

fn validate_registry_metadata(root: &Dir) -> Result<(), RegistryError> {
    let manifest_bytes = read_metadata(root, Path::new("manifest.json"))?;
    let manifest: RegistryManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| validation("manifest.json", format!("is malformed: {error}")))?;
    if manifest.schema_version != SUPPORTED_REGISTRY_SCHEMA_VERSION {
        return Err(validation(
            "manifest.schema_version",
            format!(
                "unsupported version {}; expected {SUPPORTED_REGISTRY_SCHEMA_VERSION}",
                manifest.schema_version
            ),
        ));
    }
    if manifest.schemas.broker != BROKER_SCHEMA_PATH {
        return Err(validation(
            "manifest.schemas.broker",
            format!("must equal {BROKER_SCHEMA_PATH:?}"),
        ));
    }
    let schema_bytes = read_metadata(root, Path::new(BROKER_SCHEMA_PATH))?;
    let schema: RegistrySchemaMetadata = serde_json::from_slice(&schema_bytes)
        .map_err(|error| validation(BROKER_SCHEMA_PATH, format!("is malformed: {error}")))?;
    if schema.schema_version != SUPPORTED_REGISTRY_SCHEMA_VERSION {
        return Err(validation(
            "broker schema.schema_version",
            format!(
                "unsupported version {}; expected {SUPPORTED_REGISTRY_SCHEMA_VERSION}",
                schema.schema_version
            ),
        ));
    }
    if schema.schema_version != manifest.schema_version {
        return Err(validation(
            "schema_version",
            "manifest and broker schema versions do not match",
        ));
    }
    Ok(())
}

fn read_metadata(root: &Dir, path: &Path) -> Result<Vec<u8>, RegistryError> {
    let mut options = cap_fs_ext::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let file = root
        .open_with(path, &options)
        .map_err(|source| RegistryError::Io {
            path: path.to_owned(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| RegistryError::Io {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(validation(
            path.display().to_string(),
            "metadata entry is not a regular file",
        ));
    }
    if metadata.len() > MAX_METADATA_BYTES as u64 {
        return Err(validation(
            path.display().to_string(),
            format!("metadata exceeds {MAX_METADATA_BYTES} bytes"),
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_METADATA_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| RegistryError::Io {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() > MAX_METADATA_BYTES {
        return Err(validation(
            path.display().to_string(),
            format!("metadata exceeds {MAX_METADATA_BYTES} bytes"),
        ));
    }
    Ok(bytes)
}

pub(crate) fn decode(file_stem: &str, source: &str) -> Result<Broker, RegistryError> {
    decode_with_metrics(file_stem, source).map(|(broker, _)| broker)
}

fn decode_with_metrics(file_stem: &str, source: &str) -> Result<(Broker, usize), RegistryError> {
    preflight_source(source)?;
    // #844 boundary: serde_yaml remains the schema decoder for parity. The
    // public yaml-rust2 scanner above fully bounds bytes, nodes, depth, anchors,
    // aliases, and document count before this deprecated dependency is called;
    // it is isolated, not replaced. yaml-rust2 itself is pure Rust.
    let raw: Value = serde_yaml::from_str(source).map_err(RegistryError::Yaml)?;
    let nodes = check_value_budget(&raw)?;
    reject_explicit_nulls(&raw)?;
    validate_object_keys(&raw, BROKER_KEYS, "top-level")?;
    validate_channel_keys(&raw)?;
    let wire: BrokerWire = serde_yaml::from_value(raw).map_err(RegistryError::Yaml)?;
    let broker = wire
        .into_model()
        .map_err(|message| validation("channel", message))?;
    Ok((validate_broker(file_stem, broker)?, nodes))
}

fn preflight_source(source: &str) -> Result<(), RegistryError> {
    if source.len() > MAX_DOCUMENT_BYTES {
        return Err(validation(
            "document",
            format!("exceeds {MAX_DOCUMENT_BYTES} bytes"),
        ));
    }

    let mut scanner = Scanner::new(source.chars());
    let mut state = YamlPreflight::default();
    for Token(_, token) in scanner.by_ref() {
        match token {
            TokenType::StreamStart(_) | TokenType::StreamEnd => {}
            TokenType::DocumentStart => {
                if state.documents > 0 || state.has_content {
                    return Err(validation(
                        "document",
                        "multiple YAML documents are not allowed",
                    ));
                }
                state.documents = 1;
                state.after_document_end = false;
                state.has_content = false;
            }
            TokenType::DocumentEnd => {
                if state.documents == 0 {
                    state.documents = 1;
                }
                if !state.has_content {
                    return Err(validation(
                        "document",
                        "empty YAML documents are not allowed",
                    ));
                }
                state.after_document_end = true;
            }
            TokenType::Alias(_) | TokenType::Anchor(_) => {
                return Err(validation(
                    "document",
                    "YAML anchors and aliases are not allowed",
                ));
            }
            TokenType::BlockSequenceStart
            | TokenType::BlockMappingStart
            | TokenType::FlowSequenceStart
            | TokenType::FlowMappingStart => {
                note_node(&mut state)?;
                state.depth = state
                    .depth
                    .checked_add(1)
                    .ok_or_else(|| validation("document", "YAML depth counter overflowed"))?;
                if state.depth > MAX_YAML_DEPTH {
                    return Err(validation("document", "YAML depth budget exceeded"));
                }
            }
            TokenType::Scalar(_, _) => note_node(&mut state)?,
            TokenType::BlockEnd | TokenType::FlowSequenceEnd | TokenType::FlowMappingEnd => {
                state.depth = state
                    .depth
                    .checked_sub(1)
                    .ok_or_else(|| validation("document", "YAML nesting is unbalanced"))?;
            }
            TokenType::VersionDirective(_, _)
            | TokenType::TagDirective(_, _)
            | TokenType::BlockEntry
            | TokenType::FlowEntry
            | TokenType::Key
            | TokenType::Value
            | TokenType::Tag(_, _) => {
                if state.after_document_end {
                    return Err(validation(
                        "document",
                        "content after explicit YAML document end is not allowed",
                    ));
                }
            }
        }
    }
    if let Some(error) = scanner.get_error() {
        return Err(validation("document", error.to_string()));
    }
    if state.depth != 0 {
        return Err(validation("document", "YAML nesting is unbalanced"));
    }
    if state.documents != 1 || !state.has_content {
        return Err(validation(
            "document",
            "exactly one non-empty YAML document is required",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct YamlPreflight {
    documents: usize,
    nodes: usize,
    depth: usize,
    has_content: bool,
    after_document_end: bool,
}

fn note_node(state: &mut YamlPreflight) -> Result<(), RegistryError> {
    if state.after_document_end {
        return Err(validation(
            "document",
            "content after explicit YAML document end is not allowed",
        ));
    }
    if state.documents == 0 {
        state.documents = 1;
    }
    state.has_content = true;
    state.nodes = state
        .nodes
        .checked_add(1)
        .ok_or_else(|| validation("document", "YAML node counter overflowed"))?;
    if state.nodes > MAX_YAML_NODES {
        return Err(validation("document", "YAML node budget exceeded"));
    }
    Ok(())
}

fn check_value_budget(root: &Value) -> Result<usize, RegistryError> {
    let mut stack = vec![(root, 1usize)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if nodes > MAX_YAML_NODES {
            return Err(validation("document", "YAML node budget exceeded"));
        }
        if depth > MAX_YAML_DEPTH {
            return Err(validation("document", "YAML depth budget exceeded"));
        }
        match value {
            Value::Mapping(mapping) => {
                for (key, value) in mapping {
                    stack.push((key, depth + 1));
                    stack.push((value, depth + 1));
                }
            }
            Value::Sequence(sequence) => {
                for value in sequence {
                    stack.push((value, depth + 1));
                }
            }
            Value::Tagged(tagged) => stack.push((&tagged.value, depth + 1)),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Ok(nodes)
}

fn reject_explicit_nulls(root: &Value) -> Result<(), RegistryError> {
    let mut stack = vec![(root, String::from("$"))];
    while let Some((value, path)) = stack.pop() {
        match value {
            Value::Null => {
                return Err(validation(
                    path,
                    "explicit YAML null is not allowed; omit the field instead",
                ));
            }
            Value::Mapping(mapping) => {
                for (key, value) in mapping {
                    let key_path = key
                        .as_str()
                        .map(|key| format!("$.{key}"))
                        .unwrap_or_else(|| String::from("$[key]"));
                    stack.push((key, format!("{key_path}[name]")));
                    stack.push((value, key_path));
                }
            }
            Value::Sequence(sequence) => {
                for (index, value) in sequence.iter().enumerate() {
                    stack.push((value, format!("{path}[{index}]")));
                }
            }
            Value::Tagged(tagged) => stack.push((&tagged.value, path)),
            Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Ok(())
}

struct PendingDocument {
    stem: String,
    display_path: PathBuf,
    content: String,
}

fn read_bounded(
    file: File,
    path: &Path,
    input_bytes: usize,
) -> Result<(String, usize), RegistryError> {
    if input_bytes >= MAX_INPUT_BYTES {
        return Err(validation(
            "registry",
            format!("aggregate input byte limit {MAX_INPUT_BYTES} exceeded"),
        ));
    }
    let remaining = MAX_INPUT_BYTES - input_bytes;
    let limit = MAX_DOCUMENT_BYTES.min(remaining);
    let metadata = file.metadata().map_err(|source| RegistryError::Io {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(validation(
            "registry layout",
            format!("YAML entry is not a regular file: {}", path.display()),
        ));
    }
    if metadata.len() > limit as u64 {
        let message = if remaining < MAX_DOCUMENT_BYTES {
            format!("aggregate input byte limit {MAX_INPUT_BYTES} exceeded")
        } else {
            format!("document exceeds {MAX_DOCUMENT_BYTES} bytes")
        };
        return Err(validation(path.display().to_string(), message));
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| RegistryError::Io {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() > limit {
        let message = if remaining < MAX_DOCUMENT_BYTES {
            format!("aggregate input byte limit {MAX_INPUT_BYTES} exceeded")
        } else {
            format!("document exceeds {MAX_DOCUMENT_BYTES} bytes")
        };
        return Err(validation(path.display().to_string(), message));
    }
    let bytes_read = bytes.len();
    let source = String::from_utf8(bytes).map_err(|error| RegistryError::Io {
        path: path.to_owned(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
    })?;
    Ok((source, bytes_read))
}

fn collect_yaml(
    dir: &Dir,
    display_dir: &Path,
    output: &mut Vec<PendingDocument>,
    budget: &mut LoadBudget,
    depth: usize,
) -> Result<(), RegistryError> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err(validation(
            "registry layout",
            format!("directory nesting exceeds {MAX_DIRECTORY_DEPTH} levels"),
        ));
    }
    let mut entries = Vec::new();
    for entry in dir.entries().map_err(|source| RegistryError::Io {
        path: display_dir.to_owned(),
        source,
    })? {
        let entry = entry.map_err(|source| RegistryError::Io {
            path: display_dir.to_owned(),
            source,
        })?;
        budget.directory_entries = budget
            .directory_entries
            .checked_add(1)
            .ok_or_else(|| validation("registry", "directory entry counter overflowed"))?;
        if budget.directory_entries > MAX_DIRECTORY_ENTRIES {
            return Err(validation(
                "registry",
                format!("directory entry limit {MAX_DIRECTORY_ENTRIES} exceeded"),
            ));
        }
        entries.push(entry);
    }
    entries.sort_by_key(|left| left.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            validation(
                "registry layout",
                format!(
                    "filename is not valid UTF-8 under {}",
                    display_dir.display()
                ),
            )
        })?;
        let display_path = display_dir.join(name);
        let file_type = entry.file_type().map_err(|source| RegistryError::Io {
            path: display_path.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            return Err(validation(
                "registry layout",
                format!("symlink is not allowed: {}", display_path.display()),
            ));
        }
        let is_yaml = matches!(
            Path::new(name)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("yaml" | "yml")
        );
        if is_yaml {
            if !file_type.is_file() {
                return Err(validation(
                    "registry layout",
                    format!(
                        "YAML entry is not a regular file: {}",
                        display_path.display()
                    ),
                ));
            }
            if name.starts_with('_') {
                continue;
            }
            budget.broker_files = budget
                .broker_files
                .checked_add(1)
                .ok_or_else(|| validation("registry", "broker file counter overflowed"))?;
            if budget.broker_files > MAX_BROKER_FILES {
                return Err(validation(
                    "registry",
                    format!("broker YAML file limit {MAX_BROKER_FILES} exceeded"),
                ));
            }
            let stem = Path::new(name)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| validation("registry layout", "filename has no valid stem"))?
                .to_owned();
            let mut options = cap_fs_ext::OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No).nonblock(true);
            let file = entry
                .open_with(&options)
                .map_err(|source| RegistryError::Io {
                    path: display_path.clone(),
                    source,
                })?;
            let (content, bytes_read) = read_bounded(file, &display_path, budget.input_bytes)?;
            budget.input_bytes = budget
                .input_bytes
                .checked_add(bytes_read)
                .ok_or_else(|| validation("registry", "aggregate input byte counter overflowed"))?;
            output.push(PendingDocument {
                stem,
                display_path,
                content,
            });
            continue;
        }
        if file_type.is_dir() {
            let child = entry.open_dir().map_err(|source| RegistryError::Io {
                path: display_path.clone(),
                source,
            })?;
            collect_yaml(&child, &display_path, output, budget, depth + 1)?;
        }
    }
    Ok(())
}

fn validate_broker(file_stem: &str, broker: Broker) -> Result<Broker, RegistryError> {
    if broker.id != file_stem {
        return Err(validation(
            "id",
            format!("{:?} does not equal file stem {:?}", broker.id, file_stem),
        ));
    }
    if broker.id.is_empty()
        || !broker
            .id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(validation("id", "does not match ^[a-z0-9-]+$"));
    }
    if broker.name.trim().is_empty() {
        return Err(validation("name", "is required"));
    }
    valid_uri("website", &broker.website)?;
    if broker.jurisdictions.is_empty() {
        return Err(validation("jurisdictions", "must have at least 1 entry"));
    }
    if broker.laws.is_empty() {
        return Err(validation("laws", "must have at least 1 entry"));
    }
    if !(1..=5).contains(&broker.data_sensitivity) {
        return Err(validation("data_sensitivity", "out of range 1..5"));
    }
    if broker.opt_out.is_empty() {
        return Err(validation("opt_out", "must have at least 1 channel"));
    }
    for (index, channel) in broker.opt_out.iter().enumerate() {
        validate_channel(channel).map_err(|error| prefix(error, format!("opt_out[{index}]")))?;
    }
    if let Some(added_date) = &broker.added_date
        && !is_iso_date(added_date)
    {
        return Err(validation(
            "added_date",
            format!("{added_date:?} is not ISO 8601 date"),
        ));
    }
    Ok(broker)
}

fn validate_channel(channel: &Channel) -> Result<(), RegistryError> {
    let (locale, expected_response_days) = match channel {
        Channel::Email {
            endpoint,
            locale,
            expected_response_days,
            ..
        } => {
            if !valid_email(endpoint) {
                return Err(validation(
                    "endpoint",
                    format!("{endpoint:?} is not a valid email"),
                ));
            }
            (locale, expected_response_days)
        }
        Channel::WebForm {
            url,
            form_spec,
            locale,
            expected_response_days,
            ..
        } => {
            valid_uri("url", url)?;
            validate_form_spec(form_spec)?;
            (locale, expected_response_days)
        }
    };
    if let Some(locale) = locale
        && !is_locale(locale)
    {
        return Err(validation(
            "locale",
            format!("{locale:?} does not match RFC 5646 pattern"),
        ));
    }
    if let Some(days) = expected_response_days
        && *days < 1
    {
        return Err(validation("expected_response_days", "must be >= 1"));
    }
    Ok(())
}

fn validate_form_spec(form_spec: &FormSpec) -> Result<(), RegistryError> {
    if form_spec.steps.is_empty() {
        return Err(validation("form_spec", "requires at least 1 step"));
    }
    if let Some(timeout) = form_spec.timeout_seconds
        && (!timeout.is_finite() || timeout < 1.0)
    {
        return Err(validation("timeout_seconds", "must be >= 1"));
    }
    if let Some(delay) = form_spec.rate_limit_delay
        && (!delay.is_finite() || delay < 0.0)
    {
        return Err(validation("rate_limit_delay", "must be >= 0"));
    }
    for (index, step) in form_spec.steps.iter().enumerate() {
        validate_form_step(step).map_err(|error| prefix(error, format!("steps[{index}]")))?;
    }
    Ok(())
}

fn validate_form_step(step: &FormStep) -> Result<(), RegistryError> {
    for (field, value) in [
        ("goto", step.goto.as_deref()),
        ("click", step.click.as_deref()),
        ("wait_for", step.wait_for.as_deref()),
        ("screenshot", step.screenshot.as_deref()),
        ("assert_text", step.assert_text.as_deref()),
    ] {
        if value.is_some_and(str::is_empty) {
            return Err(validation(field, "must not be empty when present"));
        }
    }
    let present = step.goto.as_deref().is_some_and(|value| !value.is_empty())
        || step.fill.as_ref().is_some_and(|value| !value.is_empty())
        || step.select.as_ref().is_some_and(|value| !value.is_empty())
        || step.click.as_deref().is_some_and(|value| !value.is_empty())
        || step
            .wait_for
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || step.wait_seconds.is_some()
        || step
            .screenshot
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || step
            .assert_text
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || step.solve_captcha.is_some();
    if !present {
        return Err(validation("step", "must contain at least 1 action"));
    }
    if let Some(wait) = step.wait_seconds
        && (!wait.is_finite() || wait < 0.0)
    {
        return Err(validation("wait_seconds", "must be >= 0"));
    }
    for selectors in [step.fill.as_ref(), step.select.as_ref()]
        .into_iter()
        .flatten()
    {
        for selector in selectors.keys() {
            if !is_selector(selector) {
                return Err(validation(
                    "selector",
                    format!("{selector:?} does not look like a CSS selector"),
                ));
            }
        }
    }
    if let Some(captcha) = &step.solve_captcha {
        validate_captcha(captcha)?;
    }
    Ok(())
}

fn validate_captcha(captcha: &SolveCaptcha) -> Result<(), RegistryError> {
    if captcha.site_key.len() < 8 {
        return Err(validation(
            "solve_captcha.site_key",
            "must be at least 8 chars",
        ));
    }
    if let Some(score) = captcha.min_score
        && (!score.is_finite() || !(0.0..=1.0).contains(&score))
    {
        return Err(validation("solve_captcha.min_score", "out of range 0..1"));
    }
    Ok(())
}

fn validate_channel_keys(raw: &Value) -> Result<(), RegistryError> {
    let Some(mapping) = raw.as_mapping() else {
        return Ok(());
    };
    let Some(opt_out) = mapping.get(Value::String("opt_out".to_owned())) else {
        return Ok(());
    };
    let Some(channels) = opt_out.as_sequence() else {
        return Ok(());
    };
    for channel in channels {
        let Some(channel_map) = channel.as_mapping() else {
            continue;
        };
        let kind = channel_map
            .get(Value::String("type".to_owned()))
            .and_then(Value::as_str)
            .unwrap_or("");
        let mut allowed = COMMON_CHANNEL_KEYS.to_vec();
        allowed.extend(match kind {
            "email" => EMAIL_CHANNEL_KEYS,
            "web_form" => WEB_FORM_CHANNEL_KEYS,
            _ => &[],
        });
        validate_object_keys(channel, &allowed, "channel")?;
        if kind == "web_form"
            && let Some(form_spec) = channel_map.get(Value::String("form_spec".to_owned()))
        {
            validate_form_spec_keys(form_spec)?;
        }
    }
    Ok(())
}

fn validate_form_spec_keys(raw: &Value) -> Result<(), RegistryError> {
    validate_object_keys(raw, FORM_SPEC_KEYS, "form_spec")?;
    let Some(mapping) = raw.as_mapping() else {
        return Ok(());
    };
    if let Some(steps) = mapping
        .get(Value::String("steps".to_owned()))
        .and_then(Value::as_sequence)
    {
        for step in steps {
            validate_object_keys(step, FORM_STEP_KEYS, "form_step")?;
            if let Some(step_map) = step.as_mapping()
                && let Some(captcha) = step_map.get(Value::String("solve_captcha".to_owned()))
            {
                validate_object_keys(captcha, CAPTCHA_KEYS, "solve_captcha")?;
            }
        }
    }
    Ok(())
}

fn validate_object_keys(
    raw: &Value,
    allowed: &[&str],
    boundary: &str,
) -> Result<(), RegistryError> {
    let Some(mapping) = raw.as_mapping() else {
        return Ok(());
    };
    for key in mapping.keys() {
        let Some(key) = key.as_str() else {
            return Err(validation(boundary, "object keys must be strings"));
        };
        if !allowed.contains(&key) {
            return Err(validation(boundary, format!("unknown field {key:?}")));
        }
    }
    Ok(())
}

fn valid_uri(field: &str, value: &str) -> Result<(), RegistryError> {
    // `format: uri` remains a compatibility annotation: match the Go oracle's
    // non-empty runtime rule until the coordinated #843 corpus cleanup.
    if value.is_empty() {
        return Err(validation(field, "is required"));
    }
    Ok(())
}

fn valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !value.chars().any(|character| character.is_whitespace())
        && !local.contains('@')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn is_iso_date(value: &str) -> bool {
    if value.len() != 10
        || value.as_bytes()[4] != b'-'
        || value.as_bytes()[7] != b'-'
        || !value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return false;
    }
    let bytes = value.as_bytes();
    let year = u32::from(bytes[0] - b'0') * 1000
        + u32::from(bytes[1] - b'0') * 100
        + u32::from(bytes[2] - b'0') * 10
        + u32::from(bytes[3] - b'0');
    let month = u32::from(bytes[5] - b'0') * 10 + u32::from(bytes[6] - b'0');
    let day = u32::from(bytes[8] - b'0') * 10 + u32::from(bytes[9] - b'0');
    if !(1..=12).contains(&month) {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day)
}

fn is_locale(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() == 2 && bytes.iter().all(u8::is_ascii_lowercase))
        || (bytes.len() == 5
            && bytes[2] == b'-'
            && bytes[..2].iter().all(u8::is_ascii_lowercase)
            && bytes[3..].iter().all(u8::is_ascii_uppercase))
}

fn is_selector(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, 'a'..='z' | 'A'..='Z' | '[' | '#' | '.' | '*') {
        return false;
    }
    chars.all(|character| {
        matches!(
            character,
            'a'..='z'
                | 'A'..='Z'
                | '0'..='9'
                | '['
                | ']'
                | '='
                | '.'
                | '\''
                | '"'
                | '_'
                | ':'
                | '#'
                | '-'
                | ' '
                | '*'
                | '>'
                | ','
                | '~'
                | '+'
        )
    })
}

fn validation(field: impl Into<String>, message: impl Into<String>) -> RegistryError {
    RegistryError::Validation {
        field: field.into(),
        message: message.into(),
    }
}

fn prefix(error: RegistryError, field: String) -> RegistryError {
    match error {
        RegistryError::Validation {
            field: inner,
            message,
        } => validation(format!("{field}.{inner}"), message),
        other => other,
    }
}

pub(crate) fn with_path(error: RegistryError, path: &Path) -> RegistryError {
    match error {
        RegistryError::Validation { field, message } => {
            validation(path.display().to_string() + ": " + &field, message)
        }
        other => other,
    }
}
