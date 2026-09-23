//! The LLM provider surface EraseMe itself owns.
//!
//! Ports `internal/llm/{llm,factory,util,agent}.go`: the usage record, the error
//! taxonomy, the retry loop's attempt accounting, the provider resolution order
//! and the host-agent descriptor.
//!
//! The transports are **not** ported. In Go, `anthropic`, `openai`, `ollama` and
//! `openai-compatible` all go through `corekit/llmkit`, which owns the wire
//! dialects, the credential reference format and the `auth_failure` error text.
//! `corekit` has no Rust counterpart, so [`create`] reports those providers as
//! not ported instead of pretending an equivalent client exists.

use std::error::Error as StdError;
use std::ffi::OsString;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use std::time::Instant;

/// A single LLM usage and cost record, mirroring Go's `UsageRecord`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageRecord {
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub cost: f64,
}

impl UsageRecord {
    /// Go's `Record()`: the record as a map with the same six keys.
    ///
    /// Go renders an integral `float64` without a decimal point (`0`, not `0.0`),
    /// so a caller comparing JSON text must read `cost` as a number rather than
    /// compare the two encodings character by character.
    pub fn record(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        out.insert("model".into(), serde_json::Value::from(self.model.clone()));
        out.insert(
            "input_tokens".into(),
            serde_json::Value::from(self.input_tokens),
        );
        out.insert(
            "output_tokens".into(),
            serde_json::Value::from(self.output_tokens),
        );
        out.insert(
            "cache_creation_tokens".into(),
            serde_json::Value::from(self.cache_creation_tokens),
        );
        out.insert(
            "cache_read_tokens".into(),
            serde_json::Value::from(self.cache_read_tokens),
        );
        out.insert("cost".into(), serde_json::Value::from(self.cost));
        serde_json::Value::Object(out)
    }
}

/// Go's `*Error`: a provider failure with an optional wrapped cause.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmError {
    message: String,
    source: Option<String>,
}

impl LlmError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(message: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for LlmError {
    /// Go renders `fmt.Sprintf("%s: %v", msg, err)` and plain `msg` without a
    /// cause.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            Some(source) => write!(formatter, "{}: {}", self.message, source),
            None => formatter.write_str(&self.message),
        }
    }
}

impl StdError for LlmError {}

/// Go's `*RateLimitError`: retryable by [`BaseClient::classify`].
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitError {
    pub cause: LlmError,
}

impl fmt::Display for RateLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.cause, formatter)
    }
}

impl StdError for RateLimitError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.cause)
    }
}

/// Go's `*ProviderError`: an unknown or unavailable provider was requested.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderError {
    pub cause: LlmError,
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.cause, formatter)
    }
}

impl StdError for ProviderError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.cause)
    }
}

/// Every failure [`BaseClient::classify`] can return.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientError {
    /// Go's `*Error`, retried while attempts remain.
    Provider(LlmError),
    /// Go's `*RateLimitError`, retried on every attempt but the last.
    RateLimit(RateLimitError),
    /// Go's `*ProviderError` — never retried by the loop.
    UnknownProvider(ProviderError),
    /// Go's `ctx.Err()`: the context won over the attempt.
    Context(String),
    /// Go's `fmt.Errorf("all %d retries exhausted: %w", retries, lastErr)`. Go
    /// only reaches it when the loop body never runs, where `lastErr` is nil and
    /// `%w` renders as the literal `%!w(<nil>)`.
    RetriesExhausted {
        retries: i64,
        source: Option<String>,
    },
    /// A provider whose transport lives in corekit/llmkit.
    TransportNotPorted { provider: String },
    /// A non-retryable foreign failure, surfaced unchanged.
    Foreign(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Provider(error) => fmt::Display::fmt(error, formatter),
            Self::RateLimit(error) => fmt::Display::fmt(error, formatter),
            Self::UnknownProvider(error) => fmt::Display::fmt(error, formatter),
            Self::Context(message) => formatter.write_str(message),
            Self::RetriesExhausted { retries, source } => match source {
                Some(source) => write!(formatter, "all {retries} retries exhausted: {source}"),
                // Go's fmt renders a nil error wrapped with %w this way.
                None => write!(formatter, "all {retries} retries exhausted: %!w(<nil>)"),
            },
            Self::TransportNotPorted { provider } => write!(
                formatter,
                "provider {provider:?} is served by corekit/llmkit, which is not ported to Rust"
            ),
            Self::Foreign(message) => formatter.write_str(message),
        }
    }
}

impl StdError for ClientError {}

/// Go's `ClassifyOptions`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassifyOptions {
    pub max_tokens: i64,
    pub temperature: f64,
    pub cache_key: String,
}

/// Go's `BaseClient`: the shared retry loop with exponential backoff.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BaseClient {
    pub model: String,
    pub max_retries: i64,
    pub cost_tracker: Vec<UsageRecord>,
}

impl BaseClient {
    /// Go's `NewBaseClient`: a non-positive retry count means the default three,
    /// and a missing tracker becomes an empty one.
    pub fn new(model: impl Into<String>, max_retries: i64, cost_tracker: Vec<UsageRecord>) -> Self {
        Self {
            model: model.into(),
            max_retries: if max_retries <= 0 { 3 } else { max_retries },
            cost_tracker,
        }
    }

    /// Go's `(*BaseClient).Classify`: run `call` until it succeeds, the context
    /// wins, or the attempts run out.
    ///
    /// `context_error` stands in for `ctx.Done()`/`ctx.Err()` and answers the text
    /// Go would return; `sleep` stands in for the backoff wait and reports whether
    /// it completed. Both are injected so the loop is exercised without real
    /// delays.
    pub fn classify<C, S, X>(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        options: &ClassifyOptions,
        mut call: C,
        mut sleep: S,
        context_error: X,
    ) -> Result<(String, UsageRecord), ClientError>
    where
        C: FnMut(&str, &str, &ClassifyOptions) -> Result<(String, UsageRecord), ClientError>,
        S: FnMut(Duration) -> bool,
        X: Fn() -> Option<String>,
    {
        let mut attempt: i64 = 1;

        while attempt <= self.max_retries {
            match call(system_prompt, user_prompt, options) {
                Ok(success) => return Ok(success),
                Err(error) => {
                    if let Some(message) = context_error() {
                        return Err(ClientError::Context(message));
                    }
                    let wait = match &error {
                        // The rate limit is retried on every attempt but the last.
                        ClientError::RateLimit(_) if attempt < self.max_retries => {
                            Duration::from_secs(
                                (1u64 << attempt) + hash_cache_key(&options.cache_key) as u64,
                            )
                        }
                        ClientError::RateLimit(_) => return Err(error),
                        // A provider failure is retried on every attempt but the last.
                        ClientError::Provider(_) if attempt < self.max_retries => {
                            Duration::from_secs(1u64 << attempt)
                        }
                        // Not retryable at all: a context error raised by the call
                        // itself, a provider-construction error, a foreign failure.
                        _ => return Err(error),
                    };
                    if !sleep(wait) {
                        return Err(ClientError::Context(
                            context_error().unwrap_or_else(|| "context canceled".to_owned()),
                        ));
                    }
                    attempt += 1;
                }
            }
        }

        Err(ClientError::RetriesExhausted {
            retries: self.max_retries,
            source: None,
        })
    }
}

/// Go's `hashCacheKey`: retry jitter from the cache key, with the empty key
/// short-circuiting to zero without hashing.
pub fn hash_cache_key(cache_key: &str) -> i64 {
    if cache_key.is_empty() {
        return 0;
    }
    let mut hash: u32 = 0x811c_9dc5;
    for byte in cache_key.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    i64::from(hash % 5)
}

/// How a provider is built: `llmkit` transports or the host-agent client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Llmkit,
    Agent,
}

impl ProviderKind {
    /// Go's descriptor spelling, `"llmkit"` or `"agent"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Llmkit => "llmkit",
            Self::Agent => "agent",
        }
    }
}

/// One entry of Go's unexported provider table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderSpec {
    pub name: &'static str,
    pub kind: ProviderKind,
    pub env_key: &'static str,
    pub default_model: &'static str,
}

/// Go's `providers` table, in sorted name order.
pub const PROVIDERS: [ProviderSpec; 5] = [
    ProviderSpec {
        name: "agent",
        kind: ProviderKind::Agent,
        env_key: "",
        default_model: "auto",
    },
    ProviderSpec {
        name: "anthropic",
        kind: ProviderKind::Llmkit,
        env_key: "ANTHROPIC_API_KEY",
        default_model: "claude-sonnet-4-6",
    },
    ProviderSpec {
        name: "ollama",
        kind: ProviderKind::Llmkit,
        env_key: "",
        default_model: "llama3.1",
    },
    ProviderSpec {
        name: "openai",
        kind: ProviderKind::Llmkit,
        env_key: "OPENAI_API_KEY",
        default_model: "gpt-4o",
    },
    ProviderSpec {
        name: "openai-compatible",
        kind: ProviderKind::Llmkit,
        env_key: "",
        default_model: "default",
    },
];

/// Go's `ListAvailableProviders` returns a Go map's iteration order, which is not
/// reproducible; this returns the same set sorted by name.
pub fn list_available_providers() -> Vec<&'static str> {
    PROVIDERS.iter().map(|spec| spec.name).collect()
}

/// Looks up a provider by name, as Go's table does.
pub fn provider_spec(name: &str) -> Option<&'static ProviderSpec> {
    PROVIDERS.iter().find(|spec| spec.name == name)
}

/// Go's `CreateOptions`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreateOptions {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub agent_backend: String,
    pub cost_tracker: Vec<UsageRecord>,
}

/// A host coding-agent CLI, mirroring Go's `agentDefs` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentDef {
    pub name: &'static str,
    pub cli: &'static str,
    pub check_cmd: &'static [&'static str],
    pub invoke_template: &'static [&'static str],
}

/// Go's `agentDefs`.
pub const AGENT_DEFS: [AgentDef; 3] = [
    AgentDef {
        name: "claude",
        cli: "claude",
        check_cmd: &["claude", "--version"],
        invoke_template: &[
            "claude",
            "-p",
            "{prompt}",
            "--output-format",
            "text",
            "--no-input",
        ],
    },
    AgentDef {
        name: "hermes",
        cli: "hermes",
        check_cmd: &["hermes", "--version"],
        invoke_template: &["hermes", "-p", "{prompt}"],
    },
    AgentDef {
        name: "copilot",
        cli: "gh",
        check_cmd: &["gh", "copilot", "--version"],
        invoke_template: &["gh", "copilot", "suggest", "-p", "{prompt}"],
    },
];

/// Go's `agentPreference`: Claude Code, then Hermes, then Copilot.
pub const AGENT_PREFERENCE: [&str; 3] = ["claude", "hermes", "copilot"];

/// The EraseMe-side client: only the host-agent provider has one in Rust.
#[derive(Debug)]
pub struct AgentClient {
    pub base: BaseClient,
    pub requested_backend: String,
    resolved_backend: String,
    available: bool,
}

/// The message Go's agent `callAPI` returns when no CLI is reachable.
pub const NO_AGENT_CLI_MESSAGE: &str = "no host agent CLI detected. Install Claude Code, Hermes or GitHub Copilot CLI, or set SYMERASEME_AGENT_BACKEND";
const AGENT_SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(120);

struct AgentCommandConfig<'a> {
    timeout: Duration,
    executable_override: Option<&'a Path>,
    environment: &'a [(OsString, OsString)],
    inherit_environment: bool,
}

impl AgentClient {
    /// Go's `NewAgentClient`, including the empty-model fallback to `auto`. The
    /// PATH probe is the real one.
    pub fn new(
        model: impl Into<String>,
        agent_backend: impl Into<String>,
        cost_tracker: Vec<UsageRecord>,
    ) -> Self {
        Self::with_probe(model, agent_backend, cost_tracker, &cli_on_path)
    }

    /// Go's `NewAgentClient` with an injected PATH probe.
    ///
    /// Go resolves the backend lazily on the first `IsAvailable()`; this resolves
    /// on construction, which the public contract cannot distinguish.
    pub fn with_probe(
        model: impl Into<String>,
        agent_backend: impl Into<String>,
        cost_tracker: Vec<UsageRecord>,
        on_path: &dyn Fn(&str) -> bool,
    ) -> Self {
        let model = model.into();
        let agent_backend = agent_backend.into();
        let resolved_backend = detect_backend(&agent_backend, on_path);
        let available = !resolved_backend.is_empty();
        Self {
            base: BaseClient::new(model_or_auto(&model), 3, cost_tracker),
            requested_backend: agent_backend,
            resolved_backend,
            available,
        }
    }

    /// Go's `(*AgentClient).IsAvailable`.
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// The backend the agent client resolved.
    pub fn resolved_backend(&self) -> &str {
        &self.resolved_backend
    }

    /// Go's `(*AgentClient).callAPI` as far as this port reaches: without a
    /// reachable CLI the shared retry loop is handed the unchanged error.
    pub fn unavailable_error(&self) -> ClientError {
        ClientError::Provider(LlmError::new(NO_AGENT_CLI_MESSAGE))
    }

    /// Go's `AgentClient.Classify`, using the shared retry loop and real host CLI.
    pub fn classify(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        options: &ClassifyOptions,
    ) -> Result<(String, UsageRecord), ClientError> {
        self.classify_with_timeout(
            system_prompt,
            user_prompt,
            options,
            AGENT_SUBPROCESS_TIMEOUT,
        )
    }

    fn classify_with_timeout(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        options: &ClassifyOptions,
        timeout: Duration,
    ) -> Result<(String, UsageRecord), ClientError> {
        self.classify_with_command(
            system_prompt,
            user_prompt,
            options,
            AgentCommandConfig {
                timeout,
                executable_override: None,
                environment: &[],
                inherit_environment: true,
            },
        )
    }

    fn classify_with_command(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        options: &ClassifyOptions,
        command: AgentCommandConfig<'_>,
    ) -> Result<(String, UsageRecord), ClientError> {
        self.base.classify(
            system_prompt,
            user_prompt,
            options,
            |system, user, _| {
                if command.executable_override.is_none()
                    && command.environment.is_empty()
                    && command.inherit_environment
                {
                    self.call_api(system, user, command.timeout)
                } else {
                    self.call_api_with(
                        system,
                        user,
                        command.timeout,
                        command.executable_override,
                        command.environment,
                        command.inherit_environment,
                    )
                }
            },
            |wait| {
                thread::sleep(wait);
                true
            },
            || None,
        )
    }

    fn call_api(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        timeout: Duration,
    ) -> Result<(String, UsageRecord), ClientError> {
        self.call_api_with(system_prompt, user_prompt, timeout, None, &[], true)
    }

    fn call_api_with(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        timeout: Duration,
        executable_override: Option<&Path>,
        environment: &[(OsString, OsString)],
        inherit_environment: bool,
    ) -> Result<(String, UsageRecord), ClientError> {
        let Some(def) = AGENT_DEFS
            .iter()
            .find(|def| def.name == self.resolved_backend)
        else {
            return Err(self.unavailable_error());
        };

        let combined = combine_prompts(system_prompt, user_prompt);
        let mut arguments = def
            .invoke_template
            .iter()
            .skip(1)
            .map(|part| OsString::from(part.replace("{prompt}", &combined)))
            .collect::<Vec<_>>();
        if !self.base.model.is_empty() && self.base.model != "auto" && def.name == "claude" {
            arguments.push(OsString::from("--model"));
            arguments.push(OsString::from(&self.base.model));
        }

        let executable = executable_override.unwrap_or_else(|| Path::new(def.cli));
        let mut command = Command::new(executable);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !inherit_environment {
            command.env_clear();
        }
        command.envs(environment.iter().cloned());
        // Go appends TERM=dumb to the inherited environment for every agent.
        command.env("TERM", "dumb");

        let start = Instant::now();
        let mut child = command.spawn().map_err(|error| {
            ClientError::Provider(LlmError::new(format!(
                "failed to invoke host agent: {}",
                go_spawn_error(def.cli, executable_override, &error)
            )))
        })?;
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let stdout_reader = thread::spawn(move || read_pipe(stdout));
        let stderr_reader = thread::spawn(move || read_pipe(stderr));

        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if start.elapsed() >= timeout => {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(ClientError::Provider(LlmError::new(format!(
                        "failed to invoke host agent: {}",
                        error
                    ))));
                }
            }
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| {
                ClientError::Provider(LlmError::new(
                    "failed to invoke host agent: stdout capture failed",
                ))
            })?
            .map_err(|error| {
                ClientError::Provider(LlmError::new(format!(
                    "failed to invoke host agent: {}",
                    error
                )))
            })?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| {
                ClientError::Provider(LlmError::new(
                    "failed to invoke host agent: stderr capture failed",
                ))
            })?
            .map_err(|error| {
                ClientError::Provider(LlmError::new(format!(
                    "failed to invoke host agent: {}",
                    error
                )))
            })?;

        if timed_out {
            return Err(ClientError::Provider(LlmError::new(format!(
                "host agent timed out after {}m{}s",
                AGENT_SUBPROCESS_TIMEOUT.as_secs() / 60,
                AGENT_SUBPROCESS_TIMEOUT.as_secs() % 60
            ))));
        }

        let status = status.expect("a completed child has an exit status");
        if !status.success() {
            let stderr = trim_go_space(&stderr);
            let stderr = truncate_go_bytes(stderr, 500);
            return Err(ClientError::Provider(LlmError::new(format!(
                "host agent exited with code {}: {}",
                status.code().unwrap_or(-1),
                stderr
            ))));
        }

        let text = String::from_utf8_lossy(&stdout).trim().to_owned();
        if text.is_empty() {
            return Err(ClientError::Provider(LlmError::new(
                "host agent returned empty response",
            )));
        }
        Ok((
            text,
            UsageRecord {
                model: format!("agent:{}", def.name),
                ..UsageRecord::default()
            },
        ))
    }
}

fn combine_prompts(system_prompt: &str, user_prompt: &str) -> String {
    format!("{system_prompt}\n\n---\n\n{user_prompt}")
}

fn read_pipe(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)?;
    Ok(output)
}

fn trim_go_space(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

fn truncate_go_bytes(text: String, limit: usize) -> String {
    if text.len() <= limit {
        return text;
    }
    String::from_utf8_lossy(&text.as_bytes()[..limit]).into_owned()
}

fn go_spawn_error(cli: &str, override_path: Option<&Path>, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound && override_path.is_none() {
        format!("exec: {cli:?}: executable file not found in $PATH")
    } else {
        error.to_string()
    }
}

/// Go's `modelOrAuto`.
fn model_or_auto(model: &str) -> String {
    if model.is_empty() {
        "auto".to_owned()
    } else {
        model.to_owned()
    }
}

/// Go's `detectBackend`, with the PATH probe injected for tests.
fn detect_backend(explicit: &str, on_path: &dyn Fn(&str) -> bool) -> String {
    if !explicit.is_empty() {
        let lowered = explicit.to_lowercase();
        let matched = AGENT_DEFS.iter().find(|def| def.name == lowered);
        if matched.is_some_and(|def| on_path(def.cli)) {
            return lowered;
        }
        return String::new();
    }
    for key in AGENT_PREFERENCE {
        let matched = AGENT_DEFS.iter().find(|def| def.name == key);
        if matched.is_some_and(|def| on_path(def.cli)) {
            return key.to_owned();
        }
    }
    String::new()
}

/// Go's `cliOnPath`: a PATH lookup with the executable bit.
pub fn cli_on_path(name: &str) -> bool {
    path_entries().iter().any(|entry| {
        let candidate: PathBuf = entry.join(name);
        is_executable(&candidate)
    })
}

fn path_entries() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default()
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Go's `Create`: resolve the provider, then the model, then build the client.
///
/// `env` stands in for `os.Getenv` so resolution is testable without touching the
/// process environment.
pub fn create(
    options: &CreateOptions,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<AgentClient, ClientError> {
    create_with(options, env, &cli_on_path)
}

/// Go's `Create` with an injected PATH probe.
pub fn create_with(
    options: &CreateOptions,
    env: &dyn Fn(&str) -> Option<String>,
    on_path: &dyn Fn(&str) -> bool,
) -> Result<AgentClient, ClientError> {
    let mut provider = if options.provider.is_empty() {
        env("SYMERASEME_LLM_PROVIDER").unwrap_or_default()
    } else {
        options.provider.clone()
    };
    if provider.is_empty() {
        provider = "anthropic".to_owned();
    }
    provider = provider.trim().to_lowercase();

    let Some(spec) = provider_spec(&provider) else {
        return Err(ClientError::UnknownProvider(ProviderError {
            cause: LlmError::new(format!(
                "unknown LLM provider {:?}. Known providers: {}",
                provider,
                list_available_providers().join(", ")
            )),
        }));
    };

    let model = if options.model.is_empty() {
        env("SYMERASEME_LLM_MODEL")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| spec.default_model.to_owned())
    } else {
        options.model.clone()
    };

    if spec.kind == ProviderKind::Agent {
        let agent_backend = if options.agent_backend.is_empty() {
            env("SYMERASEME_AGENT_BACKEND").unwrap_or_default()
        } else {
            options.agent_backend.clone()
        };
        return Ok(AgentClient::with_probe(
            model,
            agent_backend,
            options.cost_tracker.clone(),
            on_path,
        ));
    }

    Err(ClientError::TransportNotPorted {
        provider: provider.clone(),
    })
}

#[cfg(all(test, unix))]
#[path = "host_agent_tests.rs"]
mod host_agent_tests;
