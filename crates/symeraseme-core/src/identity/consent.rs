//! File-backed destructive-operation consent tokens.
//!
//! The format is shared with the Go implementation: a token is stored in
//! `consent_<sha256-prefix>.json`, with a JSON body containing the command,
//! Unix timestamps, and the URL-safe token value.  The store keeps the clock
//! and token source injectable so differential fixtures can be deterministic.

use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::Builder;

/// Default token lifetime, matching the Go/Python implementations.
pub const DEFAULT_TOKEN_TTL: i64 = 86_400;
/// Permission for the consent directory on Unix-like systems.
pub const CONSENT_DIR_MODE: u32 = 0o700;
/// Permission for consent files on Unix-like systems.
pub const CONSENT_FILE_MODE: u32 = 0o600;

const TOKEN_BYTES: usize = 16;

/// Errors returned by consent operations.
#[derive(Debug)]
pub enum ConsentError {
    Io(io::Error),
    Json(serde_json::Error),
    NotFound,
    Expired,
    CommandMismatch,
    Denied,
}

impl PartialEq for ConsentError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::NotFound, Self::NotFound)
                | (Self::Expired, Self::Expired)
                | (Self::CommandMismatch, Self::CommandMismatch)
                | (Self::Denied, Self::Denied)
                | (Self::Io(_), Self::Io(_))
                | (Self::Json(_), Self::Json(_))
        )
    }
}

impl Eq for ConsentError {}

impl fmt::Display for ConsentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("identity: consent storage error"),
            Self::Json(_) | Self::NotFound => {
                formatter.write_str("identity: consent token not found")
            }
            Self::Expired => formatter.write_str("identity: consent token expired"),
            Self::CommandMismatch => {
                formatter.write_str("identity: consent token command mismatch")
            }
            Self::Denied => formatter.write_str("identity: consent denied"),
        }
    }
}

impl std::error::Error for ConsentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ConsentError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ConsentError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// On-disk representation. `token` is omitted only for legacy v1 files.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsentRecord {
    pub command: String,
    pub issued_at: i64,
    pub expires_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Public summary returned by [`ConsentStore::list_tokens`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConsentToken {
    pub token: String,
    pub command: String,
    pub issued_at: i64,
    pub expires_at: i64,
}

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
type RandomSource = Arc<dyn Fn(usize) -> Result<Vec<u8>, ConsentError> + Send + Sync>;

/// A consent store rooted at one directory.
#[derive(Clone)]
pub struct ConsentStore {
    directory: PathBuf,
    clock: Clock,
    random: RandomSource,
}

impl fmt::Debug for ConsentStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConsentStore")
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

impl ConsentStore {
    /// Create a store rooted at `directory`.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            clock: Arc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_secs() as i64)
                    .unwrap_or_default()
            }),
            random: Arc::new(|length| {
                let mut bytes = vec![0_u8; length];
                rand::rng().fill_bytes(&mut bytes);
                Ok(bytes)
            }),
        }
    }

    /// Construct a store using the platform/configured data directory.
    pub fn from_default_directory() -> io::Result<Self> {
        Ok(Self::new(default_consent_directory()?))
    }

    /// Replace the clock for deterministic fixtures.
    pub fn with_clock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Replace the random source for deterministic fixtures.
    pub fn with_random_source(
        mut self,
        random: impl Fn(usize) -> Result<Vec<u8>, ConsentError> + Send + Sync + 'static,
    ) -> Self {
        self.random = Arc::new(random);
        self
    }

    /// Issue and atomically persist a token for `command`.
    pub fn issue_token(&self, command: &str, ttl: i64) -> Result<String, ConsentError> {
        let ttl = if ttl <= 0 { DEFAULT_TOKEN_TTL } else { ttl };
        self.ensure_directory()?;
        let bytes = (self.random)(TOKEN_BYTES)?;
        if bytes.len() != TOKEN_BYTES {
            return Err(ConsentError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "consent token source returned an invalid length",
            )));
        }
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let now = (self.clock)();
        let record = ConsentRecord {
            command: command.to_owned(),
            issued_at: now,
            expires_at: now.saturating_add(ttl),
            token: Some(token.clone()),
        };
        let body = serde_json::to_vec(&record)?;
        atomic_write(&self.path_for_token(&token), &body)?;
        Ok(token)
    }

    /// Verify a token for a command. Expired files are removed.
    pub fn verify_token(&self, command: &str, token: &str) -> Result<(), ConsentError> {
        if token.is_empty() {
            return Err(ConsentError::NotFound);
        }
        self.ensure_directory()?;
        let path = self.find_token_file(token)?;
        let body = fs::read(&path).map_err(|_| ConsentError::NotFound)?;
        let record: ConsentRecord =
            serde_json::from_slice(&body).map_err(|_| ConsentError::NotFound)?;
        if let Some(stored) = &record.token
            && stored != token
        {
            return Err(ConsentError::NotFound);
        }
        if record.command != command {
            return Err(ConsentError::CommandMismatch);
        }
        if (self.clock)() > record.expires_at {
            let _ = fs::remove_file(path);
            return Err(ConsentError::Expired);
        }
        // Go hardens existing validated records on a best-effort basis.
        let _ = tighten_permissions(&path);
        Ok(())
    }

    /// Remove a token after successful use. Missing tokens are ignored.
    pub fn consume_token(&self, token: &str) -> Result<(), ConsentError> {
        if token.is_empty() {
            return Ok(());
        }
        self.ensure_directory()?;
        match self.find_token_file(token) {
            Ok(path) => match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            },
            Err(ConsentError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Revoke a token, returning whether a file was removed.
    pub fn revoke_token(&self, token: &str) -> Result<bool, ConsentError> {
        if token.is_empty() {
            return Ok(false);
        }
        self.ensure_directory()?;
        let path = match self.find_token_file(token) {
            Ok(path) => path,
            Err(ConsentError::NotFound) => return Ok(false),
            Err(error) => return Err(error),
        };
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// List valid tokens in issued-at order and prune expired records.
    pub fn list_tokens(&self) -> Result<Vec<ConsentToken>, ConsentError> {
        self.ensure_directory()?;
        let now = (self.clock)();
        let mut tokens = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("consent_") || !name.ends_with(".json") {
                continue;
            }
            let path = entry.path();
            let body = match fs::read(&path) {
                Ok(body) => body,
                Err(_) => continue,
            };
            let record: ConsentRecord = match serde_json::from_slice(&body) {
                Ok(record) => record,
                Err(_) => continue,
            };
            if now > record.expires_at {
                let _ = fs::remove_file(path);
                continue;
            }
            let _ = tighten_permissions(&path);
            let token = record.token.unwrap_or_else(|| {
                name.trim_start_matches("consent_")
                    .trim_end_matches(".json")
                    .to_owned()
            });
            tokens.push(ConsentToken {
                token,
                command: record.command,
                issued_at: record.issued_at,
                expires_at: record.expires_at,
            });
        }
        tokens.sort_by_key(|token| token.issued_at);
        Ok(tokens)
    }

    fn ensure_directory(&self) -> io::Result<()> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(CONSENT_DIR_MODE);
        }
        builder.create(&self.directory).map_err(|error| {
            // Go MkdirAll reports ENOTDIR for an existing file at the leaf;
            // Rust's recursive builder reports EEXIST on Unix instead.
            if error.kind() == io::ErrorKind::AlreadyExists
                && fs::metadata(&self.directory).is_ok_and(|metadata| !metadata.is_dir())
            {
                io::Error::from(io::ErrorKind::NotADirectory)
            } else {
                error
            }
        })?;
        // MkdirAll uses 0700 for every new ancestor; only the requested
        // directory is subsequently hardened, ignoring chmod errors in Go.
        let _ = tighten_permissions(&self.directory);
        Ok(())
    }

    fn path_for_token(&self, token: &str) -> PathBuf {
        self.directory.join(token_filename(token))
    }

    fn find_token_file(&self, token: &str) -> Result<PathBuf, ConsentError> {
        let hashed = self.path_for_token(token);
        if hashed.is_file() {
            return Ok(hashed);
        }
        let legacy = self.directory.join(format!("consent_{token}.json"));
        if legacy.parent() != Some(self.directory.as_path()) || !legacy.is_file() {
            return Err(ConsentError::NotFound);
        }
        let destination = self.path_for_token(token);
        match fs::rename(&legacy, &destination) {
            Ok(()) => Ok(destination),
            Err(_) if destination.is_file() => Ok(destination),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(ConsentError::NotFound),
            Err(error) => Err(error.into()),
        }
    }
}

/// Return the configured default data directory used for consent files.
pub fn default_consent_directory() -> io::Result<PathBuf> {
    if let Ok(value) = std::env::var("SYMERASEME_DATA_DIR")
        && !value.is_empty()
    {
        return Ok(expand_home(value));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory unavailable"))?;
    Ok(home.join(".local/share/symeraseme"))
}

fn expand_home(value: String) -> PathBuf {
    if value == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
    }
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(value)
}

fn token_filename(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let hex = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("consent_{hex}.json")
}

fn atomic_write(path: &Path, body: &[u8]) -> io::Result<()> {
    atomic_write_with(path, body, fs::File::sync_all, close_file, chmod_temporary)
}

// Operation-local seams keep failure tests deterministic without global hooks.
fn atomic_write_with(
    path: &Path,
    body: &[u8],
    sync: impl FnOnce(&fs::File) -> io::Result<()>,
    close: impl FnOnce(fs::File) -> io::Result<()>,
    chmod: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "consent path has no parent"))?;
    let mut temporary = Builder::new()
        .prefix(".consent-")
        .suffix(".tmp")
        .tempfile_in(directory)?;
    temporary.write_all(body)?;
    // Retain the existing fail-closed Rust guard. Go has no sync call:
    // this is an explicit ID-005 difference, not normalized oracle parity.
    sync(temporary.as_file())?;
    // Go closes before chmod/rename.
    // Keep the path guard alive so every pre-rename failure removes our temp.
    let (file, temporary) = temporary.into_parts();
    close(file)?;
    chmod(&temporary)?;
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|error| error.error)
}

fn close_file(file: fs::File) -> io::Result<()> {
    #[cfg(unix)]
    {
        // Consume the owner exactly once. Do not retry a possibly closed fd
        // or turn EINTR into success; both could conceal a close failure.
        nix::unistd::close(file).map_err(io::Error::from)
    }
    #[cfg(not(unix))]
    {
        // A reviewed checked-close API for these targets remains an ID-005
        // blocker. Preserve the existing drop behavior and the sync guard.
        drop(file);
        Ok(())
    }
}

fn chmod_temporary(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The temporary entry is a file. Apply the required mode directly,
        // matching Go's checked Chmod without an extra metadata failure path.
        fs::set_permissions(path, fs::Permissions::from_mode(CONSENT_FILE_MODE))
    }
    #[cfg(not(unix))]
    tighten_permissions(path)
}

fn tighten_permissions(path: &Path) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = metadata.permissions();
        let mode = if metadata.is_dir() {
            CONSENT_DIR_MODE
        } else {
            CONSENT_FILE_MODE
        };
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(windows)]
    {
        // Go's os.Chmod on Windows maps the owner-write bit to read-only.
        // POSIX modes/ACL equivalence require separate native evidence.
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "consent_filesystem_tests.rs"]
mod filesystem_tests;

#[cfg(test)]
#[path = "consent_portable_tests.rs"]
mod portable_filesystem_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn store(directory: &Path) -> ConsentStore {
        ConsentStore::new(directory)
            .with_clock(|| 1_000_000)
            .with_random_source(|length| Ok((0..length).map(|value| value as u8).collect()))
    }

    #[test]
    fn issue_has_deterministic_token_name_and_json() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());
        let token = store.issue_token("send-removal", 3_600).unwrap();
        let expected =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode((0_u8..16).collect::<Vec<_>>());
        assert_eq!(token, expected);
        let path = directory.path().join(token_filename(&token));
        assert!(path.is_file());
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            format!(
                r#"{{"command":"send-removal","issued_at":1000000,"expires_at":1003600,"token":"{expected}"}}"#
            )
        );
    }

    #[test]
    fn verify_command_expiry_and_single_use() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());
        let token = store.issue_token("send-removal", 3_600).unwrap();
        assert_eq!(store.verify_token("send-removal", &token), Ok(()));
        assert_eq!(
            store.verify_token("other", &token),
            Err(ConsentError::CommandMismatch)
        );
        let expired = store.clone().with_clock(|| 1_003_601);
        assert_eq!(
            expired.verify_token("send-removal", &token),
            Err(ConsentError::Expired)
        );
        assert_eq!(store.consume_token(&token), Ok(()));
        assert_eq!(
            store.verify_token("send-removal", &token),
            Err(ConsentError::NotFound)
        );
        assert_eq!(store.consume_token(&token), Ok(()));
    }

    #[test]
    fn legacy_file_is_migrated_and_listed() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConsentStore::new(directory.path()).with_clock(|| 150);
        let token = "legacy-token";
        let path = directory.path().join(format!("consent_{token}.json"));
        fs::write(
            &path,
            r#"{"command":"delete","issued_at":100,"expires_at":200}"#,
        )
        .unwrap();
        assert_eq!(store.verify_token("delete", token), Ok(()));
        assert!(!path.exists());
        assert!(directory.path().join(token_filename(token)).exists());
    }

    #[test]
    fn list_prunes_expired_and_ignores_malformed() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());
        let fresh = store.issue_token("fresh", 100).unwrap();
        let expired = store.clone().with_clock(|| 2_000_001);
        expired.issue_token("expired", 1).unwrap();
        fs::write(directory.path().join("consent_bad.json"), b"{").unwrap();
        let listed = expired.list_tokens().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].token, fresh);
    }

    #[cfg(unix)]
    #[test]
    fn file_permissions_are_restricted_on_unix() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());
        let token = store.issue_token("delete", 60).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let dir_mode = fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777;
        let file_mode = fs::metadata(directory.path().join(token_filename(&token)))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, CONSENT_DIR_MODE);
        assert_eq!(file_mode, CONSENT_FILE_MODE);
    }

    #[allow(dead_code)]
    fn _fixed_clock_is_not_wall_clock() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }
}
