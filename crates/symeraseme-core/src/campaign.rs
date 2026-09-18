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

use crate::registry::{Broker, BrokerFilter, Channel, RegistryError, Template, filter_brokers};
use crate::storage::Store;
use crate::storage::projection::ProjectionError;
use crate::storage::repository::Repository;
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
    pub requests: Vec<PlanRequest>,
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
