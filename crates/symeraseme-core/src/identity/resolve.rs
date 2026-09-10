//! Application secret-reference resolution compatible with the Go identity
//! package.
//!
//! Resolution is deliberately kept at the Rust-facing boundary: literals pass
//! through, supported references use the injected backend, and vault
//! references retain EraseMe's environment/keyring fallback chain. Backend
//! failures are represented by opaque categories so provider output can never
//! become an error or debug string.

use super::keyring::{KeyringBackend, OsKeyring};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Canonical Symaira Vault reference prefix.
pub const SYMVAULT_PREFIX: &str = "symvault://";
/// Legacy vault reference prefix retained for compatibility.
pub const VAULT_PREFIX: &str = "vault://";
/// Environment reference prefix.
pub const ENV_PREFIX: &str = "env://";
/// Keychain reference prefix.
pub const KEYCHAIN_PREFIX: &str = "keychain://";

const SYMVAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Safe categories for failures at secret-provider boundaries.
///
/// This enum intentionally has no source error or command output field. The
/// provider may have returned a credential in stdout/stderr, so retaining
/// those diagnostics would make a later `Display` or `Debug` call unsafe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretBackendError {
    /// A keychain reference could not be read.
    KeychainUnavailable,
    /// An EraseMe fallback keyring entry could not be read.
    KeyringUnavailable,
    /// The `symvault` command failed or returned unusable output.
    SymvaultUnavailable,
    /// The `symvault` command exceeded its bounded timeout.
    SymvaultTimedOut,
}

impl fmt::Display for SecretBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::KeychainUnavailable => "keychain lookup failed",
            Self::KeyringUnavailable => "keyring lookup failed",
            Self::SymvaultUnavailable => "symvault lookup failed",
            Self::SymvaultTimedOut => "symvault lookup timed out",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SecretBackendError {}

/// Secret-provider operations required by [`SecretResolver`].
///
/// Implementations must return only [`SecretBackendError`] categories on
/// failure. This makes it impossible for a provider's raw error text or
/// captured output to leak through the resolver's error paths.
pub trait SecretBackend: Send + Sync {
    /// Resolve a `keychain://service/account` reference.
    fn resolve_keychain(&self, service: &str, account: &str) -> Result<String, SecretBackendError>;

    /// Resolve a `symvault://path` reference.
    fn resolve_symvault(&self, path: &str) -> Result<String, SecretBackendError>;

    /// Read the final EraseMe-specific keyring fallback.
    fn get_keyring(
        &self,
        service: &str,
        username: &str,
    ) -> Result<Option<String>, SecretBackendError>;
}

/// Production keychain, keyring and `symvault` adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsSecretBackend;

impl SecretBackend for OsSecretBackend {
    fn resolve_keychain(&self, service: &str, account: &str) -> Result<String, SecretBackendError> {
        #[cfg(target_os = "macos")]
        {
            let mut command = Command::new("security");
            command.args(["find-generic-password", "-w", "-s", service, "-a", account]);
            run_secret_command(
                command,
                SYMVAULT_TIMEOUT,
                SecretBackendError::KeychainUnavailable,
                SecretBackendError::KeychainUnavailable,
            )
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (service, account);
            Err(SecretBackendError::KeychainUnavailable)
        }
    }

    fn resolve_symvault(&self, path: &str) -> Result<String, SecretBackendError> {
        if !valid_symvault_path(path) {
            return Err(SecretBackendError::SymvaultUnavailable);
        }

        let mut command = Command::new("symvault");
        command.args(["get", "--", path, "--print"]);
        run_secret_command(
            command,
            SYMVAULT_TIMEOUT,
            SecretBackendError::SymvaultUnavailable,
            SecretBackendError::SymvaultTimedOut,
        )
    }

    fn get_keyring(
        &self,
        service: &str,
        username: &str,
    ) -> Result<Option<String>, SecretBackendError> {
        OsKeyring
            .get(service, username)
            .map_err(|_| SecretBackendError::KeyringUnavailable)
    }
}

/// Errors returned by application secret resolution.
///
/// No variant stores a resolved value, provider stderr/stdout, or an
/// arbitrary provider error. The stored names are configuration identifiers,
/// never credential contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SecretResolutionError {
    /// An `env://` reference named an unset or empty variable.
    EnvironmentUnavailable { name: String },
    /// A `keychain://` reference did not contain both service and account.
    InvalidKeychainReference,
    /// A direct provider lookup failed.
    Backend(SecretBackendError),
    /// A vault URI had no path.
    EmptyVaultReference { prefix: &'static str },
    /// A vault lookup and all configured EraseMe fallbacks failed.
    VaultFallbackExhausted {
        env_fallback: Option<String>,
        keyring_service: Option<String>,
    },
}

impl fmt::Display for SecretResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnvironmentUnavailable { name } => write!(
                formatter,
                "identity: cannot resolve secret: environment variable {name} is not set"
            ),
            Self::InvalidKeychainReference => formatter.write_str(
                "identity: cannot resolve secret: invalid keychain reference, expected keychain://service/account",
            ),
            Self::Backend(error) => {
                write!(formatter, "identity: cannot resolve secret: {error}")
            }
            Self::EmptyVaultReference { prefix } => write!(
                formatter,
                "identity: cannot resolve secret: empty {prefix} URI (provide a path like {SYMVAULT_PREFIX}part/key)"
            ),
            Self::VaultFallbackExhausted {
                env_fallback,
                keyring_service,
            } => {
                let mut message = String::from(
                    "identity: cannot resolve secret: shared secret reference could not be resolved",
                );
                if let Some(name) = env_fallback {
                    use fmt::Write as _;
                    write!(message, ", env var {name:?} not set").expect("String write");
                }
                if let Some(service) = keyring_service {
                    use fmt::Write as _;
                    write!(message, ", keyring {service:?} has no entry").expect("String write");
                }
                message.push_str(
                    ". Set the value directly or configure a supported secret reference.",
                );
                formatter.write_str(&message)
            }
        }
    }
}

impl std::error::Error for SecretResolutionError {}

/// EraseMe application-secret resolver.
///
/// `new` and `with_environment` are hermetic constructors for tests and
/// embedding callers. [`Self::from_process`] is the thin process adapter and
/// snapshots the process environment without ever formatting its values.
pub struct SecretResolver<B = OsSecretBackend> {
    backend: B,
    environment: BTreeMap<String, String>,
    env_fallback: Option<String>,
    keyring_service: Option<String>,
    keyring_username: Option<String>,
}

impl SecretResolver<OsSecretBackend> {
    /// Build a resolver from the current process environment.
    pub fn from_process() -> Self {
        Self::with_environment(OsSecretBackend, std::env::vars().collect())
    }
}

impl Default for SecretResolver<OsSecretBackend> {
    fn default() -> Self {
        Self::from_process()
    }
}

impl<B: SecretBackend> SecretResolver<B> {
    /// Build a resolver with no ambient environment values.
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            environment: BTreeMap::new(),
            env_fallback: None,
            keyring_service: None,
            keyring_username: None,
        }
    }

    /// Build a resolver with an explicit environment snapshot.
    pub fn with_environment(backend: B, environment: BTreeMap<String, String>) -> Self {
        Self {
            backend,
            environment,
            env_fallback: None,
            keyring_service: None,
            keyring_username: None,
        }
    }

    /// Set the environment variable used after a vault lookup fails.
    pub fn set_env_fallback(&mut self, name: impl Into<String>) {
        self.env_fallback = Some(name.into());
    }

    /// Set the keyring service used after a vault lookup and env fallback fail.
    pub fn set_keyring_service(&mut self, service: impl Into<String>) {
        self.keyring_service = Some(service.into());
    }

    /// Set the optional explicit keyring username for vault fallback.
    pub fn set_keyring_username(&mut self, username: impl Into<String>) {
        self.keyring_username = Some(username.into());
    }

    /// Builder form of [`Self::set_env_fallback`].
    pub fn with_env_fallback(mut self, name: impl Into<String>) -> Self {
        self.set_env_fallback(name);
        self
    }

    /// Builder form of [`Self::set_keyring_service`].
    pub fn with_keyring_service(mut self, service: impl Into<String>) -> Self {
        self.set_keyring_service(service);
        self
    }

    /// Builder form of [`Self::set_keyring_username`].
    pub fn with_keyring_username(mut self, username: impl Into<String>) -> Self {
        self.set_keyring_username(username);
        self
    }

    /// Resolve a literal or supported secret reference.
    pub fn resolve(&self, value: &str) -> Result<String, SecretResolutionError> {
        if let Some(name) = value.strip_prefix(ENV_PREFIX) {
            return self
                .environment
                .get(name)
                .filter(|value| !value.is_empty())
                .cloned()
                .ok_or_else(|| SecretResolutionError::EnvironmentUnavailable {
                    name: name.to_owned(),
                });
        }

        if let Some(reference) = value.strip_prefix(KEYCHAIN_PREFIX) {
            let Some((service, account)) = reference.split_once('/') else {
                return Err(SecretResolutionError::InvalidKeychainReference);
            };
            if service.is_empty() || account.is_empty() {
                return Err(SecretResolutionError::InvalidKeychainReference);
            }
            return self
                .backend
                .resolve_keychain(service, account)
                .map_err(SecretResolutionError::Backend);
        }

        let Some((prefix, path)) = vault_reference(value) else {
            return Ok(value.to_owned());
        };
        if path.is_empty() {
            return Err(SecretResolutionError::EmptyVaultReference { prefix });
        }

        // The Go shared resolver rejects invalid paths before spawning. Its
        // EraseMe wrapper then continues to the configured fallbacks, so a
        // malformed vault path is deliberately treated as a failed lookup.
        if valid_symvault_path(path)
            && let Ok(secret) = self.backend.resolve_symvault(path)
            && !secret.is_empty()
        {
            return Ok(secret);
        }

        if let Some(name) = self.env_fallback.as_deref().filter(|name| !name.is_empty())
            && let Some(secret) = self
                .environment
                .get(name)
                .filter(|value| !value.is_empty() && !is_secret_reference(value))
        {
            return Ok(secret.clone());
        }

        if let Some(service) = self
            .keyring_service
            .as_deref()
            .filter(|name| !name.is_empty())
        {
            let username = self
                .keyring_username
                .as_deref()
                .filter(|name| !name.is_empty())
                .unwrap_or(path);
            if let Ok(Some(secret)) = self.backend.get_keyring(service, username)
                && !secret.is_empty()
            {
                return Ok(secret);
            }
        }

        Err(SecretResolutionError::VaultFallbackExhausted {
            env_fallback: self
                .env_fallback
                .as_deref()
                .filter(|name| !name.is_empty())
                .map(str::to_owned),
            keyring_service: self
                .keyring_service
                .as_deref()
                .filter(|name| !name.is_empty())
                .map(str::to_owned),
        })
    }
}

impl<B> fmt::Debug for SecretResolver<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretResolver")
            .field(
                "environment_keys",
                &self.environment.keys().collect::<Vec<_>>(),
            )
            .field("has_env_fallback", &self.env_fallback.is_some())
            .field("has_keyring_service", &self.keyring_service.is_some())
            .field("has_keyring_username", &self.keyring_username.is_some())
            .finish_non_exhaustive()
    }
}

fn vault_reference(value: &str) -> Option<(&'static str, &str)> {
    if let Some(path) = value.strip_prefix(SYMVAULT_PREFIX) {
        return Some((SYMVAULT_PREFIX, path));
    }
    value
        .strip_prefix(VAULT_PREFIX)
        .map(|path| (VAULT_PREFIX, path))
}

fn is_secret_reference(value: &str) -> bool {
    value.starts_with(SYMVAULT_PREFIX)
        || value.starts_with(VAULT_PREFIX)
        || value.starts_with(ENV_PREFIX)
        || value.starts_with(KEYCHAIN_PREFIX)
}

fn valid_symvault_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('-')
        && !path.contains('\0')
        && !path.chars().any(char::is_control)
}

fn run_secret_command(
    mut command: Command,
    timeout: Duration,
    failure: SecretBackendError,
    timeout_error: SecretBackendError,
) -> Result<String, SecretBackendError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| failure)?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(failure);
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(failure);
    };

    let stdout_reader = thread::spawn(move || {
        let mut stdout = stdout;
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).map(|_| output)
    });
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut sink = io::sink();
        io::copy(&mut stderr, &mut sink)
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(timeout_error);
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(failure);
            }
        }
    };

    let stdout = stdout_reader.join().ok().and_then(Result::ok);
    let _ = stderr_reader.join();
    if !status.success() {
        return Err(failure);
    }
    let Some(stdout) = stdout else {
        return Err(failure);
    };
    Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
}
