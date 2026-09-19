//! OAuth2 authorization and token endpoints for mailbox providers. Ported from
//! `internal/email/oauth2.go`.
//!
//! Two properties of the Go code are load-bearing and are preserved here:
//!
//! * Authorization states are one-time values on disk with restrictive
//!   permissions and atomic replacement, and they are consumed on the first
//!   validation attempt — even when they turn out to be expired.
//! * Provider error bodies are never echoed: a failed token request reports the
//!   HTTP status only, because the body can repeat credentials or the
//!   authorization code.
//!
//! The random parts of an authorization URL (state, PKCE verifier) cannot be
//! pinned by value. The oracle records their lengths, the parameter map with
//! both replaced by a marker, and that the challenge is the S256 derivation of
//! the verifier that was returned.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Prefix of every OAuth2 error that is not a state error.
pub const OAUTH2_ERROR_PREFIX: &str = "email: oauth2 error";
/// Prefix of every state store error.
pub const OAUTH2_STATE_PREFIX: &str = "email: oauth2 state error";

const DEFAULT_STATE_TTL_SECONDS: i64 = 300;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Provider endpoints and scopes. Byte-for-byte the Go table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderConfig {
    #[serde(rename = "AuthURL")]
    pub auth_url: String,
    #[serde(rename = "TokenURL")]
    pub token_url: String,
    #[serde(rename = "Scopes")]
    pub scopes: String,
}

pub fn provider(provider: &str) -> Option<ProviderConfig> {
    let key = provider.trim().to_lowercase();
    match key.as_str() {
        "gmail" => Some(ProviderConfig {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            scopes: "https://mail.google.com/ https://www.googleapis.com/auth/gmail.send".to_string(),
        }),
        "outlook" => Some(ProviderConfig {
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".to_string(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".to_string(),
            scopes: "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access".to_string(),
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthError(String);

impl fmt::Display for OAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for OAuthError {}

impl OAuthError {
    pub fn oauth2(message: impl AsRef<str>) -> Self {
        Self(format!("{OAUTH2_ERROR_PREFIX}: {}", message.as_ref()))
    }

    pub fn state(message: impl AsRef<str>) -> Self {
        Self(format!("{OAUTH2_STATE_PREFIX}: {}", message.as_ref()))
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

/// A token endpoint reply, reduced to what the contract needs.
pub struct TokenReply {
    pub status: u16,
    pub body: String,
}

/// The transport seam for token requests. Any transport failure is opaque —
/// Go reduces it to `... failed` so a provider error body can never leak into a
/// message — and so is this error.
pub struct TokenTransportError;

impl fmt::Debug for TokenTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("token transport failed")
    }
}

impl fmt::Display for TokenTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("token transport failed")
    }
}

impl std::error::Error for TokenTransportError {}

pub trait TokenTransport {
    fn post_form(
        &self,
        endpoint: &str,
        form: &str,
        timeout: Duration,
    ) -> Result<TokenReply, TokenTransportError>;
}

/// Production transport. Token endpoints are fixed HTTPS URLs; plain HTTP is
/// admitted only for loopback hosts so a test transport cannot turn the
/// production route into cleartext.
pub struct UreqTokenTransport;

impl TokenTransport for UreqTokenTransport {
    fn post_form(
        &self,
        endpoint: &str,
        form: &str,
        timeout: Duration,
    ) -> Result<TokenReply, TokenTransportError> {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(!is_loopback(endpoint))
            .timeout_global(Some(timeout))
            .timeout_connect(Some(timeout))
            .timeout_recv_response(Some(timeout))
            .timeout_recv_body(Some(timeout))
            .max_redirects(0)
            .max_redirects_will_error(true)
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let mut response = agent
            .post(endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(form.as_bytes())
            .map_err(|_| TokenTransportError)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|_| TokenTransportError)?;
        Ok(TokenReply { status, body })
    }
}

pub fn is_loopback(endpoint: &str) -> bool {
    let without_scheme = endpoint.split("://").nth(1).unwrap_or(endpoint);
    let authority = without_scheme.split(['/', '?']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host == "::1"
}

/// One-time authorization states on disk.
pub struct OAuthStateStore {
    pub path: PathBuf,
    ttl_seconds: i64,
    now_unix: Option<Box<dyn Fn() -> i64 + Send + Sync>>,
    lock: Mutex<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StateRecord {
    provider: String,
    expires_at: i64,
}

impl OAuthStateStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            ttl_seconds: DEFAULT_STATE_TTL_SECONDS,
            now_unix: None,
            lock: Mutex::new(()),
        }
    }

    pub fn with_ttl_seconds(mut self, ttl_seconds: i64) -> Self {
        self.ttl_seconds = ttl_seconds;
        self
    }

    pub fn with_now_unix(mut self, now: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.now_unix = Some(Box::new(now));
        self
    }

    fn now(&self) -> i64 {
        match &self.now_unix {
            Some(now) => now(),
            None => current_unix(),
        }
    }

    fn ttl(&self) -> i64 {
        if self.ttl_seconds > 0 {
            self.ttl_seconds
        } else {
            DEFAULT_STATE_TTL_SECONDS
        }
    }

    pub fn store(&self, state: &str, provider: &str) -> Result<(), OAuthError> {
        if state.is_empty() || provider.is_empty() {
            return Err(OAuthError::state("state and provider are required"));
        }
        let expires_at = self.now() + self.ttl();
        self.update(|records| {
            records.insert(
                state.to_string(),
                StateRecord {
                    provider: provider.to_string(),
                    expires_at,
                },
            );
            Ok(())
        })
    }

    /// Consumes the state: a matching record is deleted even when it is expired.
    pub fn validate(&self, state: &str) -> Result<(), OAuthError> {
        if state.is_empty() {
            return Err(OAuthError::state("missing state (possible CSRF attack)"));
        }
        let now = self.now();
        self.update(|records| {
            let Some(record) = records.remove(state) else {
                return Err(OAuthError::state("state mismatch (possible CSRF attack)"));
            };
            if record.expires_at < now {
                return Err(OAuthError::state("state expired (possible CSRF attack)"));
            }
            Ok(())
        })
    }

    fn update(
        &self,
        apply: impl FnOnce(&mut BTreeMap<String, StateRecord>) -> Result<(), OAuthError>,
    ) -> Result<(), OAuthError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| OAuthError::state("state lock poisoned"))?;
        if self.path.as_os_str().is_empty() {
            return Err(OAuthError::state("state path is empty"));
        }
        let directory = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        create_private_directory(&directory)
            .map_err(|error| OAuthError::state(format!("state directory: {error}")))?;
        let mut records = BTreeMap::new();
        match std::fs::read(&self.path) {
            Ok(raw) if !raw.is_empty() => {
                records = serde_json::from_slice(&raw)
                    .map_err(|_| OAuthError::state("invalid state file"))?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(OAuthError::state(format!("read state file: {error}")));
            }
        }
        apply(&mut records)?;
        let raw = serde_json::to_vec(&records)
            .map_err(|error| OAuthError::state(format!("encode state file: {error}")))?;
        // Atomic replacement through a private temporary file in the same
        // directory, so a reader never sees a half-written state list.
        let mut temp = tempfile::Builder::new()
            .prefix(".oauth2-state-")
            .tempfile_in(&directory)
            .map_err(|error| OAuthError::state(format!("create state file: {error}")))?;
        set_private_file_mode(temp.as_file())
            .map_err(|error| OAuthError::state(format!("state permissions: {error}")))?;
        temp.write_all(&raw)
            .map_err(|error| OAuthError::state(format!("write state file: {error}")))?;
        temp.as_file()
            .sync_all()
            .map_err(|error| OAuthError::state(format!("close state file: {error}")))?;
        let _ = set_private_file_mode_path(&self.path);
        temp.persist(&self.path)
            .map_err(|error| OAuthError::state(format!("commit state file: {error}")))
            .map(|_| ())
    }
}

/// Path Go would use: `$XDG_DATA_HOME/symeraseme/oauth2_state.json`, else
/// `~/.local/share/symeraseme/oauth2_state.json`.
pub fn default_state_path() -> PathBuf {
    match std::env::var_os("XDG_DATA_HOME") {
        Some(base) if !base.is_empty() => PathBuf::from(base)
            .join("symeraseme")
            .join("oauth2_state.json"),
        _ => {
            let home = std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .or_else(|| std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()));
            match home {
                Some(home) => PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("symeraseme")
                    .join("oauth2_state.json"),
                None => PathBuf::from(".")
                    .join(".symeraseme")
                    .join("oauth2_state.json"),
            }
        }
    }
}

pub struct OAuth2Client {
    pub states: OAuthStateStore,
    pub timeout: Duration,
}

impl OAuth2Client {
    pub fn new(states: OAuthStateStore) -> Self {
        Self {
            states,
            timeout: REQUEST_TIMEOUT,
        }
    }

    /// Returns the provider authorization URL and the PKCE verifier that must be
    /// kept for the token exchange.
    pub fn authorise_url(
        &self,
        provider_name: &str,
        client_id: &str,
        redirect_uri: &str,
    ) -> Result<(String, String), OAuthError> {
        let config = provider(provider_name)
            .ok_or_else(|| OAuthError::oauth2(format!("unknown provider {provider_name:?}")))?;
        let mut verifier_bytes = [0u8; 64];
        rand::rng().fill_bytes(&mut verifier_bytes);
        let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
        let challenge = pkce_challenge(&verifier);
        let mut state_bytes = [0u8; 16];
        rand::rng().fill_bytes(&mut state_bytes);
        let state = URL_SAFE_NO_PAD.encode(state_bytes);
        self.states
            .store(&state, &provider_name.trim().to_lowercase())?;
        let params = [
            ("access_type", "offline"),
            ("client_id", client_id),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("prompt", "consent"),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", config.scopes.as_str()),
            ("state", state.as_str()),
        ];
        let encoded = encode_form(&params);
        Ok((format!("{}?{encoded}", config.auth_url), verifier))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn exchange_code(
        &self,
        transport: &dyn TokenTransport,
        provider_name: &str,
        code: &str,
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>, OAuthError> {
        let config = provider(provider_name)
            .ok_or_else(|| OAuthError::oauth2(format!("unknown provider {provider_name:?}")))?;
        let mut params = vec![
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ];
        if !verifier.is_empty() {
            params.push(("code_verifier", verifier));
        }
        self.token_request(transport, &config.token_url, &params, "token exchange")
    }

    pub fn refresh_access_token(
        &self,
        transport: &dyn TokenTransport,
        provider_name: &str,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>, OAuthError> {
        let config = provider(provider_name)
            .ok_or_else(|| OAuthError::oauth2(format!("unknown provider {provider_name:?}")))?;
        let params = [
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "refresh_token"),
        ];
        self.token_request(transport, &config.token_url, &params, "token refresh")
    }

    fn token_request(
        &self,
        transport: &dyn TokenTransport,
        endpoint: &str,
        fields: &[(&str, &str)],
        operation: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>, OAuthError> {
        let form = encode_form(fields);
        let reply = transport
            .post_form(endpoint, &form, self.timeout)
            .map_err(|_| OAuthError::oauth2(format!("{operation} failed")))?;
        if !(200..300).contains(&reply.status) {
            return Err(OAuthError::oauth2(format!(
                "{operation} returned HTTP {}",
                reply.status
            )));
        }
        match serde_json::from_str::<serde_json::Value>(&reply.body) {
            Ok(serde_json::Value::Object(map)) => Ok(map),
            Ok(_) | Err(_) => Err(OAuthError::oauth2(format!(
                "{operation} returned invalid JSON"
            ))),
        }
    }
}

/// S256 challenge: base64url-without-padding of the SHA-256 of the verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// `url.Values.Encode()`: parameters sorted by key, values in insertion order,
/// spaces as `+` and everything outside the unreserved set percent-encoded.
pub fn encode_form(fields: &[(&str, &str)]) -> String {
    let mut sorted: Vec<(&str, &str)> = fields.to_vec();
    sorted.sort_by(|left, right| left.0.cmp(right.0));
    sorted
        .into_iter()
        .map(|(key, value)| format!("{}={}", query_escape(key), query_escape(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn query_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn current_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn create_private_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)
    }
}

fn set_private_file_mode(file: &std::fs::File) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(())
    }
}

fn set_private_file_mode_path(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
