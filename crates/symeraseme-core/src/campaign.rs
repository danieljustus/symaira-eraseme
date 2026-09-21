//! Campaign planning and plan reads (DOM-002).
//!
//! Ports `internal/campaign/campaign.go` and `planning.go`, which mirror
//! `src/symeraseme/core/planning.py`. The shared golden fixture is
//! `tests/fixtures/event-store/golden-plan.json`, checked by Go's
//! `TestGoldenPlanConformance`.
//!
//! Deviations from Go, all deliberate:
//! - The `PLANNED` event instant is injected instead of read from
//!   `time.Now()`, matching the rest of this crate's clock convention.
//! - Go resolves the identity snapshot hash inside `PlanCampaign`; this port
//!   takes the resolved hash, so the planner carries no keyring dependency and
//!   the caller owns the "missing profile means an empty hash" policy that the
//!   golden fixture exercises. `identity::hash_profile` produces the value.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::identity::Profile;
use crate::jsonorder::go_map_order;
use crate::registry::{Broker, BrokerFilter, Channel, RegistryError, Template, filter_brokers};
use crate::storage::Store;
use crate::storage::projection::ProjectionError;
use crate::storage::repository::{ListRemovalRequestsOptions, Repository};
use crate::storage::types::{EventType, Source};

/// Empty strings mean "no filter"; `status` defaults to `active`, matching the
/// Python loader default. `max_brokers <= 0` disables the cap.
#[derive(Debug, Clone, Default)]
pub struct PlanOpts {
    pub campaign_id: String,
    pub jurisdiction: String,
    pub law: String,
    pub priority: String,
    pub category: String,
    pub status: String,
    pub include_inactive: bool,
    pub include_disabled: bool,
    pub max_brokers: i64,
    pub notes: String,
}

/// One planned removal request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanRequest {
    pub request_id: i64,
    pub broker_id: String,
    pub broker_name: String,
    pub channel: String,
    pub template: String,
}

/// The `plan_campaign` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanResult {
    pub campaign_id: String,
    pub total_brokers: i64,
    pub matched: i64,
    pub planned: i64,
    /// Go appends to a nil slice, so a plan that matched nothing marshals as
    /// `null` rather than `[]`.
    #[serde(serialize_with = "serialize_go_appended_slice")]
    pub requests: Vec<PlanRequest>,
}

/// Go's zero value for a slice built by `append` is nil, which marshals as
/// `null`; only a slice made with `make` marshals as `[]`.
fn serialize_go_appended_slice<S: serde::Serializer>(
    values: &[PlanRequest],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if values.is_empty() {
        serializer.serialize_none()
    } else {
        values.serialize(serializer)
    }
}

#[derive(Debug)]
pub enum PlanError {
    Registry(RegistryError),
    Database(rusqlite::Error),
    Projection(ProjectionError),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Registry(error) => write!(formatter, "{error}"),
            Self::Database(error) => write!(formatter, "{error}"),
            Self::Projection(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for PlanError {}

impl From<rusqlite::Error> for PlanError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<ProjectionError> for PlanError {
    fn from(error: ProjectionError) -> Self {
        Self::Projection(error)
    }
}

/// The opt-out channel planning selected for one broker.
enum SelectedChannel {
    Email {
        endpoint: String,
        template: Option<Template>,
        locale: String,
        expected_response_days: i64,
    },
    WebForm {
        url: String,
    },
}

impl SelectedChannel {
    fn kind(&self) -> &'static str {
        match self {
            Self::Email { .. } => "email",
            Self::WebForm { .. } => "web_form",
        }
    }

    fn endpoint(&self) -> &str {
        match self {
            Self::Email { endpoint, .. } => endpoint,
            Self::WebForm { url } => url,
        }
    }

    fn expected_response_days(&self) -> i64 {
        match self {
            Self::Email {
                expected_response_days,
                ..
            } => *expected_response_days,
            // Python's WebFormOptOut branch carries no template or locale and
            // always reports 30 days.
            Self::WebForm { .. } => 30,
        }
    }

    fn locale(&self) -> &str {
        match self {
            Self::Email { locale, .. } => locale,
            Self::WebForm { .. } => "",
        }
    }
}

/// Go's `selectChannel`: the first opt-out channel that is an email or a web
/// form wins. The registry model carries exactly those two channel kinds, so
/// this is the first channel.
fn select_channel(broker: &Broker) -> Option<SelectedChannel> {
    match broker.opt_out.first()? {
        Channel::Email {
            endpoint,
            template,
            locale,
            expected_response_days,
            ..
        } => {
            let days = match expected_response_days {
                Some(days) if *days > 0 => i64::from(*days),
                _ => 30,
            };
            Some(SelectedChannel::Email {
                endpoint: endpoint.clone(),
                template: *template,
                locale: locale.clone().unwrap_or_default(),
                expected_response_days: days,
            })
        }
        Channel::WebForm { url, .. } => Some(SelectedChannel::WebForm { url: url.clone() }),
    }
}

/// The wire name of a registry template, as it appears in the schema.
fn template_name(template: Template) -> &'static str {
    match template {
        Template::CcpaDeletion => "ccpa-deletion",
        Template::GdprArt17 => "gdpr-art17",
    }
}

/// Go's `resolveTemplate`: `<template>[.<locale>].md.j2`, else empty.
fn resolve_template(channel: &SelectedChannel) -> String {
    let SelectedChannel::Email {
        template, locale, ..
    } = channel
    else {
        return String::new();
    };
    let Some(template) = template else {
        return String::new();
    };
    let name = template_name(*template);
    if locale.is_empty() {
        format!("{name}.md.j2")
    } else {
        format!("{name}.{locale}.md.j2")
    }
}

/// Go's `resolveJurisdiction`: the requested jurisdiction when the broker has
/// it, else the broker's first, else `UNKNOWN`.
fn resolve_jurisdiction(broker: &Broker, requested: &str) -> String {
    let wire_names: Vec<String> = broker
        .jurisdictions
        .iter()
        .map(|jurisdiction| serde_json::to_value(jurisdiction).unwrap_or(Value::Null))
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect();
    if !requested.is_empty() && wire_names.iter().any(|name| name == requested) {
        return requested.to_owned();
    }
    wire_names
        .first()
        .cloned()
        .unwrap_or_else(|| "UNKNOWN".to_owned())
}

/// Implements the core planning flow against a store.
pub fn plan_campaign(
    store: &Store,
    brokers: &[Broker],
    identity_hash: &str,
    opts: &PlanOpts,
    now: DateTime<Utc>,
) -> Result<PlanResult, PlanError> {
    let repository = Repository::new(store);
    // Go ignores the boolean: a pre-existing campaign id is appended to.
    repository.create_campaign(&opts.campaign_id, "initial", &opts.notes)?;

    let status = if opts.status.is_empty() {
        "active".to_owned()
    } else {
        opts.status.clone()
    };
    let filtered = filter_brokers(
        brokers,
        &BrokerFilter {
            jurisdiction: (!opts.jurisdiction.is_empty()).then(|| opts.jurisdiction.clone()),
            law: (!opts.law.is_empty()).then(|| opts.law.clone()),
            priority: (!opts.priority.is_empty()).then(|| opts.priority.clone()),
            category: (!opts.category.is_empty()).then(|| opts.category.clone()),
            include_disabled: opts.include_disabled,
            status: Some(status),
            include_inactive: opts.include_inactive,
        },
    );
    let total_brokers = filtered.len() as i64;

    let mut selected: Vec<(&Broker, SelectedChannel)> = Vec::new();
    for broker in filtered {
        if let Some(channel) = select_channel(broker) {
            selected.push((broker, channel));
        }
    }
    let matched = selected.len() as i64;
    if opts.max_brokers > 0 && selected.len() as i64 > opts.max_brokers {
        selected.truncate(opts.max_brokers as usize);
    }

    let mut result = PlanResult {
        campaign_id: opts.campaign_id.clone(),
        total_brokers,
        matched,
        planned: 0,
        requests: Vec::new(),
    };
    for (broker, channel) in selected {
        let template_id = resolve_template(&channel);
        let request_id = repository.create_removal_request(
            &broker.id,
            channel.kind(),
            &opts.campaign_id,
            &resolve_jurisdiction(broker, &opts.jurisdiction),
            &template_id,
            identity_hash,
        )?;
        let mut payload = Map::new();
        payload.insert("broker_name".to_owned(), json!(broker.name));
        payload.insert("broker_website".to_owned(), json!(broker.website));
        payload.insert("channel".to_owned(), json!(channel.kind()));
        payload.insert("endpoint".to_owned(), json!(channel.endpoint()));
        payload.insert("template".to_owned(), json!(template_id));
        payload.insert("locale".to_owned(), json!(channel.locale()));
        payload.insert(
            "expected_response_days".to_owned(),
            json!(channel.expected_response_days()),
        );
        store.append_and_project(
            request_id,
            &EventType::Planned,
            &payload,
            &Source::System,
            now,
        )?;
        result.requests.push(PlanRequest {
            request_id,
            broker_id: broker.id.clone(),
            broker_name: broker.name.clone(),
            channel: channel.kind().to_owned(),
            template: template_id,
        });
    }
    result.planned = result.requests.len() as i64;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Execution (`internal/campaign/execution.go` + `batch.go`)
// ---------------------------------------------------------------------------

/// The outcome of one executed request, as Go's `map[string]any`.
pub type ExecuteResult = Map<String, Value>;

/// The resolved identity profile, or the message of the failure that resolving
/// it produced. Go resolves the profile inside `ExecuteRequest`, so a broken
/// profile fails one request instead of the whole batch; the caller owns the
/// resolution here, as it does for planning, and hands the failure through.
pub type ProfileSource<'a> = Result<Option<&'a Profile>, &'a str>;

/// Go's `ExecuteOpts`, minus the injectable adapters.
///
/// `realPlanCommand` leaves `ExecuteOpts.Email` nil and supplies a
/// `WebFormAdapter` without a `FormExecutor`, so neither adapter can reach the
/// network: the email branch always stops at `email_sender is required`, and the
/// web-form branch always previews (dry run) or records a manual task. This port
/// carries no adapter fields at all, which makes that structural.
#[derive(Debug, Clone, Default)]
pub struct ExecuteOpts<'a> {
    pub account: String,
    pub dry_run: bool,
    /// The registry the web-form adapter previews against, as
    /// `realPlanCommand` passes `loadRegistry()` to `NewWebFormAdapter`.
    pub brokers: &'a [Broker],
}

#[derive(Debug)]
pub enum ExecuteError {
    RequestNotFound(i64),
    IdentityProfile(String),
    IdentityProfileNotFound,
    EmailSenderRequired,
    Database(rusqlite::Error),
    Projection(ProjectionError),
}

impl std::fmt::Display for ExecuteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestNotFound(id) => {
                write!(formatter, "campaign: removal request not found: {id}")
            }
            Self::IdentityProfile(message) => write!(formatter, "{message}"),
            Self::IdentityProfileNotFound => write!(
                formatter,
                "campaign: identity profile not found — run 'symeraseme init-profile' first"
            ),
            Self::EmailSenderRequired => write!(
                formatter,
                "campaign: email_sender is required for email-based requests"
            ),
            Self::Database(error) => write!(formatter, "{error}"),
            Self::Projection(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ExecuteError {}

impl From<rusqlite::Error> for ExecuteError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<ProjectionError> for ExecuteError {
    fn from(error: ProjectionError) -> Self {
        Self::Projection(error)
    }
}

/// Go's `_BATCH_LIMIT`.
pub const BATCH_LIMIT: i64 = 10;

/// Go's `ExecuteCampaign`: the PLANNED requests of a campaign, clamped to the
/// batch limit, executed one by one. A failing request is captured as an
/// unsuccessful result and never aborts the batch.
pub fn execute_campaign(
    store: &Store,
    campaign_id: &str,
    opts: &ExecuteOpts<'_>,
    profile: ProfileSource<'_>,
    batch_size: i64,
    now: DateTime<Utc>,
) -> Result<Map<String, Value>, ExecuteError> {
    let batch_size = match batch_size {
        size if size > BATCH_LIMIT => BATCH_LIMIT,
        size if size <= 0 => 5,
        size => size,
    };
    let repository = Repository::new(store);
    let total_planned = repository.count_removal_requests(Some(campaign_id), Some("PLANNED"))?;
    let batch = repository.list_removal_requests(ListRemovalRequestsOptions {
        campaign_id: Some(campaign_id.to_owned()),
        status: Some("PLANNED".to_owned()),
        limit: Some(batch_size),
        ..ListRemovalRequestsOptions::default()
    })?;

    // Go builds this with `make`, so an empty batch still marshals as `[]`.
    let mut results: Vec<Value> = Vec::with_capacity(batch.len());
    for request in &batch {
        let result = match execute_request(store, request.id, opts, profile, now) {
            Ok(result) => result,
            Err(error) => {
                let mut failure = Map::new();
                failure.insert("success".to_owned(), json!(false));
                failure.insert("error".to_owned(), json!(error.to_string()));
                failure.insert("request_id".to_owned(), json!(request.id));
                failure
            }
        };
        results.push(Value::Object(result));
    }

    let mut out = Map::new();
    out.insert("campaign_id".to_owned(), json!(campaign_id));
    out.insert("total_planned".to_owned(), json!(total_planned));
    out.insert("batch_size".to_owned(), json!(batch.len()));
    out.insert("results".to_owned(), Value::Array(results));
    // Go returns `map[string]any` at every level of this payload
    // (`ExecuteCampaign`, `ExecuteRequest` and the captured failure), so the
    // keys marshal sorted; without this `preserve_order` would keep the
    // insertion order above.
    let Value::Object(ordered) = go_map_order(Value::Object(out)) else {
        unreachable!("go_map_order keeps objects as objects")
    };
    Ok(ordered)
}

/// Go's `ExecuteRequest`: dispatch on the request's channel.
pub fn execute_request(
    store: &Store,
    request_id: i64,
    opts: &ExecuteOpts<'_>,
    profile: ProfileSource<'_>,
    now: DateTime<Utc>,
) -> Result<ExecuteResult, ExecuteError> {
    let repository = Repository::new(store);
    let Some(request) = repository.get_removal_request(request_id)? else {
        return Err(ExecuteError::RequestNotFound(request_id));
    };
    // Go reads `req["broker_id"]` into `brokerName` and passes it on as both.
    let broker_name = request.broker_id;
    let channel = if request.channel.is_empty() {
        "email"
    } else {
        request.channel.as_str()
    };
    if channel == "web_form" {
        return execute_web_form_request(store, request_id, &broker_name, opts, profile, now);
    }
    let events = repository.get_events(request_id, 0)?;
    let payload = events
        .last()
        .map(|event| event.payload.clone())
        .unwrap_or_default();
    execute_email_request(
        request_id,
        &broker_name,
        &payload,
        &request.template_id,
        opts,
        profile,
    )
}

fn execute_email_request(
    request_id: i64,
    broker_name: &str,
    payload: &Map<String, Value>,
    template_id: &str,
    opts: &ExecuteOpts<'_>,
    profile: ProfileSource<'_>,
) -> Result<ExecuteResult, ExecuteError> {
    let endpoint = payload
        .get("endpoint")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // Unlike planning, a missing profile is a hard error on the email path.
    let profile = profile
        .map_err(|message| ExecuteError::IdentityProfile(message.to_owned()))?
        .ok_or(ExecuteError::IdentityProfileNotFound)?;

    let required_fields = match payload.get("required_fields").and_then(Value::as_array) {
        Some(fields) => fields
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        None => vec!["full_name".to_owned(), "email_addresses".to_owned()],
    };
    let missing = missing_fields(profile, &required_fields);
    if !missing.is_empty() {
        return Err(ExecuteError::IdentityProfile(format!(
            "Missing required identity fields: {}. Run 'symeraseme init-profile' to update your profile.",
            missing.join(", ")
        )));
    }

    let rendered = default_renderer(template_id, profile, broker_name);
    let subject = format!("Data Deletion Request — {broker_name}");
    if opts.dry_run {
        let mut out = Map::new();
        out.insert("success".to_owned(), json!(true));
        out.insert("dry_run".to_owned(), json!(true));
        out.insert("request_id".to_owned(), json!(request_id));
        out.insert("to".to_owned(), json!(endpoint));
        out.insert("subject".to_owned(), json!(subject));
        out.insert("body".to_owned(), json!(rendered));
        return Ok(out);
    }
    Err(ExecuteError::EmailSenderRequired)
}

/// Go's `defaultRenderer`: a deterministic placeholder body. `realPlanCommand`
/// never injects `ExecuteOpts.Render`, so this is the only renderer the CLI
/// reaches.
fn default_renderer(template_id: &str, profile: &Profile, broker_name: &str) -> String {
    format!(
        "[template {template_id} — {broker_name} / {}]",
        profile.full_name
    )
}

/// Go's `missingFields`/`profileFieldEmpty`.
fn missing_fields(profile: &Profile, fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .filter(|field| profile_field_empty(profile, field))
        .cloned()
        .collect()
}

fn profile_field_empty(profile: &Profile, field: &str) -> bool {
    match field {
        "full_name" => profile.full_name.is_empty(),
        "email_addresses" => profile.email_addresses.is_empty(),
        // Go treats any address as satisfying both `address` and `state`.
        "address" | "state" => profile.addresses.is_empty(),
        "date_of_birth" => profile
            .date_of_birth
            .as_ref()
            .is_none_or(|value| value.is_empty()),
        "name_variants" => profile.name_variants.is_empty(),
        "phone_numbers" => profile.phone_numbers.is_empty(),
        "jurisdictions" => profile.jurisdictions.is_empty(),
        _ => true,
    }
}

/// Go's `WebFormAdapter.Run` for the configuration `realPlanCommand` builds:
/// no `FormExecutor`, so a real run never submits a form. A dry run previews
/// the registry form; a live run asks for a manual task, which
/// `execute_web_form_request` then creates (`DeferManualTask`).
///
/// Go builds a `FormSpec` before either branch, but the only parts of it those
/// branches read are the start URL and the step count — the filled field values
/// exist for the executor this port does not have.
fn web_form_preview(
    brokers: &[Broker],
    broker_id: &str,
    dry_run: bool,
) -> Result<Map<String, Value>, Map<String, Value>> {
    let invalid = |message: String| {
        let mut result = Map::new();
        result.insert("success".to_owned(), json!(false));
        result.insert("code".to_owned(), json!("invalid_spec"));
        result.insert("error".to_owned(), json!(message));
        result.insert("reason".to_owned(), json!("generic_error"));
        result.insert("dry_run".to_owned(), json!(dry_run));
        result
    };
    let Some(broker) = brokers.iter().find(|broker| broker.id == broker_id) else {
        return Err(invalid(format!("campaign: broker {broker_id:?} not found")));
    };
    let web_form = broker.opt_out.iter().find_map(|channel| match channel {
        Channel::WebForm {
            url,
            form_spec,
            disabled,
            ..
        } if !disabled.unwrap_or(false) => Some((url, form_spec)),
        _ => None,
    });
    let Some((url, form_spec)) = web_form else {
        return Err(invalid(format!(
            "campaign: broker {broker_id:?} has no active web form channel"
        )));
    };

    let mut result = Map::new();
    if dry_run {
        result.insert("success".to_owned(), json!(true));
        result.insert("dry_run".to_owned(), json!(true));
        result.insert("broker_id".to_owned(), json!(broker.id));
        result.insert("broker_name".to_owned(), json!(broker.name));
        result.insert("url".to_owned(), json!(url));
        result.insert("steps".to_owned(), json!(form_spec.steps.len()));
        return Ok(result);
    }
    // `createManualTask` with `DeferManualTask` set.
    result.insert("success".to_owned(), json!(false));
    result.insert("status".to_owned(), json!("manual_action_required"));
    result.insert("reason".to_owned(), json!("dynamic_form"));
    result.insert("broker_id".to_owned(), json!(broker.id));
    result.insert("broker_name".to_owned(), json!(broker.name));
    result.insert("url".to_owned(), json!(url));
    result.insert("dry_run".to_owned(), json!(false));
    Ok(result)
}

/// Go's `sanitizeWebFormResult`: an allowlist, never a pass-through. Keys an
/// executor could add that this port cannot produce are dropped here exactly as
/// an unknown key is.
fn sanitize_web_form_result(
    raw: &Map<String, Value>,
    profile: Option<&Profile>,
) -> Map<String, Value> {
    let mut result = Map::new();
    for (key, value) in raw {
        match key.as_str() {
            "success" | "dry_run" => {
                if let Some(flag) = value.as_bool() {
                    result.insert(key.clone(), json!(flag));
                }
            }
            "task_id" | "step_index" | "total_steps" | "duration_ms" | "skipped_fields" => {
                result.insert(key.clone(), value.clone());
            }
            "code" | "reason" | "status" | "broker_id" | "broker_name" | "step"
            | "failed_field" => {
                result.insert(key.clone(), json!(bounded_redacted(value, profile, 200)));
            }
            "instructions" | "error" | "hint" => {
                result.insert(key.clone(), json!(bounded_redacted(value, profile, 500)));
            }
            "url" | "final_url" => {
                result.insert(key.clone(), json!(bounded_redacted(value, profile, 2048)));
            }
            _ => continue,
        }
    }
    result
}

/// Go's `boundedRedacted`: profile values removed, then cut to `limit` runes.
fn bounded_redacted(value: &Value, profile: Option<&Profile>, limit: usize) -> String {
    let text = value.as_str().unwrap_or_default();
    let redacted = crate::manualtasks::redact_identity_values(text, profile);
    match redacted.char_indices().nth(limit) {
        Some((index, _)) => redacted[..index].to_owned(),
        None => redacted,
    }
}

/// Go's `reasonForCode`.
fn reason_for_code(value: Option<&Value>) -> &'static str {
    match value.and_then(Value::as_str).unwrap_or_default() {
        "blocked_captcha" => "captcha_failed",
        "navigation_timeout" => "timeout",
        "field_not_found" => "unknown_field",
        "confirmation_failed" => "assertion_failed",
        _ => "generic_error",
    }
}

/// Go's `executeWebformRequest` for a configured runner: run the adapter,
/// sanitize its answer and record the SENT / SEND_FAILED outcome.
fn execute_web_form_request(
    store: &Store,
    request_id: i64,
    broker_name: &str,
    opts: &ExecuteOpts<'_>,
    profile: ProfileSource<'_>,
    now: DateTime<Utc>,
) -> Result<ExecuteResult, ExecuteError> {
    // A missing profile is valid here and yields an empty snapshot hash.
    let profile = profile.map_err(|message| ExecuteError::IdentityProfile(message.to_owned()))?;
    let identity_hash = profile
        .map(crate::identity::hash_profile)
        .unwrap_or_default();

    let raw = match web_form_preview(opts.brokers, broker_name, opts.dry_run) {
        Ok(raw) | Err(raw) => raw,
    };
    let mut result = sanitize_web_form_result(&raw, profile);

    let succeeded = result
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut payload = Map::new();
    if succeeded {
        payload.insert("broker_name".to_owned(), json!(broker_name));
        payload.insert("form_url".to_owned(), field(&result, "url"));
        payload.insert("expected_response_days".to_owned(), json!(30));
        payload.insert("identity_snapshot_hash".to_owned(), json!(identity_hash));
        payload.insert("formflow_code".to_owned(), field(&result, "code"));
        payload.insert("evidence".to_owned(), field(&result, "evidence"));
        store.append_and_project(request_id, &EventType::Sent, &payload, &Source::System, now)?;
    } else {
        let reason = match result.get("reason").and_then(Value::as_str) {
            Some(reason) if !reason.is_empty() => reason.to_owned(),
            _ => reason_for_code(result.get("code")).to_owned(),
        };
        payload.insert("error".to_owned(), field(&result, "error"));
        payload.insert("broker_name".to_owned(), json!(broker_name));
        payload.insert("formflow_code".to_owned(), field(&result, "code"));
        payload.insert("reason".to_owned(), json!(reason.clone()));
        payload.insert("evidence".to_owned(), field(&result, "evidence"));
        payload.insert(
            "form_url".to_owned(),
            match result.get("final_url") {
                Some(final_url) if !final_url.is_null() => final_url.clone(),
                _ => field(&result, "url"),
            },
        );
        if result.get("task_id").is_none_or(Value::is_null) {
            // Go creates the task before SEND_FAILED so HUMAN_ACTION_REQUIRED
            // projects first and SEND_FAILED becomes the final status.
            let form_url = match result.get("url").and_then(Value::as_str) {
                Some(url) if !url.is_empty() => url.to_owned(),
                _ => result
                    .get("final_url")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            };
            let task = crate::manualtasks::create(
                store,
                &crate::manualtasks::CreateOpts {
                    request_id: Some(request_id),
                    broker_id: broker_name.to_owned(),
                    broker_name: broker_name.to_owned(),
                    form_url,
                    reason,
                    error_message: raw
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    ..crate::manualtasks::CreateOpts::default()
                },
                profile,
                now,
            )?;
            payload.insert("task_id".to_owned(), json!(task.id));
            result.insert("task_id".to_owned(), json!(task.id));
        } else {
            payload.insert("task_id".to_owned(), field(&result, "task_id"));
        }
        store.append_and_project(
            request_id,
            &EventType::SendFailed,
            &payload,
            &Source::System,
            now,
        )?;
    }

    let mut out = Map::new();
    out.insert("success".to_owned(), field(&result, "success"));
    out.insert("request_id".to_owned(), json!(request_id));
    for (key, value) in result {
        out.insert(key, value);
    }
    Ok(out)
}

/// Go reads an absent map key as a nil `any`, which marshals as `null`.
fn field(map: &Map<String, Value>, key: &str) -> Value {
    map.get(key).cloned().unwrap_or(Value::Null)
}
