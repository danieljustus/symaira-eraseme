//! LLM-injected reply classification and rebuttal service orchestration.
//!
//! Provider transports stay outside EraseMe. Callers pass a classifier closure
//! when an LLM is available; `None` exercises the deterministic fallback.

use crate::identity::Profile;
use crate::llm::UsageRecord;
use crate::redaction::{Address as RedactionAddress, RedactionProfile};
use crate::storage::{EventType, Source, Store};
use crate::templating::{self, Address, FrozenDateTime, RenderContext};
use crate::triage_contract::{
    ClassificationResult, RejectionClassification, SummaryBytes, parse_classification_response,
    parse_rejection_classification, select_fallback_template,
};
use crate::triage_prompts::{build_rebuttal_classifier_prompt, build_user_prompt};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CLASSIFIER_SYSTEM_PROMPT: &str = "You are a precise email classifier for a data broker removal tool.\n\nYour task is to classify incoming broker reply emails into one of these categories:\n\n- ack: Broker acknowledges receipt. They are processing the request.\n- confirmed: Broker confirms the data has been deleted or account closed.\n- rejected: Broker explicitly rejects the request.\n- verification: Broker asks for more information or identity verification.\n- human_required: The reply requires manual human review.\n- autoresponder: Automated out-of-office, delivery receipt, or receipt notice.\n- bounce: Hard bounce — email address does not exist or mailbox is full.\n- unclear: Cannot confidently classify into any of the above.\n\nRespond with ONLY a JSON object on a single line:\n{\"classification\": \"<label>\", \"confidence\": <0.0-1.0>, \"summary\": \"<text>\", \"extracted_fields\": {}}\nIf confidence < 0.4, use \"unclear\" as the fallback classification.";

pub const REBUTTAL_SYSTEM_PROMPT: &str = "You are a precise rejection classifier for a data broker removal tool.\n\nClassify the broker response as address_mismatch, identity_challenged,\nccpa_identity_challenged, or other. Respond with only JSON:\n{\"classification\":\"<label>\",\"confidence\":0.0,\"summary\":\"<text>\",\"key_points\":[],\"jurisdiction\":\"<GDPR|CCPA|unknown>\"}";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LlmResponse {
    pub text: String,
    pub usage: UsageRecord,
}

/// A supplied classifier call receives system prompt, user prompt, and cache key.
pub type LlmCall<'a> = dyn Fn(&str, &str, &str) -> Result<LlmResponse, String> + 'a;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassifyOptions {
    pub broker_name: String,
    pub broker_website: String,
    pub original_subject: String,
    pub original_snippet: String,
    pub reply_subject: String,
    pub reply_body: String,
    pub cache_key: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassifyOutcome {
    pub result: ClassificationResult,
    pub error: Option<String>,
    pub usage: UsageRecord,
}

pub fn classify_reply(
    options: &ClassifyOptions,
    profile: Option<&RedactionProfile>,
    llm: Option<&LlmCall<'_>>,
) -> ClassifyOutcome {
    let Some(llm) = llm else {
        let mut result = ClassificationResult::default();
        result.summary = SummaryBytes::from_text("Classifier not initialized");
        return ClassifyOutcome {
            result,
            error: None,
            usage: UsageRecord::default(),
        };
    };
    let prompt = build_user_prompt(
        &options.broker_name,
        &options.broker_website,
        &options.original_subject,
        &options.original_snippet,
        &options.reply_subject,
        &options.reply_body,
        profile,
    );
    match llm(
        CLASSIFIER_SYSTEM_PROMPT,
        &prompt.wire_string(),
        &options.cache_key,
    ) {
        Ok(response) => ClassifyOutcome {
            result: parse_classification_response(&response.text),
            error: None,
            usage: response.usage,
        },
        Err(error) => {
            let mut result = ClassificationResult::default();
            result.summary = SummaryBytes::from_text(&format!("API error: {error}"));
            ClassifyOutcome {
                result,
                error: Some(error),
                usage: UsageRecord::default(),
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RebuttalOptions {
    pub broker_name: String,
    pub broker_website: String,
    pub broker_message: String,
    pub original_request_template: String,
    pub original_request_date: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RebuttalResult {
    pub template_name: String,
    pub label: String,
    pub description: String,
    pub jurisdiction: String,
    pub rejection_classification: String,
    pub confidence: f64,
    pub rebuttal_body: String,
    pub needs_human_review: bool,
    pub llm_used: bool,
    pub usage: UsageRecord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebuttalError(String);

impl std::fmt::Display for RebuttalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RebuttalError {}

pub fn generate_rebuttal(
    options: &RebuttalOptions,
    profile: Option<&Profile>,
    llm: Option<&LlmCall<'_>>,
) -> Result<RebuttalResult, RebuttalError> {
    let redaction = profile.map(redaction_profile);
    let mut classification = RejectionClassification::default();
    let mut llm_used = false;
    let mut usage = UsageRecord::default();
    if let Some(llm) = llm {
        let prompt = build_rebuttal_classifier_prompt(
            &options.broker_name,
            &options.broker_message,
            &options.original_request_template,
            redaction.as_ref(),
        );
        if let Ok(response) = llm(
            REBUTTAL_SYSTEM_PROMPT,
            &prompt.wire_string(),
            &format!("rebuttal:{}", options.broker_name),
        ) {
            classification = parse_rejection_classification(&response.text);
            usage = response.usage;
            llm_used = true;
        }
    }

    let mut key = classification.classification.as_str();
    if !llm_used || key == "other" {
        key = select_fallback_template(&options.broker_message);
    }
    if key.is_empty() {
        key = "identity_challenged";
    }
    let (mut name, mut label, mut description, mut jurisdiction) = rejection_template(key);
    let available = templating::list_template_names();
    if !available.contains(&name) {
        if let Some(candidate) = available
            .iter()
            .find(|candidate| candidate.starts_with(&jurisdiction.to_lowercase()))
        {
            name = candidate;
        }
    }
    if !available.contains(&name) {
        name = "gdpr-art17.en.md.j2";
        label = "GDPR Erasure Request (Fallback)";
        description = "Fallback template when rebuttal template not found";
        jurisdiction = "GDPR";
    }
    let mut extra = std::collections::BTreeMap::new();
    extra.insert(
        "original_request_date".to_owned(),
        Value::String(options.original_request_date.clone()),
    );
    let now = FrozenDateTime::from_rfc3339(system_now().to_rfc3339())
        .map_err(|error| RebuttalError(error.to_string()))?;
    let context = render_context(profile, options, extra, now);
    let body =
        templating::render(name, &context).map_err(|error| RebuttalError(error.to_string()))?;
    let needs_human_review =
        !llm_used || classification.classification == "other" || classification.confidence < 0.5;
    Ok(RebuttalResult {
        template_name: name.to_owned(),
        label: label.to_owned(),
        description: description.to_owned(),
        jurisdiction: jurisdiction.to_owned(),
        rejection_classification: if llm_used {
            classification.classification
        } else {
            "fallback".to_owned()
        },
        confidence: classification.confidence,
        rebuttal_body: body,
        needs_human_review,
        llm_used,
        usage,
    })
}

/// Reply-row and event orchestration equivalent to `replies.Service`.
pub struct Service<'store> {
    store: &'store Store,
}

impl<'store> Service<'store> {
    pub fn new(store: &'store Store) -> Self {
        Self { store }
    }

    pub fn classify_reply(
        &self,
        request_id: i64,
        options: &ClassifyRequest,
        profile: Option<&RedactionProfile>,
        llm: Option<&LlmCall<'_>>,
        save: bool,
    ) -> Result<ClassifyOutcome, String> {
        let reply = latest_reply(self.store, request_id, true)?.ok_or_else(|| {
            format!("no unclassified inbox reply found for request #{request_id}")
        })?;
        let options = ClassifyOptions {
            broker_name: options.broker_name.clone(),
            broker_website: options.broker_website.clone(),
            original_subject: options.original_subject.clone(),
            original_snippet: options.original_snippet.clone(),
            reply_subject: reply.subject,
            reply_body: reply.snippet,
            cache_key: format!("broker:{}", options.broker_name),
        };
        let outcome = classify_reply(&options, profile, llm);
        if save && outcome.error.is_none() {
            self.store.connection().execute(
                "UPDATE inbox_replies SET classified_as = ?1, classifier_confidence = ?2, llm_summary = ?3 WHERE id = ?4",
                params![outcome.result.classification, outcome.result.confidence, outcome.result.summary.wire_string(), reply.id],
            ).map_err(|error| error.to_string())?;
            let payload = json!({
                "classification": outcome.result.classification,
                "confidence": outcome.result.confidence,
                "summary": outcome.result.summary.wire_string(),
                "extracted_fields": outcome.result.extracted_fields,
                "reply_id": reply.id,
            });
            append_event(self.store, request_id, &outcome.result.event_type, payload)?;
        }
        Ok(outcome)
    }

    pub fn generate_rebuttal(
        &self,
        request_id: i64,
        options: &RebuttalRequest,
        profile: Option<&Profile>,
        llm: Option<&LlmCall<'_>>,
        save: bool,
    ) -> Result<RebuttalResult, String> {
        let reply = latest_reply(self.store, request_id, false)?;
        let message = reply
            .as_ref()
            .map(|row| row.snippet.as_str())
            .filter(|message| !message.is_empty())
            .unwrap_or(&options.original_request_template);
        let result = generate_rebuttal(
            &RebuttalOptions {
                broker_name: options.broker_name.clone(),
                broker_website: options.broker_website.clone(),
                broker_message: message.to_owned(),
                original_request_template: options.original_request_template.clone(),
                original_request_date: options.original_request_date.clone(),
            },
            profile,
            llm,
        )
        .map_err(|error| error.to_string())?;
        if save {
            append_event(
                self.store,
                request_id,
                "REBUTTAL_SENT",
                json!({
                    "template_name": result.template_name,
                    "rejection_classification": result.rejection_classification,
                    "confidence": result.confidence,
                    "llm_used": result.llm_used,
                    "broker_message_snippet": truncate_bytes(message, 200),
                }),
            )?;
        }
        Ok(result)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassifyRequest {
    pub broker_name: String,
    pub broker_website: String,
    pub original_subject: String,
    pub original_snippet: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RebuttalRequest {
    pub broker_name: String,
    pub broker_website: String,
    pub original_request_template: String,
    pub original_request_date: String,
}

#[derive(Clone)]
struct ReplyRow {
    id: i64,
    subject: String,
    snippet: String,
}

fn latest_reply(
    store: &Store,
    request_id: i64,
    unclassified: bool,
) -> Result<Option<ReplyRow>, String> {
    let query = if unclassified {
        "SELECT id, subject, snippet FROM inbox_replies WHERE request_id = ?1 AND classified_as IS NULL ORDER BY received_at DESC, id DESC LIMIT 1"
    } else {
        "SELECT id, subject, snippet FROM inbox_replies WHERE request_id = ?1 ORDER BY received_at DESC, id DESC LIMIT 1"
    };
    store
        .connection()
        .query_row(query, [request_id], |row| {
            Ok(ReplyRow {
                id: row.get(0)?,
                subject: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                snippet: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            })
        })
        .optional()
        .map_err(|error| error.to_string())
}

fn append_event(
    store: &Store,
    request_id: i64,
    event_type: &str,
    payload: Value,
) -> Result<(), String> {
    let event_type = EventType::from_wire(event_type.to_owned());
    let payload = payload.as_object().cloned().unwrap_or_default();
    store
        .append_and_project(
            request_id,
            &event_type,
            &payload,
            &Source::System,
            system_now(),
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn truncate_bytes(value: &str, limit: usize) -> String {
    let bytes = &value.as_bytes()[..value.len().min(limit)];
    String::from_utf8_lossy(bytes).into_owned()
}

fn system_now() -> chrono::DateTime<chrono::Utc> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    chrono::DateTime::<chrono::Utc>::from_timestamp(
        elapsed.as_secs() as i64,
        elapsed.subsec_nanos(),
    )
    .unwrap_or_default()
}

fn rejection_template(key: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match key {
        "address_mismatch" => (
            "gdpr-rebuttal-address.md.j2",
            "Address Discrepancy Rebuttal (GDPR)",
            "Broker rejected request citing old address on file",
            "GDPR",
        ),
        "identity_challenged" => (
            "gdpr-rebuttal-identity.md.j2",
            "Identity Verification Rebuttal (GDPR)",
            "Broker challenged identity verification",
            "GDPR",
        ),
        "ccpa_identity_challenged" => (
            "ccpa-rebuttal-deletion.md.j2",
            "Identity Verification Rebuttal (CCPA)",
            "Broker requested additional identity info under CCPA",
            "CCPA",
        ),
        _ => ("", "", "", "GDPR"),
    }
}

fn redaction_profile(profile: &Profile) -> RedactionProfile {
    RedactionProfile {
        full_name: profile.full_name.clone(),
        name_variants: profile.name_variants.clone(),
        email_addresses: profile.email_addresses.clone(),
        phone_numbers: profile.phone_numbers.clone(),
        addresses: profile
            .addresses
            .iter()
            .map(|address| RedactionAddress {
                street: address.street.clone(),
                city: address.city.clone(),
                postal_code: address.postal_code.clone(),
            })
            .collect(),
    }
}

fn render_context(
    profile: Option<&Profile>,
    options: &RebuttalOptions,
    extra: std::collections::BTreeMap<String, Value>,
    now: FrozenDateTime,
) -> RenderContext {
    let mut context = RenderContext {
        broker_name: options.broker_name.clone(),
        broker_website: options.broker_website.clone(),
        extra,
        now,
        ..RenderContext::default()
    };
    if let Some(profile) = profile {
        context.full_name = profile.full_name.clone();
        context.name_variants = profile.name_variants.clone();
        context.date_of_birth = profile.date_of_birth.clone();
        context.addresses = profile
            .addresses
            .iter()
            .map(|address| Address {
                street: address.street.clone(),
                city: address.city.clone(),
                postal_code: address.postal_code.clone(),
                country: address.country.clone(),
            })
            .collect();
        context.email_addresses = profile.email_addresses.clone();
        context.phone_numbers = profile.phone_numbers.clone();
        context.jurisdictions = profile.jurisdictions.clone();
    }
    context
}
