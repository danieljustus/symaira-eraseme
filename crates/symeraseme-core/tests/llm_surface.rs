//! Replays the recorded LLM provider surface against the port.
//!
//! The oracle (`rust-tests/parity/oracle/llm-provider-surface`) drove the real Go
//! implementation with a frozen environment and frozen inputs and recorded what it
//! answered. This replays the same cases against `symeraseme_core::llm`.
//!
//! The provider factory, local HTTP transport and response handling are replayed
//! separately from this fixture using source-bound Go request observations.

use std::time::Duration;

use serde_json::Value;
use symeraseme_core::llm::{
    AgentClient, BaseClient, ClassifyOptions, ClientError, CreateOptions, LlmError, RateLimitError,
    UsageRecord, create_with, hash_cache_key, list_available_providers,
};

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/llm-provider-surface/cases.json"
));

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("the recorded llm surface fixture parses")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("a string array")
        .iter()
        .map(|entry| entry.as_str().expect("a string entry").to_owned())
        .collect()
}

/// The Go type name of a failure, as the oracle recorded it.
fn go_error_type(error: &ClientError) -> &'static str {
    match error {
        ClientError::Provider(_) => "Error",
        ClientError::RateLimit(_) => "RateLimitError",
        ClientError::UnknownProvider(_) => "ProviderError",
        ClientError::Context(_) => "*errors.errorString",
        ClientError::RetriesExhausted { .. } => "*fmt.wrapError",
        ClientError::Foreign(_) => "*errors.errorString",
        ClientError::TransportNotPorted { .. } => "ClientError",
    }
}

fn split_provider_message(message: &str) -> (String, Vec<String>) {
    const MARKER: &str = "Known providers: ";
    match message.find(MARKER) {
        Some(index) => {
            let mut names = message[index + MARKER.len()..]
                .split(", ")
                .map(str::to_owned)
                .collect::<Vec<_>>();
            names.sort();
            (message[..index + MARKER.len()].to_owned(), names)
        }
        None => (message.to_owned(), Vec::new()),
    }
}

#[test]
fn provider_set_matches_the_recorded_names() {
    let expected = strings(&fixture()["providers"]);
    assert_eq!(list_available_providers(), expected);
}

#[test]
fn usage_records_match_the_recorded_map() {
    let fixture = fixture();
    for case in fixture["usage_records"].as_array().expect("usage records") {
        let input = &case["input"];
        let record = UsageRecord {
            model: input["model"].as_str().expect("model").to_owned(),
            input_tokens: input["input_tokens"].as_i64().expect("input tokens"),
            output_tokens: input["output_tokens"].as_i64().expect("output tokens"),
            cache_creation_tokens: input["cache_creation_tokens"]
                .as_i64()
                .expect("cache creation tokens"),
            cache_read_tokens: input["cache_read_tokens"]
                .as_i64()
                .expect("cache read tokens"),
            cost: input["cost"].as_f64().expect("cost"),
        };
        let rendered = record.record();
        let expected = &case["record"];
        assert_eq!(expected.as_object().expect("a map").len(), 6, "six keys");
        assert_eq!(rendered["model"], expected["model"]);
        assert_eq!(rendered["input_tokens"], expected["input_tokens"]);
        assert_eq!(rendered["output_tokens"], expected["output_tokens"]);
        assert_eq!(
            rendered["cache_creation_tokens"],
            expected["cache_creation_tokens"]
        );
        assert_eq!(rendered["cache_read_tokens"], expected["cache_read_tokens"]);
        // Go renders an integral float without a decimal point, so the cost is
        // compared as the number it is.
        let rendered_cost = rendered["cost"].as_f64().expect("a numeric cost");
        let expected_cost = expected["cost"].as_f64().expect("a numeric cost");
        assert_eq!(rendered_cost, expected_cost, "cost {rendered:?}");
    }
}

#[test]
fn cache_key_jitter_matches_the_go_hash() {
    let fixture = fixture();
    let jitter = fixture["cache_key_jitter"].as_object().expect("jitter map");
    assert!(!jitter.is_empty(), "the fixture lost its jitter inputs");
    for (key, expected) in jitter {
        assert_eq!(
            hash_cache_key(key),
            expected.as_i64().expect("a jitter value"),
            "jitter for {key:?}"
        );
    }
}

#[test]
fn create_cases_match_the_recorded_resolution() {
    let fixture = fixture();
    let cases = fixture["create_cases"].as_array().expect("create cases");
    assert!(!cases.is_empty(), "the fixture lost its create cases");

    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let environment = case["env"].as_object().cloned().unwrap_or_default();
        let env = |name: &str| {
            environment
                .get(name)
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        };
        let options = CreateOptions {
            provider: case["provider"].as_str().unwrap_or("").to_owned(),
            model: case["model"].as_str().unwrap_or("").to_owned(),
            agent_backend: case["agent_backend"].as_str().unwrap_or("").to_owned(),
            ..CreateOptions::default()
        };

        // An empty PATH is what the oracle ran with, so nothing is on it.
        match create_with(&options, &env, &|_| false) {
            Ok(client) => {
                assert!(
                    case["client"].as_bool().unwrap_or(false),
                    "{id} unexpectedly built a client"
                );
                assert_eq!(
                    client.base.model,
                    case["agent_model"].as_str().expect("agent model"),
                    "{id} model"
                );
                assert_eq!(
                    client.base.max_retries,
                    case["agent_max_retries"].as_i64().expect("retries"),
                    "{id} retries"
                );
                assert_eq!(
                    client.base.cost_tracker.len() as i64,
                    case["agent_tracker_len"].as_i64().expect("tracker"),
                    "{id} tracker"
                );
                assert_eq!(
                    client.is_available(),
                    case["agent_available"].as_bool().expect("availability"),
                    "{id} availability"
                );
                let requested_backend = options.agent_backend.as_str();
                let requested_backend = if requested_backend.is_empty() {
                    env("SYMERASEME_AGENT_BACKEND").unwrap_or_default()
                } else {
                    requested_backend.to_owned()
                };
                assert_eq!(client.requested_backend, requested_backend, "{id} backend");
            }
            Err(error) => {
                assert!(
                    !case["client"].as_bool().unwrap_or(true),
                    "{id} unexpectedly failed: {error}"
                );
                assert_eq!(
                    go_error_type(&error),
                    case["error_type"].as_str().expect("error type"),
                    "{id} error type"
                );
                let (prefix, names) = split_provider_message(&error.to_string());
                assert_eq!(
                    prefix,
                    case["error_prefix"].as_str().expect("error prefix"),
                    "{id} error prefix"
                );
                assert_eq!(names, strings(&case["error_names"]), "{id} known providers");
            }
        }
    }
}

#[test]
fn agent_backend_environment_selects_an_available_cli() {
    let options = CreateOptions {
        provider: "agent".to_owned(),
        ..CreateOptions::default()
    };
    let client = create_with(
        &options,
        &|name| (name == "SYMERASEME_AGENT_BACKEND").then(|| "claude".to_owned()),
        &|name| name == "claude",
    )
    .expect("agent provider");
    assert_eq!(client.requested_backend, "claude");
    assert_eq!(client.resolved_backend(), "claude");
    assert!(client.is_available());
}

#[test]
fn retry_cases_match_the_recorded_attempt_accounting() {
    let fixture = fixture();
    let jitter = fixture["cache_key_jitter"]
        .as_object()
        .expect("jitter map")
        .clone();
    let cases = fixture["retry_cases"].as_array().expect("retry cases");
    assert!(!cases.is_empty(), "the fixture lost its retry cases");

    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let max_retries = case["max_retries"].as_i64().expect("max retries");
        let cache_key = case["cache_key"].as_str().unwrap_or("");
        let options = ClassifyOptions {
            max_tokens: case["max_tokens"].as_i64().expect("max tokens"),
            temperature: case["temperature"].as_f64().expect("temperature"),
            cache_key: cache_key.to_owned(),
        };
        let base = if case["zero_value"].as_bool().unwrap_or(false) {
            BaseClient {
                model: "oracle-model".to_owned(),
                max_retries,
                cost_tracker: Vec::new(),
            }
        } else {
            BaseClient::new("oracle-model", max_retries, Vec::new())
        };

        let script = case["script"].as_array().cloned().unwrap_or_default();
        let cancelled = id == "cancelled-context-wins-over-retry";
        let context = move || cancelled.then(|| "context canceled".to_owned());

        let mut attempts = 0_i64;
        let mut sleeps: Vec<Duration> = Vec::new();

        let result = if id == "host-agent-unavailable" {
            let agent = AgentClient::with_probe("auto", "", Vec::new(), &|_| false);
            base.classify(
                "system",
                "user",
                &options,
                |_, _, _| {
                    attempts += 1;
                    Err(agent.unavailable_error())
                },
                |wait| {
                    sleeps.push(wait);
                    true
                },
                context,
            )
        } else {
            base.classify(
                "system",
                "user",
                &options,
                |_, _, _| {
                    let step = script
                        .get((attempts as usize).min(script.len().saturating_sub(1)))
                        .cloned();
                    attempts += 1;
                    let Some(step) = step else {
                        return Err(ClientError::Foreign("no scripted answer".to_owned()));
                    };
                    match step["kind"].as_str().unwrap_or("error") {
                        "ok" => Ok((
                            step["text"].as_str().unwrap_or("").to_owned(),
                            UsageRecord {
                                model: "oracle-model".to_owned(),
                                input_tokens: 10,
                                output_tokens: 2,
                                cost: 0.001,
                                ..UsageRecord::default()
                            },
                        )),
                        "rate_limit" => Err(ClientError::RateLimit(RateLimitError {
                            cause: LlmError::new(""),
                        })),
                        "ctx_error" => Err(ClientError::Context("context canceled".to_owned())),
                        _ => Err(ClientError::Foreign(
                            step["msg"].as_str().unwrap_or("").to_owned(),
                        )),
                    }
                },
                |wait| {
                    sleeps.push(wait);
                    true
                },
                context,
            )
        };

        if case["attempts_observed"].as_bool().unwrap_or(false) {
            assert_eq!(
                attempts,
                case["attempts"].as_i64().expect("attempts"),
                "{id} attempt count"
            );
        }

        let recorded_error = case["error"].as_str();
        match (&result, recorded_error) {
            (Ok(_), None) | (Err(_), Some(_)) => {}
            _ => panic!(
                "{id}: the oracle recorded {recorded_error:?} but the port returned {result:?}"
            ),
        }

        match &result {
            Ok((text, usage)) => {
                assert_eq!(text, case["text"].as_str().unwrap_or(""), "{id} text");
                assert_eq!(
                    usage.model,
                    case["usage"]["model"].as_str().expect("usage model"),
                    "{id} usage model"
                );
                assert_eq!(
                    usage.input_tokens,
                    case["usage"]["input_tokens"]
                        .as_i64()
                        .expect("input tokens"),
                    "{id} input tokens"
                );
                assert_eq!(
                    usage.output_tokens,
                    case["usage"]["output_tokens"]
                        .as_i64()
                        .expect("output tokens"),
                    "{id} output tokens"
                );
                assert_eq!(usage.cost, 0.001, "{id} cost");
            }
            Err(error) => {
                assert_eq!(
                    error.to_string(),
                    recorded_error.expect("recorded error text"),
                    "{id} error text"
                );
                assert_eq!(
                    go_error_type(error),
                    case["error_type"].as_str().unwrap_or(""),
                    "{id} error type"
                );
            }
        }

        // The loop's arithmetic: 2^attempt seconds, plus the cache-key jitter for a
        // rate limit. The jitter inputs themselves are pinned above.
        let first_kind = script
            .first()
            .and_then(|step| step["kind"].as_str())
            .unwrap_or("");
        let key_jitter = jitter
            .get(cache_key)
            .and_then(Value::as_u64)
            .unwrap_or_else(|| hash_cache_key(cache_key) as u64);
        for (index, wait) in sleeps.iter().enumerate() {
            let attempt = index as u32 + 1;
            let expected = if first_kind == "rate_limit" {
                Duration::from_secs((1_u64 << attempt) + key_jitter)
            } else {
                Duration::from_secs(1_u64 << attempt)
            };
            assert_eq!(*wait, expected, "{id} backoff before attempt {attempt}");
        }
    }
}
