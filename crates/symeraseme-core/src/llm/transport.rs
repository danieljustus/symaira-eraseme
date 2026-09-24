//! Non-streaming llmkit-compatible chat transports used by EraseMe's classifier.

use std::fmt;
use std::io::{Read, Take};
use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{BaseClient, ClassifyOptions, ClientError, LlmError, RateLimitError, UsageRecord};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_RESPONSE_BYTES: u64 = 16 << 20;
const MAX_ERROR_BYTES: u64 = 8 << 10;
const ANTHROPIC_MAX_TOKENS: i64 = 8192;

#[derive(Clone, Copy)]
enum Dialect {
    OpenAi,
    Anthropic,
}

#[derive(Clone, Copy)]
enum Auth {
    None,
    Bearer,
    Header(&'static str),
}

/// A provider configured from EraseMe's llmkit-backed provider set.
pub struct LlmkitClient {
    model: String,
    provider: String,
    base_url: String,
    api_key: String,
    dialect: Dialect,
    auth: Auth,
    agent: ureq::Agent,
}

impl fmt::Debug for LlmkitClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LlmkitClient")
            .field("model", &self.model)
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("has_api_key", &!self.api_key.is_empty())
            .finish_non_exhaustive()
    }
}

impl LlmkitClient {
    pub(super) fn new(
        provider: &str,
        model: String,
        base_url: String,
        api_key: Option<String>,
    ) -> Result<Self, ClientError> {
        let (base_default, dialect, auth, default_model, custom_url) = match provider {
            "anthropic" => (
                "https://api.anthropic.com",
                Dialect::Anthropic,
                Auth::Header("x-api-key"),
                "claude-sonnet-4-6",
                false,
            ),
            "openai" => (
                "https://api.openai.com/v1",
                Dialect::OpenAi,
                Auth::Bearer,
                "gpt-4o",
                false,
            ),
            "ollama" => (
                "http://localhost:11434/v1",
                Dialect::OpenAi,
                Auth::None,
                "llama3.1",
                false,
            ),
            "openai-compatible" => ("", Dialect::OpenAi, Auth::Bearer, "default", true),
            _ => {
                return Err(ClientError::UnknownProvider(super::ProviderError {
                    cause: LlmError::new(format!("unknown LLM provider {provider:?}")),
                }));
            }
        };
        let model = if model.is_empty() {
            default_model.to_owned()
        } else {
            model
        };
        let base_url = if base_url.is_empty() {
            base_default.to_owned()
        } else {
            base_url
        };
        if base_url.is_empty() && custom_url {
            let detail =
                "llmkit: provider \"custom\" requires a base URL override (WithBaseURL)".to_owned();
            return Err(ClientError::Provider(LlmError::with_source(
                format!("llmkit client for {provider:?}: {detail}"),
                detail,
            )));
        }
        let api_key = api_key.unwrap_or_default();
        if !matches!(auth, Auth::None) && api_key.is_empty() {
            let detail = if custom_url {
                "llmkit: auth_failure: no credential reference or default provided".to_owned()
            } else {
                let variable = if provider == "anthropic" {
                    "ANTHROPIC_API_KEY"
                } else {
                    "OPENAI_API_KEY"
                };
                format!(
                    "llmkit: auth_failure: environment variable {variable} is not set (reference {variable})"
                )
            };
            return Err(ClientError::Provider(LlmError::with_source(
                format!("llmkit client for {provider:?}: {detail}"),
                detail,
            )));
        }
        validate_base_url(&base_url, auth, provider)?;

        let host_is_loopback = is_loopback_url(&base_url);
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(!host_is_loopback && !matches!(auth, Auth::None))
            .timeout_global(Some(REQUEST_TIMEOUT))
            .timeout_connect(Some(REQUEST_TIMEOUT))
            .timeout_recv_response(Some(REQUEST_TIMEOUT))
            .timeout_recv_body(Some(REQUEST_TIMEOUT))
            .max_redirects(0)
            .max_redirects_will_error(true)
            .build();

        Ok(Self {
            model,
            provider: provider.to_owned(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key,
            dialect,
            auth,
            agent: ureq::Agent::new_with_config(config),
        })
    }

    pub fn is_available(&self) -> bool {
        true
    }

    pub fn classify(
        &self,
        base: &BaseClient,
        system_prompt: &str,
        user_prompt: &str,
        options: &ClassifyOptions,
    ) -> Result<(String, UsageRecord), ClientError> {
        base.classify(
            system_prompt,
            user_prompt,
            options,
            |system, user, options| self.call_api(system, user, options),
            |wait| {
                std::thread::sleep(wait);
                true
            },
            || None,
        )
    }

    fn call_api(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        options: &ClassifyOptions,
    ) -> Result<(String, UsageRecord), ClientError> {
        let (path, body) = match self.dialect {
            Dialect::OpenAi => (
                "/chat/completions",
                serde_json::to_vec(&OpenAiRequest::new(
                    &self.model,
                    system_prompt,
                    user_prompt,
                    options,
                ))
                .map_err(|error| provider_error("encode request", error.to_string()))?,
            ),
            Dialect::Anthropic => (
                "/messages",
                serde_json::to_vec(&AnthropicRequest::new(
                    &self.model,
                    system_prompt,
                    user_prompt,
                    options,
                ))
                .map_err(|error| provider_error("encode request", error.to_string()))?,
            ),
        };
        let url = format!("{}{path}", self.base_url);
        let mut request = self
            .agent
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
        request = match self.auth {
            Auth::None => request,
            Auth::Bearer => request.header("Authorization", &format!("Bearer {}", self.api_key)),
            Auth::Header(name) => request.header(name, &self.api_key),
        };
        if matches!(self.dialect, Dialect::Anthropic) {
            request = request.header("anthropic-version", "2023-06-01");
        }
        let mut response = request
            .send(&body)
            .map_err(|error| provider_error("build request", error.to_string()))?;
        let status = response.status().as_u16();
        if status >= 300 {
            let mut reader = response.body_mut().as_reader().take(MAX_ERROR_BYTES);
            let body = read_body(&mut reader)
                .map_err(|error| provider_error("read response", error.to_string()))?;
            return Err(http_error(status, &body, &self.api_key));
        }
        let mut reader = response.body_mut().as_reader().take(MAX_RESPONSE_BYTES);
        let body = read_body(&mut reader)
            .map_err(|error| provider_error("read response", error.to_string()))?;
        let text = match self.dialect {
            Dialect::OpenAi => {
                let parsed: OpenAiResponse = serde_json::from_slice(&body)
                    .map_err(|error| provider_error("decode chat response", error.to_string()))?;
                let Some(choice) = parsed.choices.first() else {
                    return Err(provider_error(
                        "chat response contained no choices",
                        "".to_owned(),
                    ));
                };
                choice.message.content.clone()
            }
            Dialect::Anthropic => {
                let parsed: AnthropicResponse = serde_json::from_slice(&body).map_err(|error| {
                    provider_error("decode anthropic response", error.to_string())
                })?;
                parsed
                    .content
                    .iter()
                    .map(|part| part.text.as_str())
                    .filter(|part| !part.is_empty())
                    .collect::<String>()
            }
        };
        Ok((
            text.trim().to_owned(),
            UsageRecord {
                model: self.model.clone(),
                ..UsageRecord::default()
            },
        ))
    }
}

pub(super) fn validate_provider_base_url(
    provider: &str,
    base_url: &str,
) -> Result<(), ClientError> {
    if base_url.is_empty() {
        return Ok(());
    }
    let auth = if provider == "ollama" {
        Auth::None
    } else if provider == "anthropic" {
        Auth::Header("x-api-key")
    } else {
        Auth::Bearer
    };
    validate_base_url(base_url, auth, provider)
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "is_zero")]
    max_tokens: i64,
    stream: bool,
}

impl<'a> OpenAiRequest<'a> {
    fn new(model: &'a str, system: &'a str, user: &'a str, options: &ClassifyOptions) -> Self {
        let mut messages = Vec::with_capacity(2);
        if !system.is_empty() {
            messages.push(OpenAiMessage {
                role: "system",
                content: system,
            });
        }
        messages.push(OpenAiMessage {
            role: "user",
            content: user,
        });
        Self {
            model,
            messages,
            temperature: (options.temperature != 0.0).then_some(options.temperature),
            max_tokens: options.max_tokens,
            stream: false,
        }
    }
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: i64,
    messages: [AnthropicMessage<'a>; 1],
    #[serde(skip_serializing_if = "str::is_empty")]
    system: &'a str,
}

impl<'a> AnthropicRequest<'a> {
    fn new(model: &'a str, system: &'a str, user: &'a str, options: &ClassifyOptions) -> Self {
        Self {
            model,
            max_tokens: if options.max_tokens > 0 {
                options.max_tokens
            } else {
                ANTHROPIC_MAX_TOKENS
            },
            messages: [AnthropicMessage {
                role: "user",
                content: user,
            }],
            system,
        }
    }
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    message: OpenAiResponseMessage,
}

#[derive(Deserialize, Default)]
struct OpenAiResponseMessage {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(default)]
    text: String,
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

fn read_body(reader: &mut Take<impl Read>) -> std::io::Result<Vec<u8>> {
    let mut body = Vec::new();
    reader.read_to_end(&mut body)?;
    Ok(body)
}

fn provider_error(stage: &str, source: String) -> ClientError {
    let detail = if source.is_empty() {
        format!("llmkit: provider_error: {stage}")
    } else {
        format!("llmkit: provider_error: {stage}: {source}")
    };
    ClientError::Provider(LlmError::with_source(detail.clone(), detail))
}

fn http_error(status: u16, body: &[u8], api_key: &str) -> ClientError {
    let body = String::from_utf8_lossy(body);
    let body = if api_key.is_empty() {
        body.into_owned()
    } else {
        body.replace(api_key, "[REDACTED]")
    };
    let body = body.trim();
    let excerpt = if body.len() > 512 {
        let mut end = 512;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        &body[..end]
    } else {
        body
    };
    let lower = excerpt.to_lowercase();
    let code = match status {
        401 | 403 => "auth_failure",
        429 => "rate_limited",
        404 => "model_not_found",
        400 if [
            "context_length_exceeded",
            "maximum context length",
            "context window",
            "too many tokens",
            "input length exceeds",
        ]
        .iter()
        .any(|marker| lower.contains(marker)) =>
        {
            "context_overflow"
        }
        _ => "provider_error",
    };
    let detail = if excerpt.is_empty() {
        format!("llmkit: {code} (status {status})")
    } else {
        format!("llmkit: {code} (status {status}): {excerpt}")
    };
    let cause = LlmError::with_source(detail.clone(), detail);
    if code == "rate_limited" {
        ClientError::RateLimit(RateLimitError { cause })
    } else {
        ClientError::Provider(cause)
    }
}

fn validate_base_url(base_url: &str, auth: Auth, provider: &str) -> Result<(), ClientError> {
    if matches!(auth, Auth::None) {
        return Ok(());
    }
    let provider_id = if provider == "openai-compatible" {
        "custom"
    } else {
        provider
    };
    let Ok(uri) = ureq::http::Uri::from_str(base_url) else {
        return Err(auth_url_error(provider, provider_id, false));
    };
    let (Some(_scheme), Some(host)) = (uri.scheme_str(), uri.host()) else {
        return Err(auth_url_error(provider, provider_id, false));
    };
    if uri.scheme_str() == Some("https") || is_loopback_host(host) {
        Ok(())
    } else {
        Err(auth_url_error(provider, provider_id, true))
    }
}

fn is_loopback_url(base_url: &str) -> bool {
    ureq::http::Uri::from_str(base_url)
        .ok()
        .and_then(|uri| uri.host().map(is_loopback_host))
        .unwrap_or(false)
}

fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    IpAddr::from_str(host)
        .map(|address| address.is_loopback())
        .unwrap_or(false)
}

fn auth_url_error(provider: &str, provider_id: &str, outside_loopback: bool) -> ClientError {
    let reason = if outside_loopback {
        "requires an HTTPS base URL for credentialed requests outside loopback hosts"
    } else {
        "requires an HTTPS base URL for credentialed requests"
    };
    let detail = format!("llmkit: auth_failure: provider {provider_id:?} {reason}");
    ClientError::Provider(LlmError::with_source(
        format!("llmkit client for {provider:?}: {detail}"),
        detail,
    ))
}
