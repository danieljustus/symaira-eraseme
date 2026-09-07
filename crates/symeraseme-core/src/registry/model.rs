use serde::{Deserialize, Serialize};
use std::fmt;

fn default_data_sensitivity() -> u8 {
    3
}

fn default_status() -> Status {
    Status::Active
}

fn reject_null_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    match Option::<T>::deserialize(deserializer)? {
        Some(value) => Ok(Some(value)),
        None => Err(serde::de::Error::custom(
            "explicit YAML null is not allowed",
        )),
    }
}

/// A registry broker document from schema version 1.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Broker {
    pub id: String,
    pub name: String,
    pub website: String,
    pub category: Category,
    pub jurisdictions: Vec<Jurisdiction>,
    pub laws: Vec<Law>,
    #[serde(default = "default_data_sensitivity")]
    pub data_sensitivity: u8,
    pub priority: Priority,
    pub opt_out: Vec<Channel>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub verification: Option<Verification>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub disabled: Option<bool>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub added_date: Option<String>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub source: Option<String>,
    #[serde(default = "default_status")]
    pub status: Status,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub notes: Option<String>,
}

impl Broker {
    /// Decodes and strictly validates one YAML broker document.
    pub fn from_yaml(
        file_stem: &str,
        source: &str,
    ) -> Result<Self, crate::registry::RegistryError> {
        super::validate::decode(file_stem, source)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct BrokerWire {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) website: String,
    pub(crate) category: Category,
    pub(crate) jurisdictions: Vec<Jurisdiction>,
    pub(crate) laws: Vec<Law>,
    #[serde(default = "default_data_sensitivity")]
    pub(crate) data_sensitivity: u8,
    pub(crate) priority: Priority,
    pub(crate) opt_out: Vec<ChannelWire>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) verification: Option<VerificationWire>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) disabled: Option<bool>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) added_date: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) source: Option<String>,
    #[serde(default = "default_status")]
    pub(crate) status: Status,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) notes: Option<String>,
}

impl BrokerWire {
    pub(crate) fn into_model(self) -> Result<Broker, &'static str> {
        let opt_out = self
            .opt_out
            .into_iter()
            .map(ChannelWire::into_model)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Broker {
            id: self.id,
            name: self.name,
            website: self.website,
            category: self.category,
            jurisdictions: self.jurisdictions,
            laws: self.laws,
            data_sensitivity: self.data_sensitivity,
            priority: self.priority,
            opt_out,
            verification: self.verification.map(Verification::from),
            disabled: self.disabled,
            added_date: self.added_date,
            source: self.source,
            status: self.status,
            notes: self.notes,
        })
    }
}

/// Business category enum from the registry contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    #[serde(rename = "people-search")]
    PeopleSearch,
    Marketing,
    Credit,
    Analytics,
    #[serde(rename = "background-check")]
    BackgroundCheck,
    #[serde(rename = "social-media")]
    SocialMedia,
    Other,
}

/// Jurisdiction enum from the registry contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Jurisdiction {
    At,
    Ch,
    De,
    Dk,
    Eu,
    Fi,
    Fr,
    Gb,
    Ie,
    Il,
    Lu,
    Nl,
    No,
    Se,
    Uk,
    Us,
}

/// Privacy-law enum from the registry contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Law {
    Gdpr,
    Ccpa,
    Cpra,
    Lgpd,
    Pipeda,
}

/// Removal priority enum from the registry contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    Medium,
    Low,
}

/// Operational broker status enum from the registry contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    Deprecated,
    Merged,
    #[serde(rename = "out-of-business")]
    OutOfBusiness,
}

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Active => "active",
            Self::Deprecated => "deprecated",
            Self::Merged => "merged",
            Self::OutOfBusiness => "out-of-business",
        };
        formatter.write_str(value)
    }
}

/// Channel type enum, useful to callers inspecting a channel without matching
/// its full variant.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Email,
    #[serde(rename = "web_form")]
    WebForm,
}

/// An opt-out channel. The internally tagged representation makes the
/// `type` discriminator part of the wire contract while keeping email and
/// web-form requirements distinct in the Rust model.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum Channel {
    #[serde(rename = "email")]
    Email {
        endpoint: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template: Option<Template>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        locale: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required_fields: Option<Vec<RequiredField>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        supports_suppression: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_response_days: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disabled: Option<bool>,
    },
    #[serde(rename = "web_form")]
    WebForm {
        url: String,
        form_spec: FormSpec,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template: Option<Template>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        locale: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required_fields: Option<Vec<RequiredField>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        supports_suppression: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_response_days: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disabled: Option<bool>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct ChannelWire {
    #[serde(rename = "type")]
    channel_type: ChannelType,
    #[serde(default, deserialize_with = "reject_null_option")]
    endpoint: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    url: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    form_spec: Option<FormSpecWire>,
    #[serde(default, deserialize_with = "reject_null_option")]
    template: Option<Template>,
    #[serde(default, deserialize_with = "reject_null_option")]
    locale: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    required_fields: Option<Vec<RequiredField>>,
    #[serde(default, deserialize_with = "reject_null_option")]
    supports_suppression: Option<bool>,
    #[serde(default, deserialize_with = "reject_null_option")]
    expected_response_days: Option<u32>,
    #[serde(default, deserialize_with = "reject_null_option")]
    disabled: Option<bool>,
}

impl ChannelWire {
    fn into_model(self) -> Result<Channel, &'static str> {
        match self.channel_type {
            ChannelType::Email => {
                if self.endpoint.is_none() {
                    return Err("email channel requires endpoint");
                }
                if self.url.is_some() || self.form_spec.is_some() {
                    return Err("email channel must not carry web_form fields");
                }
                Ok(Channel::Email {
                    endpoint: self.endpoint.expect("checked above"),
                    template: self.template,
                    locale: self.locale,
                    required_fields: self.required_fields,
                    supports_suppression: self.supports_suppression,
                    expected_response_days: self.expected_response_days,
                    disabled: self.disabled,
                })
            }
            ChannelType::WebForm => {
                if self.url.is_none() || self.form_spec.is_none() {
                    return Err("web_form channel requires url and form_spec");
                }
                if self.endpoint.is_some() {
                    return Err("web_form channel must not carry email fields");
                }
                Ok(Channel::WebForm {
                    url: self.url.expect("checked above"),
                    form_spec: self.form_spec.expect("checked above").into(),
                    template: self.template,
                    locale: self.locale,
                    required_fields: self.required_fields,
                    supports_suppression: self.supports_suppression,
                    expected_response_days: self.expected_response_days,
                    disabled: self.disabled,
                })
            }
        }
    }
}

impl Channel {
    pub fn channel_type(&self) -> ChannelType {
        match self {
            Self::Email { .. } => ChannelType::Email,
            Self::WebForm { .. } => ChannelType::WebForm,
        }
    }
}

/// Letter-template enum from the registry contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Template {
    #[serde(rename = "ccpa-deletion")]
    CcpaDeletion,
    #[serde(rename = "gdpr-art17")]
    GdprArt17,
}

/// Identity fields a broker may require.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredField {
    #[serde(rename = "full_name")]
    FullName,
    Email,
    Address,
    #[serde(rename = "date_of_birth")]
    DateOfBirth,
    State,
}

/// Keyword sets used to classify broker replies.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Verification {
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub ack_keywords: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub rejection_keywords: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub human_required_keywords: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct VerificationWire {
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) ack_keywords: Option<Vec<String>>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) rejection_keywords: Option<Vec<String>>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) human_required_keywords: Option<Vec<String>>,
}

impl From<VerificationWire> for Verification {
    fn from(wire: VerificationWire) -> Self {
        Self {
            ack_keywords: wire.ack_keywords,
            rejection_keywords: wire.rejection_keywords,
            human_required_keywords: wire.human_required_keywords,
        }
    }
}

/// Declarative web-form specification.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FormSpec {
    pub steps: Vec<FormStep>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout_seconds: Option<f64>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub rate_limit_delay: Option<f64>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub headless: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct FormSpecWire {
    pub(crate) steps: Vec<FormStepWire>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) timeout_seconds: Option<f64>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) rate_limit_delay: Option<f64>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) headless: Option<bool>,
}

impl From<FormSpecWire> for FormSpec {
    fn from(wire: FormSpecWire) -> Self {
        Self {
            steps: wire.steps.into_iter().map(FormStep::from).collect(),
            timeout_seconds: wire.timeout_seconds,
            rate_limit_delay: wire.rate_limit_delay,
            headless: wire.headless,
        }
    }
}

/// One web-form action step.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FormStep {
    #[serde(
        rename = "goto",
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub goto: Option<String>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub fill: Option<std::collections::BTreeMap<String, String>>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub select: Option<std::collections::BTreeMap<String, String>>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub click: Option<String>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub wait_for: Option<String>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub wait_seconds: Option<f64>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub screenshot: Option<String>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub assert_text: Option<String>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub solve_captcha: Option<SolveCaptcha>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct FormStepWire {
    #[serde(rename = "goto", default, deserialize_with = "reject_null_option")]
    pub(crate) goto: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) fill: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) select: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) click: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) wait_for: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) wait_seconds: Option<f64>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) screenshot: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) assert_text: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) solve_captcha: Option<SolveCaptchaWire>,
}

impl From<FormStepWire> for FormStep {
    fn from(wire: FormStepWire) -> Self {
        Self {
            goto: wire.goto,
            fill: wire.fill,
            select: wire.select,
            click: wire.click,
            wait_for: wire.wait_for,
            wait_seconds: wire.wait_seconds,
            screenshot: wire.screenshot,
            assert_text: wire.assert_text,
            solve_captcha: wire.solve_captcha.map(SolveCaptcha::from),
        }
    }
}

/// CAPTCHA type enum from the registry contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptchaType {
    #[serde(rename = "recaptcha-v2")]
    RecaptchaV2,
    #[serde(rename = "recaptcha-v3")]
    RecaptchaV3,
    Hcaptcha,
    Turnstile,
}

/// CAPTCHA provider enum from the registry contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptchaProvider {
    Capsolver,
    #[serde(rename = "2captcha")]
    TwoCaptcha,
    Anticaptcha,
}

/// CAPTCHA-solving action.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SolveCaptcha {
    #[serde(rename = "type")]
    pub captcha_type: CaptchaType,
    pub site_key: String,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub provider: Option<CaptchaProvider>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub action: Option<String>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub min_score: Option<f64>,
    #[serde(
        default,
        deserialize_with = "reject_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_invisible: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct SolveCaptchaWire {
    #[serde(rename = "type")]
    pub(crate) captcha_type: CaptchaType,
    pub(crate) site_key: String,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) provider: Option<CaptchaProvider>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) action: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) min_score: Option<f64>,
    #[serde(default, deserialize_with = "reject_null_option")]
    pub(crate) is_invisible: Option<bool>,
}

impl From<SolveCaptchaWire> for SolveCaptcha {
    fn from(wire: SolveCaptchaWire) -> Self {
        Self {
            captcha_type: wire.captcha_type,
            site_key: wire.site_key,
            provider: wire.provider,
            action: wire.action,
            min_score: wire.min_score,
            is_invisible: wire.is_invisible,
        }
    }
}
