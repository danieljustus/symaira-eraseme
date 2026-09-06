use serde::{Deserialize, Serialize};
use std::fmt;

fn default_data_sensitivity() -> u8 {
    3
}

fn default_status() -> Status {
    Status::Active
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
    #[serde(default)]
    pub verification: Option<Verification>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub added_date: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default = "default_status")]
    pub status: Status,
    #[serde(default)]
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
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum Channel {
    #[serde(rename = "email")]
    Email {
        endpoint: String,
        #[serde(default)]
        template: Option<Template>,
        #[serde(default)]
        locale: Option<String>,
        #[serde(default)]
        required_fields: Option<Vec<RequiredField>>,
        #[serde(default)]
        supports_suppression: Option<bool>,
        #[serde(default)]
        expected_response_days: Option<u32>,
        #[serde(default)]
        disabled: Option<bool>,
    },
    #[serde(rename = "web_form")]
    WebForm {
        url: String,
        form_spec: FormSpec,
        #[serde(default)]
        template: Option<Template>,
        #[serde(default)]
        locale: Option<String>,
        #[serde(default)]
        required_fields: Option<Vec<RequiredField>>,
        #[serde(default)]
        supports_suppression: Option<bool>,
        #[serde(default)]
        expected_response_days: Option<u32>,
        #[serde(default)]
        disabled: Option<bool>,
    },
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
    #[serde(default)]
    pub ack_keywords: Option<Vec<String>>,
    #[serde(default)]
    pub rejection_keywords: Option<Vec<String>>,
    #[serde(default)]
    pub human_required_keywords: Option<Vec<String>>,
}

/// Declarative web-form specification.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct FormSpec {
    pub steps: Vec<FormStep>,
    #[serde(default)]
    pub timeout_seconds: Option<f64>,
    #[serde(default)]
    pub rate_limit_delay: Option<f64>,
    #[serde(default)]
    pub headless: Option<bool>,
}

/// One web-form action step.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct FormStep {
    #[serde(rename = "goto", default)]
    pub goto: Option<String>,
    #[serde(default)]
    pub fill: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub select: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub click: Option<String>,
    #[serde(default)]
    pub wait_for: Option<String>,
    #[serde(default)]
    pub wait_seconds: Option<f64>,
    #[serde(default)]
    pub screenshot: Option<String>,
    #[serde(default)]
    pub assert_text: Option<String>,
    #[serde(default)]
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
    #[serde(default)]
    pub provider: Option<CaptchaProvider>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub min_score: Option<f64>,
    #[serde(default)]
    pub is_invisible: Option<bool>,
}
