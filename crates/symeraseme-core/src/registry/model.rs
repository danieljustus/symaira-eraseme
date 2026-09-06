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
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
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
struct ChannelWire {
    #[serde(rename = "type")]
    channel_type: ChannelType,
    #[serde(default, deserialize_with = "reject_null_option")]
    endpoint: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    url: Option<String>,
    #[serde(default, deserialize_with = "reject_null_option")]
    form_spec: Option<FormSpec>,
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

impl<'de> Deserialize<'de> for Channel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ChannelWire::deserialize(deserializer)?;
        match wire.channel_type {
            ChannelType::Email => {
                if wire.endpoint.is_none() {
                    return Err(serde::de::Error::missing_field("endpoint"));
                }
                if wire.url.is_some() || wire.form_spec.is_some() {
                    return Err(serde::de::Error::custom(
                        "email channel must not carry web_form fields",
                    ));
                }
                Ok(Self::Email {
                    endpoint: wire.endpoint.expect("checked above"),
                    template: wire.template,
                    locale: wire.locale,
                    required_fields: wire.required_fields,
                    supports_suppression: wire.supports_suppression,
                    expected_response_days: wire.expected_response_days,
                    disabled: wire.disabled,
                })
            }
            ChannelType::WebForm => {
                if wire.url.is_none() || wire.form_spec.is_none() {
                    return Err(serde::de::Error::missing_field("url/form_spec"));
                }
                if wire.endpoint.is_some() {
                    return Err(serde::de::Error::custom(
                        "web_form channel must not carry email fields",
                    ));
                }
                Ok(Self::WebForm {
                    url: wire.url.expect("checked above"),
                    form_spec: wire.form_spec.expect("checked above"),
                    template: wire.template,
                    locale: wire.locale,
                    required_fields: wire.required_fields,
                    supports_suppression: wire.supports_suppression,
                    expected_response_days: wire.expected_response_days,
                    disabled: wire.disabled,
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
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
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

/// Declarative web-form specification.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
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

/// One web-form action step.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
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
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
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
