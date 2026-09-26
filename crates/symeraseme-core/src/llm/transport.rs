//! EraseMe adapter over CoreKit's descriptor-driven LLM transport.

use std::time::Duration;
use std::{net::IpAddr, str::FromStr};

use symaira_core_llm::{ChatOptions, Client, ClientBuilder, Error, ErrorCode, Message, lookup};

use super::{BaseClient, ClassifyOptions, ClientError, LlmError, RateLimitError, UsageRecord};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// A provider configured from EraseMe's llmkit-backed provider set.
pub struct LlmkitClient {
    model: String,
    api_key: String,
    client: Client,
}

impl std::fmt::Debug for LlmkitClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LlmkitClient")
            .field("model", &self.model)
            .field("provider", &self.client.descriptor().id)
            .field("base_url", &self.client.base_url())
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
        credential_ref: String,
    ) -> Result<Self, ClientError> {
        let provider_id = if provider == "openai-compatible" {
            "custom"
        } else {
            provider
        };
        let Some(descriptor) = lookup(provider_id) else {
            return Err(ClientError::UnknownProvider(super::ProviderError {
                cause: LlmError::new(format!("unknown LLM provider {provider:?}")),
            }));
        };
        let api_key = api_key.unwrap_or_default();
        let mut builder =
            ClientBuilder::new(descriptor.clone(), credential_ref).timeout(REQUEST_TIMEOUT);
        if !base_url.is_empty() {
            builder = builder.base_url(base_url);
        }
        if !api_key.is_empty() {
            builder = builder.api_key(api_key.clone());
        }
        let client = builder
            .build()
            .map_err(|error| constructor_error(provider, error, &api_key))?;
        Ok(Self {
            model,
            api_key,
            client,
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
        let max_tokens = u32::try_from(options.max_tokens).unwrap_or_default();
        let choice = self
            .client
            .chat(
                &self.model,
                &[Message {
                    role: "user".to_owned(),
                    content: user_prompt.to_owned(),
                }],
                Some(&ChatOptions {
                    temperature: (options.temperature != 0.0).then_some(options.temperature),
                    max_tokens,
                    system: system_prompt.to_owned(),
                    ..ChatOptions::default()
                }),
            )
            .map_err(|error| classify_error(error, &self.api_key))?;
        Ok((
            choice.content.trim().to_owned(),
            UsageRecord {
                model: self.model.clone(),
                ..UsageRecord::default()
            },
        ))
    }
}

fn constructor_error(provider: &str, error: Error, api_key: &str) -> ClientError {
    let detail = redact(error.to_string(), api_key);
    ClientError::Provider(LlmError::with_source(
        format!("llmkit client for {provider:?}: {detail}"),
        detail,
    ))
}

fn classify_error(error: Error, api_key: &str) -> ClientError {
    let message = redact(error.to_string(), api_key);
    let cause = LlmError::with_source(message.clone(), message);
    if error.code == ErrorCode::RateLimited {
        ClientError::RateLimit(RateLimitError { cause })
    } else {
        ClientError::Provider(cause)
    }
}

fn redact(message: String, api_key: &str) -> String {
    if api_key.is_empty() {
        message
    } else {
        message.replace(api_key, "[REDACTED]")
    }
}

/// Preserve Go's constructor validation order before credential lookup. The
/// CoreKit builder applies the same transport policy again when it is built.
pub(super) fn validate_provider_base_url(
    provider: &str,
    base_url: &str,
) -> Result<(), ClientError> {
    if base_url.is_empty() || provider == "ollama" {
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
    let (Some(scheme), Some(host)) = (uri.scheme_str(), uri.host()) else {
        return Err(auth_url_error(provider, provider_id, false));
    };
    if scheme == "https" || is_loopback_host(host) {
        Ok(())
    } else {
        Err(auth_url_error(provider, provider_id, true))
    }
}

fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
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
