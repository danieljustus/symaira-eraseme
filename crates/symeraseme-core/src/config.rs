//! Configuration precedence and storage-path resolution.
//!
//! The loader is intentionally context-based: callers provide the home
//! directory, current directory, and environment instead of making the core
//! library depend on process-global state. This keeps configuration tests
//! deterministic and leaves process integration to a later CLI slice.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use toml::Value;

const DEFAULT_DATA_DIR: &str = "~/.local/share/symeraseme";
const DEFAULT_PORT: i32 = 8000;
const CONFIG_DIR: &str = "symeraseme";
const CONFIG_FILE: &str = "config.toml";
const PROJECT_FILE: &str = ".symeraseme.toml";
const DATABASE_FILE: &str = "symeraseme.db";

/// Inputs used by [`load`] and [`resolve_storage`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigContext {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub environment: BTreeMap<String, String>,
}

impl ConfigContext {
    /// Creates a context from explicit home, current-directory, and env data.
    pub fn new(
        home_dir: impl Into<PathBuf>,
        current_dir: impl Into<PathBuf>,
        environment: BTreeMap<String, String>,
    ) -> Self {
        Self {
            home_dir: home_dir.into(),
            current_dir: current_dir.into(),
            environment,
        }
    }

    /// Returns a copy with one environment variable set.
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }
}

/// Effective persistent configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Config {
    pub data_dir: String,
    pub db_dir: String,
    pub encrypt_db: bool,
    pub port: i32,
    pub allow_remote: bool,
}

impl Config {
    /// Returns the documented defaults.
    pub fn defaults() -> Self {
        defaults()
    }

    /// Loads this configuration using an explicit context.
    pub fn load(context: &ConfigContext) -> Result<Self, ConfigError> {
        load(context)
    }

    /// Resolves the effective storage paths using an explicit context.
    pub fn resolve_storage(context: &ConfigContext) -> Result<Storage, ConfigError> {
        resolve_storage(context)
    }
}

/// Validated absolute storage configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Storage {
    pub data_dir: PathBuf,
    pub db_dir: PathBuf,
    pub db_path: PathBuf,
    pub temp_dir: PathBuf,
    pub encrypt_db: bool,
}

/// A configuration failure with an optional supported-field classification.
#[derive(Debug)]
pub enum ConfigError {
    Context(&'static str),
    Io {
        scope: &'static str,
    },
    Parse {
        scope: &'static str,
    },
    Field {
        scope: &'static str,
        field: &'static str,
        message: &'static str,
    },
    Validation {
        field: &'static str,
        message: &'static str,
    },
}

impl ConfigError {
    /// Returns the supported configuration field associated with this error.
    pub fn field(&self) -> Option<&'static str> {
        match self {
            Self::Field { field, .. } | Self::Validation { field, .. } => Some(field),
            Self::Context(_) | Self::Io { .. } | Self::Parse { .. } => None,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Context(message) => write!(formatter, "configuration context: {message}"),
            Self::Io { scope } => write!(formatter, "{scope} configuration could not be read"),
            Self::Parse { scope } => write!(formatter, "{scope} configuration is malformed TOML"),
            Self::Field {
                scope,
                field,
                message,
            } => write!(
                formatter,
                "{scope} configuration field {field:?}: {message}"
            ),
            Self::Validation { field, message } => {
                write!(formatter, "configuration field {field:?}: {message}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Returns the documented persistent configuration.
pub fn defaults() -> Config {
    Config {
        data_dir: DEFAULT_DATA_DIR.to_owned(),
        db_dir: String::new(),
        encrypt_db: false,
        port: DEFAULT_PORT,
        allow_remote: false,
    }
}

/// Loads configuration in the order defaults, global file, project file, env.
pub fn load(context: &ConfigContext) -> Result<Config, ConfigError> {
    validate_context(context)?;
    let mut config = defaults();

    let global_path = global_config_path(context);
    merge_file(&mut config, &global_path, "global")?;

    let project_path = context.current_dir.join(PROJECT_FILE);
    merge_file(&mut config, &project_path, "project")?;

    apply_environment(&mut config, context)?;
    validate_config(&config)?;
    Ok(config)
}

/// Resolves all persistent paths and a user-scoped encrypted temp directory.
pub fn resolve_storage(context: &ConfigContext) -> Result<Storage, ConfigError> {
    let config = load(context)?;
    let data_dir = resolve_path(&config.data_dir, "data_dir", context)?;
    let db_dir = if config.db_dir.is_empty() {
        data_dir.clone()
    } else {
        resolve_path(&config.db_dir, "db_dir", context)?
    };
    let temp_dir = default_encrypted_temp_dir(context)?;

    Ok(Storage {
        db_path: db_dir.join(DATABASE_FILE),
        data_dir,
        db_dir,
        temp_dir,
        encrypt_db: config.encrypt_db,
    })
}

/// Returns the user-scoped directory used for encrypted database copies.
pub fn default_encrypted_temp_dir(context: &ConfigContext) -> Result<PathBuf, ConfigError> {
    validate_context(context)?;
    let cache_root = user_cache_dir(context)?;
    Ok(clean_absolute(
        &cache_root.join(CONFIG_DIR).join("database"),
    ))
}

fn validate_context(context: &ConfigContext) -> Result<(), ConfigError> {
    if context.home_dir.as_os_str().is_empty() || !context.home_dir.is_absolute() {
        return Err(ConfigError::Context("home directory must be absolute"));
    }
    if context.current_dir.as_os_str().is_empty() || !context.current_dir.is_absolute() {
        return Err(ConfigError::Context("current directory must be absolute"));
    }
    if context.home_dir.to_string_lossy().contains('\0')
        || context.current_dir.to_string_lossy().contains('\0')
    {
        return Err(ConfigError::Context(
            "directories must not contain NUL bytes",
        ));
    }
    Ok(())
}

fn global_config_path(context: &ConfigContext) -> PathBuf {
    match context.environment.get("XDG_CONFIG_HOME") {
        Some(path) if !path.is_empty() && Path::new(path).is_absolute() => {
            PathBuf::from(path).join(CONFIG_DIR).join(CONFIG_FILE)
        }
        _ => context
            .home_dir
            .join(".config")
            .join(CONFIG_DIR)
            .join(CONFIG_FILE),
    }
}

fn merge_file(config: &mut Config, path: &Path, scope: &'static str) -> Result<(), ConfigError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(ConfigError::Io { scope }),
    };
    let document = contents
        .parse::<Value>()
        .map_err(|_| ConfigError::Parse { scope })?;
    let Some(table) = document.as_table() else {
        return Err(ConfigError::Parse { scope });
    };

    for (key, value) in table {
        if is_supported_key(key) {
            apply_value(config, key, value, scope)?;
        }
    }
    Ok(())
}

fn apply_environment(config: &mut Config, context: &ConfigContext) -> Result<(), ConfigError> {
    const ENV_FIELDS: [(&str, &str); 5] = [
        ("SYMERASEME_DATA_DIR", "data_dir"),
        ("SYMERASEME_DB_DIR", "db_dir"),
        ("SYMERASEME_ENCRYPT_DB", "encrypt_db"),
        ("SYMERASEME_PORT", "port"),
        ("SYMERASEME_ALLOW_REMOTE", "allow_remote"),
    ];

    for (environment_name, field) in ENV_FIELDS {
        let Some(value) = context.environment.get(environment_name) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        apply_value(config, field, &Value::String(value.clone()), "environment").map_err(
            |error| match error {
                ConfigError::Field { field, message, .. } => ConfigError::Field {
                    scope: environment_name,
                    field,
                    message,
                },
                other => other,
            },
        )?;
    }
    Ok(())
}

fn is_supported_key(key: &str) -> bool {
    matches!(
        key,
        "data_dir" | "db_dir" | "encrypt_db" | "port" | "allow_remote"
    )
}

fn apply_value(
    config: &mut Config,
    key: &str,
    value: &Value,
    scope: &'static str,
) -> Result<(), ConfigError> {
    match key {
        "data_dir" | "db_dir" => {
            let Value::String(value) = value else {
                return Err(field_error(scope, key, "must be a string"));
            };
            if value.trim().is_empty() {
                return Err(field_error(scope, key, "must not be empty"));
            }
            if key == "data_dir" {
                config.data_dir = value.clone();
            } else {
                config.db_dir = value.clone();
            }
        }
        "encrypt_db" | "allow_remote" => {
            let parsed = parse_bool(value).map_err(|message| field_error(scope, key, message))?;
            if key == "encrypt_db" {
                config.encrypt_db = parsed;
            } else {
                config.allow_remote = parsed;
            }
        }
        "port" => {
            config.port = parse_int(value).map_err(|message| field_error(scope, key, message))?;
        }
        _ => return Err(field_error(scope, key, "unknown field")),
    }
    Ok(())
}

fn field_error(scope: &'static str, field: &str, message: &'static str) -> ConfigError {
    let field = match field {
        "data_dir" => "data_dir",
        "db_dir" => "db_dir",
        "encrypt_db" => "encrypt_db",
        "port" => "port",
        "allow_remote" => "allow_remote",
        _ => "unknown",
    };
    ConfigError::Field {
        scope,
        field,
        message,
    }
}

fn parse_bool(value: &Value) -> Result<bool, &'static str> {
    match value {
        Value::Boolean(value) => Ok(*value),
        Value::Integer(value) => match value {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("integer boolean must be 0 or 1"),
        },
        Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err("boolean must be true/false, yes/no, on/off, or 1/0"),
        },
        _ => Err("must be a boolean"),
    }
}

fn parse_int(value: &Value) -> Result<i32, &'static str> {
    let parsed = match value {
        Value::Integer(value) => *value,
        Value::String(value) => value
            .trim()
            .parse::<i64>()
            .map_err(|_| "must be an integer")?,
        _ => return Err("must be an integer"),
    };
    i32::try_from(parsed).map_err(|_| "must be an integer")
}

fn validate_config(config: &Config) -> Result<(), ConfigError> {
    if !(1..=65_535).contains(&config.port) {
        return Err(ConfigError::Validation {
            field: "port",
            message: "must be within 1..65535",
        });
    }
    validate_raw_path(&config.data_dir, "data_dir")?;
    if !config.db_dir.is_empty() {
        validate_raw_path(&config.db_dir, "db_dir")?;
    }
    Ok(())
}

fn validate_raw_path(raw: &str, field: &'static str) -> Result<(), ConfigError> {
    if raw.trim().is_empty() {
        return Err(ConfigError::Validation {
            field,
            message: "must not be empty",
        });
    }
    if raw.contains('\0') {
        return Err(ConfigError::Validation {
            field,
            message: "must not contain a NUL byte",
        });
    }
    Ok(())
}

fn resolve_path(
    raw: &str,
    field: &'static str,
    context: &ConfigContext,
) -> Result<PathBuf, ConfigError> {
    validate_raw_path(raw, field)?;
    let path = if raw == "~" {
        context.home_dir.clone()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        context.home_dir.join(rest)
    } else {
        PathBuf::from(raw)
    };
    let path = if path.is_absolute() {
        path
    } else {
        context.current_dir.join(path)
    };
    Ok(clean_absolute(&path))
}

fn user_cache_dir(context: &ConfigContext) -> Result<PathBuf, ConfigError> {
    #[cfg(target_os = "macos")]
    {
        Ok(context.home_dir.join("Library").join("Caches"))
    }

    #[cfg(target_os = "windows")]
    {
        let local_app_data = context.environment.get("LOCALAPPDATA");
        let explicit_temp = context
            .environment
            .get("TEMP")
            .or_else(|| context.environment.get("TMP"));
        windows_cache_root(
            local_app_data.map(String::as_str),
            &context.home_dir,
            explicit_temp.map(String::as_str),
        )
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(xdg_cache_home) = context.environment.get("XDG_CACHE_HOME") {
            if !xdg_cache_home.is_empty() && Path::new(xdg_cache_home).is_absolute() {
                return Ok(PathBuf::from(xdg_cache_home));
            }
        }
        Ok(context.home_dir.join(".cache"))
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_cache_root(
    local_app_data: Option<&str>,
    home_dir: &Path,
    explicit_temp: Option<&str>,
) -> Result<PathBuf, ConfigError> {
    if let Some(local_app_data) = local_app_data
        && !local_app_data.is_empty()
        && Path::new(local_app_data).is_absolute()
    {
        return Ok(PathBuf::from(local_app_data));
    }
    if home_dir.is_absolute() {
        return Ok(home_dir.to_path_buf());
    }
    if let Some(explicit_temp) = explicit_temp
        && !explicit_temp.is_empty()
        && Path::new(explicit_temp).is_absolute()
    {
        return Ok(PathBuf::from(explicit_temp));
    }
    Err(ConfigError::Context(
        "cache directory requires an absolute LOCALAPPDATA, home, or explicit temp directory",
    ))
}

fn clean_absolute(path: &Path) -> PathBuf {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => cleaned.push(prefix.as_os_str()),
            Component::RootDir => cleaned.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = cleaned.pop();
            }
            Component::Normal(part) => cleaned.push(part),
        }
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_absolute_is_lexical() {
        assert_eq!(
            clean_absolute(Path::new("/tmp/symeraseme/../data")),
            PathBuf::from("/tmp/data")
        );
    }

    #[test]
    fn windows_cache_root_prefers_local_app_data_then_home_then_explicit_temp() {
        let local_app_data = Path::new("/tmp/local-app-data");
        let home = Path::new("/tmp/home");
        let temp = Path::new("/tmp/explicit-temp");

        assert_eq!(
            windows_cache_root(
                Some(local_app_data.to_str().unwrap()),
                home,
                Some(temp.to_str().unwrap())
            )
            .unwrap(),
            local_app_data
        );
        assert_eq!(
            windows_cache_root(None, home, Some(temp.to_str().unwrap())).unwrap(),
            home
        );
        assert_eq!(
            windows_cache_root(
                None,
                Path::new("relative-home"),
                Some(temp.to_str().unwrap())
            )
            .unwrap(),
            temp
        );
        assert!(windows_cache_root(None, Path::new("relative-home"), None).is_err());
    }
}
