use super::model::{Broker, BrokerWire, Channel, FormSpec, FormStep, SolveCaptcha};
use serde_yaml::Value;
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use yaml_rust2::parser::{Event, EventReceiver, Parser};

const MAX_DOCUMENT_BYTES: usize = 1 << 20;
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
    let root = root.as_ref();
    reject_symlink(root)?;
    let brokers_root = root.join("brokers");
    reject_symlink(&brokers_root)?;
    let mut paths = Vec::new();
    let mut budget = LoadBudget::default();
    collect_yaml(&brokers_root, &mut paths, &mut budget, 0)?;
    paths.sort();

    let mut seen_ids = HashSet::new();
    let mut brokers = Vec::with_capacity(paths.len().min(MAX_OUTPUT_BROKERS));
    for path in paths {
        if brokers.len() >= MAX_OUTPUT_BROKERS {
            return Err(validation(
                "registry",
                format!("output broker limit {MAX_OUTPUT_BROKERS} exceeded"),
            ));
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| RegistryError::Validation {
                field: path.display().to_string(),
                message: "filename is not valid UTF-8".to_owned(),
            })?;
        if file_name.starts_with('_') {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| RegistryError::Validation {
                field: path.display().to_string(),
                message: "filename has no valid stem".to_owned(),
            })?;
        if !seen_ids.insert(stem.to_owned()) {
            return Err(validation(
                "id",
                format!("duplicate broker id {stem:?} across directories"),
            ));
        }
        let remaining_bytes = MAX_INPUT_BYTES.saturating_sub(budget.input_bytes);
        let (source, bytes_read) = read_bounded(&path, remaining_bytes)?;
        budget.input_bytes = budget
            .input_bytes
            .checked_add(bytes_read)
            .ok_or_else(|| validation("registry", "aggregate input byte counter overflowed"))?;
        let (broker, nodes) =
            decode_with_metrics(stem, &source).map_err(|error| with_path(error, &path))?;
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

/// Alias matching the loader terminology used by the Go implementation.
pub fn load(root: impl AsRef<Path>) -> Result<Vec<Broker>, RegistryError> {
    load_from_dir(root)
}

pub(crate) fn decode(file_stem: &str, source: &str) -> Result<Broker, RegistryError> {
    decode_with_metrics(file_stem, source).map(|(broker, _)| broker)
}

fn decode_with_metrics(file_stem: &str, source: &str) -> Result<(Broker, usize), RegistryError> {
    preflight_source(source)?;
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
    let mut checker = YamlPreflight::default();
    Parser::new_from_str(source)
        .load(&mut checker, false)
        .map_err(|error| validation("document", error.to_string()))?;
    if checker.saw_anchor || checker.saw_alias {
        return Err(validation(
            "document",
            "YAML anchors and aliases are not allowed",
        ));
    }
    if checker.depth != 0 {
        return Err(validation("document", "YAML nesting is unbalanced"));
    }
    if checker.nodes > MAX_YAML_NODES {
        return Err(validation("document", "YAML node budget exceeded"));
    }
    Ok(())
}

#[derive(Default)]
struct YamlPreflight {
    nodes: usize,
    depth: usize,
    saw_anchor: bool,
    saw_alias: bool,
}

impl EventReceiver for YamlPreflight {
    fn on_event(&mut self, event: Event) {
        match event {
            Event::Scalar(_, _, anchor, _) => {
                self.nodes = self.nodes.saturating_add(1);
                self.saw_anchor |= anchor != 0;
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                self.nodes = self.nodes.saturating_add(1);
                self.depth = self.depth.saturating_add(1);
                self.saw_anchor |= anchor != 0;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                self.depth = self.depth.saturating_sub(1);
            }
            Event::Alias(_) => {
                self.nodes = self.nodes.saturating_add(1);
                self.saw_alias = true;
            }
            Event::Nothing
            | Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart
            | Event::DocumentEnd => {}
        }
    }
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
    let mut stack = vec![root];
    while let Some(value) = stack.pop() {
        match value {
            Value::Mapping(mapping) => {
                for (key, value) in mapping {
                    if key.as_str().is_some_and(is_optional_field) && matches!(value, Value::Null) {
                        return Err(validation(
                            key.as_str().unwrap_or("field"),
                            "explicit YAML null is not allowed; omit the field instead",
                        ));
                    }
                    stack.push(key);
                    stack.push(value);
                }
            }
            Value::Sequence(sequence) => stack.extend(sequence),
            Value::Tagged(tagged) => stack.push(&tagged.value),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Ok(())
}

fn is_optional_field(field: &str) -> bool {
    matches!(
        field,
        "verification"
            | "disabled"
            | "added_date"
            | "source"
            | "status"
            | "notes"
            | "endpoint"
            | "url"
            | "form_spec"
            | "template"
            | "locale"
            | "required_fields"
            | "supports_suppression"
            | "expected_response_days"
            | "ack_keywords"
            | "rejection_keywords"
            | "human_required_keywords"
            | "timeout_seconds"
            | "rate_limit_delay"
            | "headless"
            | "goto"
            | "fill"
            | "select"
            | "click"
            | "wait_for"
            | "wait_seconds"
            | "screenshot"
            | "assert_text"
            | "solve_captcha"
            | "provider"
            | "action"
            | "min_score"
            | "is_invisible"
            | "data_sensitivity"
    )
}

fn reject_symlink(path: &Path) -> Result<(), RegistryError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| RegistryError::Io {
        path: path.to_owned(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(validation(
            "registry layout",
            format!("symlink is not allowed: {}", path.display()),
        ));
    }
    Ok(())
}

fn open_regular(path: &Path) -> Result<File, RegistryError> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(NO_FOLLOW)
            .open(path)
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT prevents following a final reparse point.
        OpenOptions::new()
            .read(true)
            .custom_flags(0x0020_0000)
            .open(path)
    };
    #[cfg(not(any(unix, windows)))]
    let file = OpenOptions::new().read(true).open(path);
    let file = file.map_err(|source| RegistryError::Io {
        path: path.to_owned(),
        source,
    })?;
    let opened_metadata = file.metadata().map_err(|source| RegistryError::Io {
        path: path.to_owned(),
        source,
    })?;
    let path_metadata = fs::symlink_metadata(path).map_err(|source| RegistryError::Io {
        path: path.to_owned(),
        source,
    })?;
    if !opened_metadata.file_type().is_file()
        || !path_metadata.file_type().is_file()
        || opened_metadata.len() != path_metadata.len()
    {
        return Err(validation(
            "registry layout",
            format!(
                "YAML entry is not a stable regular file: {}",
                path.display()
            ),
        ));
    }
    Ok(file)
}

#[cfg(target_os = "linux")]
const NO_FOLLOW: i32 = 0x20000;
#[cfg(target_os = "android")]
const NO_FOLLOW: i32 = 0x20000;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
const NO_FOLLOW: i32 = 0x0100;

fn read_bounded(path: &Path, remaining_bytes: usize) -> Result<(String, usize), RegistryError> {
    if remaining_bytes == 0 {
        return Err(validation(
            "registry",
            format!("aggregate input byte limit {MAX_INPUT_BYTES} exceeded"),
        ));
    }
    let file = open_regular(path)?;
    let metadata = file.metadata().map_err(|source| RegistryError::Io {
        path: path.to_owned(),
        source,
    })?;
    let limit = MAX_DOCUMENT_BYTES.min(remaining_bytes);
    if metadata.len() > limit as u64 {
        let message = if remaining_bytes < MAX_DOCUMENT_BYTES {
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
        let message = if remaining_bytes < MAX_DOCUMENT_BYTES {
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
    path: &Path,
    output: &mut Vec<PathBuf>,
    budget: &mut LoadBudget,
    depth: usize,
) -> Result<(), RegistryError> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err(validation(
            "registry layout",
            format!("directory nesting exceeds {MAX_DIRECTORY_DEPTH} levels"),
        ));
    }
    let entries = fs::read_dir(path).map_err(|source| RegistryError::Io {
        path: path.to_owned(),
        source,
    })?;
    for entry in entries {
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
        let entry = entry.map_err(|source| RegistryError::Io {
            path: path.to_owned(),
            source,
        })?;
        let child = entry.path();
        let file_type = entry.file_type().map_err(|source| RegistryError::Io {
            path: child.clone(),
            source,
        })?;
        let is_yaml = matches!(
            child.extension().and_then(|ext| ext.to_str()),
            Some("yaml" | "yml")
        );
        if file_type.is_symlink() {
            return Err(validation(
                "registry layout",
                format!("symlink is not allowed: {}", child.display()),
            ));
        }
        if is_yaml {
            if !file_type.is_file() {
                return Err(validation(
                    "registry layout",
                    format!("YAML entry is not a regular file: {}", child.display()),
                ));
            }
            let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
                return Err(validation(
                    "registry layout",
                    format!("filename is not valid UTF-8: {}", child.display()),
                ));
            };
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
            output.push(child);
            continue;
        }
        if file_type.is_dir() {
            collect_yaml(&child, output, budget, depth + 1)?;
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

fn with_path(error: RegistryError, path: &Path) -> RegistryError {
    match error {
        RegistryError::Validation { field, message } => {
            validation(path.display().to_string() + ": " + &field, message)
        }
        other => other,
    }
}
