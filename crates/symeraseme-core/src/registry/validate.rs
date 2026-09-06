use super::model::{Broker, Channel, FormSpec, FormStep, SolveCaptcha};
use serde_yaml::Value;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

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

/// Loads every non-documentation broker YAML under `registry/brokers`.
pub fn load_from_dir(root: impl AsRef<Path>) -> Result<Vec<Broker>, RegistryError> {
    let brokers_root = root.as_ref().join("brokers");
    let mut paths = Vec::new();
    collect_yaml(&brokers_root, &mut paths)?;
    paths.sort();

    let mut brokers = Vec::with_capacity(paths.len());
    for path in paths {
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
        let source = fs::read_to_string(&path).map_err(|source| RegistryError::Io {
            path: path.clone(),
            source,
        })?;
        let broker = decode(stem, &source).map_err(|error| with_path(error, &path))?;
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
    let raw: Value = serde_yaml::from_str(source).map_err(RegistryError::Yaml)?;
    validate_object_keys(&raw, BROKER_KEYS, "top-level")?;
    validate_channel_keys(&raw)?;
    let broker: Broker = serde_yaml::from_value(raw).map_err(RegistryError::Yaml)?;
    validate_broker(file_stem, broker)
}

fn collect_yaml(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), RegistryError> {
    let entries = fs::read_dir(path).map_err(|source| RegistryError::Io {
        path: path.to_owned(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| RegistryError::Io {
            path: path.to_owned(),
            source,
        })?;
        let child = entry.path();
        if child.is_dir() {
            collect_yaml(&child, output)?;
            continue;
        }
        let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with('_') {
            continue;
        }
        if matches!(
            child.extension().and_then(|ext| ext.to_str()),
            Some("yaml" | "yml")
        ) {
            output.push(child);
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
        && timeout < 1.0
    {
        return Err(validation("timeout_seconds", "must be >= 1"));
    }
    if let Some(delay) = form_spec.rate_limit_delay
        && delay < 0.0
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
        && wait < 0.0
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
        && !(0.0..=1.0).contains(&score)
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
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
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
