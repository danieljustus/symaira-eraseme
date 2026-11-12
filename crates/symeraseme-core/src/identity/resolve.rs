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
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
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

/// Upper bound for the output readers to see EOF once the provider's process
/// tree has been killed. Only a descendant that escaped the owned process
/// group (for example via `setsid`) can still hold a pipe open this long.
const READER_GRACE: Duration = Duration::from_secs(1);

/// Kills the provider and every descendant that is still in its process group.
///
/// On Unix the provider is spawned as the leader of a fresh process group, so a
/// group-wide `SIGKILL` also reaches background children that inherited the
/// output pipes. Elsewhere only the direct child can be terminated.
fn kill_process_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        let _ =
            nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), nix::sys::signal::SIGKILL);
    }
    let _ = child.kill();
}

fn run_secret_command(
    mut command: Command,
    timeout: Duration,
    failure: SecretBackendError,
    timeout_error: SecretBackendError,
) -> Result<String, SecretBackendError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().map_err(|_| failure)?;
    let Some(stdout) = child.stdout.take() else {
        kill_process_tree(&mut child);
        let _ = child.wait();
        return Err(failure);
    };
    let Some(stderr) = child.stderr.take() else {
        kill_process_tree(&mut child);
        let _ = child.wait();
        return Err(failure);
    };

    let (stdout_tx, stdout_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stdout = stdout;
        let mut output = Vec::new();
        let _ = stdout_tx.send(stdout.read_to_end(&mut output).map(|_| output));
    });
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stderr = stderr;
        let _ = stderr_tx.send(io::copy(&mut stderr, &mut io::sink()));
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                // The group is killed before the leader is reaped so its id
                // cannot be recycled in between. The readers end on their own
                // once the last pipe holder is gone; they are not joined.
                kill_process_tree(&mut child);
                let _ = child.wait();
                return Err(timeout_error);
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                kill_process_tree(&mut child);
                let _ = child.wait();
                return Err(failure);
            }
        }
    };

    // The leader is gone, but a background descendant may still own the pipes.
    // Clean it up before waiting for EOF so the reads below are bounded.
    kill_process_tree(&mut child);
    if !status.success() {
        return Err(failure);
    }
    let Ok(Ok(stdout)) = stdout_rx.recv_timeout(READER_GRACE) else {
        return Err(timeout_error);
    };
    let _ = stderr_rx.recv_timeout(READER_GRACE);
    Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// Unique marker a failing provider prints; it must never reach an error.
    const MARKER: &str = "RUNNER-FAILURE-MARKER-7f3a9c";
    const FAILURE: SecretBackendError = SecretBackendError::KeychainUnavailable;
    const TIMEOUT: SecretBackendError = SecretBackendError::SymvaultTimedOut;

    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    fn run(script: &str, timeout: Duration) -> Result<String, SecretBackendError> {
        run_secret_command(sh(script), timeout, FAILURE, TIMEOUT)
    }

    fn assert_error_is_redacted<E: fmt::Display + fmt::Debug>(error: &E) {
        let display = error.to_string();
        let debug = format!("{error:?}");
        assert!(
            !display.contains(MARKER),
            "display leaked marker: {display}"
        );
        assert!(!debug.contains(MARKER), "debug leaked marker: {debug}");
    }

    fn dir_entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("read fixture dir")
            .map(|entry| {
                entry
                    .expect("dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into()
            })
            .collect();
        names.sort();
        names
    }

    /// Backend that routes lookups through the real runner with fake `sh`
    /// providers; it never touches a real keychain, keyring or vault.
    struct ScriptBackend {
        script: &'static str,
        timeout: Duration,
    }

    impl SecretBackend for ScriptBackend {
        fn resolve_keychain(&self, _: &str, _: &str) -> Result<String, SecretBackendError> {
            run_secret_command(sh(self.script), self.timeout, FAILURE, FAILURE)
        }

        fn resolve_symvault(&self, _: &str) -> Result<String, SecretBackendError> {
            run_secret_command(
                sh(self.script),
                self.timeout,
                SecretBackendError::SymvaultUnavailable,
                TIMEOUT,
            )
        }

        fn get_keyring(&self, _: &str, _: &str) -> Result<Option<String>, SecretBackendError> {
            Ok(None)
        }
    }

    #[test]
    fn success_trims_surrounding_whitespace() {
        let secret = run(
            "printf '  \\n\\t s3cret value \\n\\n'",
            Duration::from_secs(5),
        );
        assert_eq!(secret, Ok("s3cret value".to_owned()));
    }

    #[test]
    fn success_ignores_stderr_and_allows_empty_output() {
        let secret = run(
            &format!("echo {MARKER} >&2; printf ok"),
            Duration::from_secs(5),
        );
        assert_eq!(secret, Ok("ok".to_owned()));
        assert_eq!(run("exit 0", Duration::from_secs(5)), Ok(String::new()));
    }

    #[test]
    fn non_zero_exit_returns_failure_category_without_output() {
        let script = format!("echo {MARKER}; echo {MARKER} >&2; exit 3");
        let error = run(&script, Duration::from_secs(5)).expect_err("non-zero exit must fail");
        assert_eq!(error, FAILURE);
        assert_error_is_redacted(&error);
    }

    #[test]
    fn missing_executable_returns_failure_category() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join(format!("no-such-{MARKER}"));
        let error = run_secret_command(
            Command::new(&missing),
            Duration::from_secs(5),
            FAILURE,
            TIMEOUT,
        )
        .expect_err("spawn failure must fail");
        assert_eq!(error, FAILURE);
        assert_error_is_redacted(&error);
        assert!(dir_entries(dir.path()).is_empty());
    }

    #[test]
    fn timeout_returns_timeout_category_and_reaps_child() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_file = dir.path().join("pid");
        // `exec` keeps the recorded pid equal to the killed process so no
        // grandchild can keep the output pipes open.
        let script = format!(
            "echo $$ > '{}'; echo {MARKER}; echo {MARKER} >&2; exec sleep 30",
            pid_file.display()
        );

        let started = Instant::now();
        let error = run(&script, Duration::from_millis(750)).expect_err("timeout must fail");
        let elapsed = started.elapsed();

        assert_eq!(error, TIMEOUT);
        assert_ne!(error, FAILURE, "timeout must not use the failure category");
        assert_error_is_redacted(&error);
        assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");

        let pid = fs::read_to_string(&pid_file).expect("child recorded its pid");
        let alive = sh(&format!("kill -0 {}", pid.trim()))
            .stderr(Stdio::null())
            .status()
            .expect("probe kill -0")
            .success();
        assert!(!alive, "timed-out child {} is still running", pid.trim());
        assert_eq!(dir_entries(dir.path()), ["pid"], "unexpected residue");
    }

    /// Reads the pid a provider script recorded, waiting for the atomic rename
    /// that publishes it.
    fn recorded_pid(path: &Path) -> Option<i32> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Ok(text) = fs::read_to_string(path) {
                return text.trim().parse().ok();
            }
            thread::sleep(Duration::from_millis(10));
        }
        None
    }

    fn process_exists(pid: i32) -> bool {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
    }

    fn wait_until_dead(pid: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if !process_exists(pid) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// Runs a provider whose non-exec shell starts a background `sleep` that
    /// inherits stdout/stderr and publishes its pid in `bg`. The runner runs on
    /// a helper thread under an outer deadline so a regression fails instead of
    /// hanging; a leaked descendant is killed by its recorded pid.
    fn run_with_background_sleep(
        body: &str,
        timeout: Duration,
    ) -> (Result<String, SecretBackendError>, Duration, i32) {
        let dir = tempfile::tempdir().expect("tempdir");
        let bg = dir.path().join("bg");
        let tmp = dir.path().join("bg.tmp");
        let script = format!(
            "sleep 30 & echo $! > '{tmp}' && mv '{tmp}' '{bg}'; {body}",
            tmp = tmp.display(),
            bg = bg.display(),
        );
        let (tx, rx) = mpsc::channel();
        let started = Instant::now();
        thread::spawn(move || {
            let _ = tx.send(run(&script, timeout));
        });
        let outcome = rx.recv_timeout(Duration::from_secs(8));
        let elapsed = started.elapsed();
        let pid = recorded_pid(&bg).expect("background sleep published its pid");
        let Ok(result) = outcome else {
            let _ =
                nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), nix::sys::signal::SIGKILL);
            panic!("run_secret_command did not return within 8s: descendant holds the pipes");
        };
        (result, elapsed, pid)
    }

    #[test]
    fn timeout_kills_background_descendant_holding_the_pipes() {
        let body = format!("echo {MARKER}; echo {MARKER} >&2; wait");
        let (result, elapsed, pid) = run_with_background_sleep(&body, Duration::from_millis(750));
        let error = result.expect_err("timeout must fail");
        assert_eq!(error, TIMEOUT);
        assert_error_is_redacted(&error);
        assert!(elapsed < Duration::from_secs(3), "took {elapsed:?}");
        assert!(
            wait_until_dead(pid),
            "descendant {pid} survived the timeout"
        );
    }

    #[test]
    fn leader_exit_does_not_wait_for_background_descendant_holding_the_pipes() {
        let body = format!("echo s3cret; echo {MARKER} >&2; exit 0");
        let (result, elapsed, pid) = run_with_background_sleep(&body, Duration::from_secs(5));
        assert_eq!(result, Ok("s3cret".to_owned()));
        assert!(elapsed < Duration::from_secs(3), "took {elapsed:?}");
        assert!(wait_until_dead(pid), "descendant {pid} survived the leader");
    }

    #[test]
    fn failing_leader_with_background_descendant_returns_failure_without_hanging() {
        let body = format!("echo {MARKER}; echo {MARKER} >&2; exit 4");
        let (result, elapsed, pid) = run_with_background_sleep(&body, Duration::from_secs(5));
        let error = result.expect_err("non-zero exit must fail");
        assert_eq!(error, FAILURE);
        assert_error_is_redacted(&error);
        assert!(elapsed < Duration::from_secs(3), "took {elapsed:?}");
        assert!(wait_until_dead(pid), "descendant {pid} survived the leader");
    }

    #[test]
    fn resolver_returns_err_for_failing_keychain_provider_without_leak() {
        let backend = ScriptBackend {
            script: "echo RUNNER-FAILURE-MARKER-7f3a9c; echo RUNNER-FAILURE-MARKER-7f3a9c >&2; exit 1",
            timeout: Duration::from_secs(5),
        };
        let resolver = SecretResolver::new(backend);
        let error = resolver
            .resolve("keychain://service/account")
            .expect_err("failing provider must be an error");
        assert_eq!(error, SecretResolutionError::Backend(FAILURE));
        assert_error_is_redacted(&error);
        assert!(!format!("{resolver:?}").contains(MARKER));
    }

    #[test]
    fn resolver_returns_err_for_timed_out_and_failing_symvault_without_leak() {
        let timed_out = ScriptBackend {
            script: "echo RUNNER-FAILURE-MARKER-7f3a9c; echo RUNNER-FAILURE-MARKER-7f3a9c >&2; exec sleep 30",
            timeout: Duration::from_millis(500),
        };
        let failing = ScriptBackend {
            script: "echo RUNNER-FAILURE-MARKER-7f3a9c; exit 2",
            timeout: Duration::from_secs(5),
        };
        for backend in [timed_out, failing] {
            let resolver = SecretResolver::new(backend).with_env_fallback("UNSET_FALLBACK");
            let error = resolver
                .resolve("symvault://part/key")
                .expect_err("failed vault lookup must exhaust fallbacks");
            assert!(matches!(
                error,
                SecretResolutionError::VaultFallbackExhausted { .. }
            ));
            assert_error_is_redacted(&error);
        }
    }

    #[test]
    fn os_backend_rejects_unsafe_symvault_paths_without_spawning() {
        let backend = OsSecretBackend;
        for path in ["", "-flag", "a\0b", "a\nb"] {
            assert_eq!(
                backend.resolve_symvault(path),
                Err(SecretBackendError::SymvaultUnavailable),
                "path {path:?}"
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn os_backend_keychain_is_unavailable_off_macos() {
        assert_eq!(
            OsSecretBackend.resolve_keychain("service", "account"),
            Err(SecretBackendError::KeychainUnavailable)
        );
    }
}
