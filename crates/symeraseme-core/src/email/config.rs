//! IMAP configuration from `IMAP_*` variables. Ported from
//! `internal/email/config.go`, including the rule that a resolved OAuth2 token
//! suppresses password resolution (the raw value is still carried).
//!
//! The loader takes the environment explicitly so callers and parity tests
//! measure the same code, and [`load_imap_config`] is the process-environment
//! wrapper production uses.

use crate::email::types::{ImapConfig, OAuth2Token};
use crate::identity::{OsSecretBackend, SecretResolver};
use std::collections::BTreeMap;

/// Explicit transport overrides. An OAuth2 override is resolved before any
/// password and never falls back to its environment token.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImapConfigOptions {
    pub oauth2_access_token: Option<String>,
    pub oauth2_username: Option<String>,
}

const PASSWORD_ENV: &str = "IMAP_PASSWORD";
const OAUTH2_ENV: &str = "IMAP_OAUTH2_ACCESS_TOKEN";

pub fn load_imap_config() -> Result<ImapConfig, String> {
    load_imap_config_with_options(ImapConfigOptions::default())
}

/// Reads the process environment. Prefer [`load_imap_config_with`] where the
/// environment is already known.
pub fn load_imap_config_with_options(options: ImapConfigOptions) -> Result<ImapConfig, String> {
    load_imap_config_with(options, &process_environment())
}

pub fn process_environment() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

pub fn load_imap_config_with(
    options: ImapConfigOptions,
    environment: &BTreeMap<String, String>,
) -> Result<ImapConfig, String> {
    let port = env_port("IMAP_PORT", 993, environment)?;
    let username = env_value("IMAP_USERNAME", environment).unwrap_or_default();
    let mut oauth_username = env_value("IMAP_OAUTH2_USERNAME", environment).unwrap_or_default();
    if oauth_username.is_empty() {
        oauth_username = username.clone();
    }
    if let Some(explicit) = options
        .oauth2_username
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        oauth_username = explicit.to_string();
    }

    let mut access_token = env_value(OAUTH2_ENV, environment).unwrap_or_default();
    let mut explicit_token = false;
    if let Some(explicit) = options
        .oauth2_access_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        access_token = explicit.to_string();
        explicit_token = true;
    }
    let oauth2 =
        resolve_imap_oauth2_from(&access_token, &oauth_username, explicit_token, environment)?;

    let mut password = env_value(PASSWORD_ENV, environment).unwrap_or_default();
    if oauth2.is_none() && !password.is_empty() {
        let mut resolver = SecretResolver::with_environment(OsSecretBackend, environment.clone());
        resolver.set_env_fallback(PASSWORD_ENV);
        resolver.set_keyring_service("symeraseme-imap");
        resolver.set_keyring_username(PASSWORD_ENV);
        password = resolver
            .resolve(&password)
            .map_err(|error| format!("email: cannot resolve IMAP password: {error}"))?;
    }

    Ok(ImapConfig {
        host: env_default("IMAP_HOST", "imap.gmail.com", environment),
        port,
        username,
        password,
        use_tls: env_bool("IMAP_SSL", true, environment),
        folder: env_default("IMAP_FOLDER", "INBOX", environment),
        since_days: env_int("IMAP_SINCE_DAYS", 14, environment),
        max_messages: env_int("IMAP_MAX_MESSAGES", 50, environment),
        oauth2,
    })
}

pub fn resolve_imap_oauth2(
    access_token: &str,
    username: &str,
) -> Result<Option<OAuth2Token>, String> {
    resolve_imap_oauth2_from(access_token, username, false, &process_environment())
}

/// A secret reference resolves through the shared resolver; a literal token is
/// accepted for compatibility. An explicit argument never falls back to the
/// environment token.
fn resolve_imap_oauth2_from(
    access_token: &str,
    username: &str,
    explicit_token: bool,
    environment: &BTreeMap<String, String>,
) -> Result<Option<OAuth2Token>, String> {
    if access_token.is_empty() {
        return Ok(None);
    }
    let mut resolver = SecretResolver::with_environment(OsSecretBackend, environment.clone());
    resolver.set_keyring_service("symeraseme-oauth2");
    resolver.set_keyring_username(format!("oauth2:{username}:access_token"));
    if !explicit_token {
        resolver.set_env_fallback(OAUTH2_ENV);
    }
    let resolved = resolver
        .resolve(access_token)
        .map_err(|error| format!("email: cannot resolve IMAP OAuth2 access token: {error}"))?;
    if resolved.is_empty() {
        return Err("email: cannot resolve IMAP OAuth2 access token: empty value".to_string());
    }
    Ok(Some(OAuth2Token {
        username: username.to_string(),
        access_token: resolved,
    }))
}

/// The OAuth2 SASL payload. It embeds the token, so it is a secret too.
pub fn xoauth2_payload(username: &str, token: &str) -> String {
    format!("user={username}\u{1}auth=Bearer {token}\u{1}\u{1}")
}

fn env_value(name: &str, environment: &BTreeMap<String, String>) -> Option<String> {
    environment.get(name).cloned()
}

fn env_default(name: &str, fallback: &str, environment: &BTreeMap<String, String>) -> String {
    match environment.get(name).filter(|value| !value.is_empty()) {
        Some(value) => value.clone(),
        None => fallback.to_string(),
    }
}

fn env_bool(name: &str, fallback: bool, environment: &BTreeMap<String, String>) -> bool {
    let Some(value) = environment.get(name) else {
        return fallback;
    };
    match value.trim().to_lowercase().as_str() {
        "" => fallback,
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => fallback,
    }
}

fn env_int(name: &str, fallback: i64, environment: &BTreeMap<String, String>) -> i64 {
    match environment.get(name).map(|value| value.parse::<i64>()) {
        Some(Ok(value)) if value >= 0 => value,
        _ => fallback,
    }
}

fn env_port(
    name: &str,
    fallback: i64,
    environment: &BTreeMap<String, String>,
) -> Result<i64, String> {
    let Some(value) = environment.get(name).filter(|value| !value.is_empty()) else {
        return Ok(fallback);
    };
    match value.parse::<i64>() {
        Ok(port) if (1..=65535).contains(&port) => Ok(port),
        _ => Err(format!("email: {name} must be a valid TCP port")),
    }
}
