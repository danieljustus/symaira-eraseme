//! Deterministic rendering of the canonical Jinja templates.
//!
//! The registry sources are embedded directly from the repository.  No runtime
//! filesystem access is used for template lookup: callers can render only one
//! of the eleven names in [`list_template_names`].

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use minijinja::value::{Object, Value};
use minijinja::{AutoEscape, Environment, Error, ErrorKind, State, UndefinedBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};

const MAX_NAME_BYTES: usize = 256;
const MAX_ERROR_BYTES: usize = 256;
const TEMPLATE_COUNT: usize = 11;

struct TemplateSpec {
    name: &'static str,
    source: &'static str,
    html: bool,
}

const TEMPLATES: &[TemplateSpec] = &[
    TemplateSpec {
        name: "ccpa-deletion.en.md.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/laws/ccpa-deletion.en.md.j2"
        )),
        html: false,
    },
    TemplateSpec {
        name: "ccpa-opt-out.en.md.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/laws/ccpa-opt-out.en.md.j2"
        )),
        html: false,
    },
    TemplateSpec {
        name: "ccpa-rebuttal-deletion.md.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/laws/ccpa-rebuttal-deletion.md.j2"
        )),
        html: false,
    },
    TemplateSpec {
        name: "gdpr-art17.de.md.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/laws/gdpr-art17.de.md.j2"
        )),
        html: false,
    },
    TemplateSpec {
        name: "gdpr-art17.en.md.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/laws/gdpr-art17.en.md.j2"
        )),
        html: false,
    },
    TemplateSpec {
        name: "gdpr-rebuttal-address.md.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/laws/gdpr-rebuttal-address.md.j2"
        )),
        html: false,
    },
    TemplateSpec {
        name: "gdpr-rebuttal-identity.md.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/laws/gdpr-rebuttal-identity.md.j2"
        )),
        html: false,
    },
    TemplateSpec {
        name: "gdpr-rebuttal-rejected.en.md.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/laws/gdpr-rebuttal-rejected.en.md.j2"
        )),
        html: false,
    },
    TemplateSpec {
        name: "gdpr-rebuttal-verification.en.md.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/laws/gdpr-rebuttal-verification.en.md.j2"
        )),
        html: false,
    },
    TemplateSpec {
        name: "dashboard.html.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/templates/dashboard.html.j2"
        )),
        html: true,
    },
    TemplateSpec {
        name: "report.html.j2",
        source: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../registry/templates/report.html.j2"
        )),
        html: true,
    },
];

/// A bounded, intentionally opaque rendering error.
///
/// The underlying template-engine error is never exposed because it can
/// contain source excerpts, rendered values, or other caller-provided data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemplateError {
    message: String,
}

impl TemplateError {
    fn new(message: &'static str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }

    fn named(message: &'static str, name: &str) -> Self {
        let mut rendered = String::with_capacity(message.len() + name.len() + 4);
        rendered.push_str(message);
        rendered.push_str(" '");
        rendered.push_str(name);
        rendered.push('\'');
        rendered.truncate(MAX_ERROR_BYTES);
        Self { message: rendered }
    }
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TemplateError {}

/// A serializable frozen timestamp supplied by the caller.
///
/// It is deliberately not obtained from the wall clock.  Templates currently
/// use only `strftime("%Y-%m-%d %H:%M UTC")`, which is rendered in UTC.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FrozenDateTime(String);

impl FrozenDateTime {
    /// Creates a frozen timestamp from an RFC 3339 value.
    pub fn from_rfc3339(value: impl Into<String>) -> Result<Self, TemplateError> {
        let value = value.into();
        DateTime::parse_from_rfc3339(&value)
            .map(|_| Self(value))
            .map_err(|_| TemplateError::new("invalid frozen timestamp"))
    }

    /// Returns the original RFC 3339 representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for FrozenDateTime {
    fn default() -> Self {
        Self("1970-01-01T00:00:00+00:00".to_owned())
    }
}

#[derive(Debug)]
struct FrozenDateTimeObject {
    value: DateTime<chrono::FixedOffset>,
}

impl Object for FrozenDateTimeObject {
    fn repr(self: &Arc<Self>) -> minijinja::value::ObjectRepr {
        minijinja::value::ObjectRepr::Plain
    }

    fn call_method(
        self: &Arc<Self>,
        _state: &mut State<'_, '_>,
        method: &str,
        args: &[Value],
    ) -> Result<Value, Error> {
        if method != "strftime" || args.len() != 1 {
            return Err(Error::new(
                ErrorKind::InvalidOperation,
                "unsupported datetime method",
            ));
        }
        let format: String = args[0].clone().into();
        if format != "%Y-%m-%d %H:%M UTC" {
            return Err(Error::new(
                ErrorKind::InvalidOperation,
                "unsupported datetime format",
            ));
        }
        Ok(Value::from(
            self.value
                .with_timezone(&Utc)
                .format("%Y-%m-%d %H:%M UTC")
                .to_string(),
        ))
    }
}

/// An address exposed as the flat profile field used by the canonical sources.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Address {
    pub street: String,
    pub city: String,
    pub postal_code: String,
    pub country: String,
}

/// The language-neutral, serde-serializable context accepted by [`render`].
///
/// `extra` is applied last, exactly as the Go renderer applies its extra
/// variables, so an extra variable may intentionally override any base field.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RenderContext {
    pub full_name: String,
    #[serde(default)]
    pub name_variants: Vec<String>,
    #[serde(default)]
    pub date_of_birth: Option<String>,
    #[serde(default)]
    pub addresses: Vec<Address>,
    #[serde(default)]
    pub email_addresses: Vec<String>,
    #[serde(default)]
    pub phone_numbers: Vec<String>,
    #[serde(default)]
    pub jurisdictions: Vec<String>,
    #[serde(default)]
    pub broker_name: String,
    #[serde(default)]
    pub broker_website: String,
    #[serde(default)]
    pub brokers: Vec<JsonValue>,
    #[serde(default)]
    pub data: JsonValue,
    #[serde(default)]
    pub now: FrozenDateTime,
    #[serde(default)]
    pub extra: BTreeMap<String, JsonValue>,
}

impl Default for RenderContext {
    fn default() -> Self {
        Self {
            full_name: String::new(),
            name_variants: Vec::new(),
            date_of_birth: None,
            addresses: Vec::new(),
            email_addresses: Vec::new(),
            phone_numbers: Vec::new(),
            jurisdictions: Vec::new(),
            broker_name: String::new(),
            broker_website: String::new(),
            brokers: Vec::new(),
            data: JsonValue::Null,
            now: FrozenDateTime::default(),
            extra: BTreeMap::new(),
        }
    }
}

/// Returns exactly the eleven canonical public names, in lexical order.
pub fn list_template_names() -> Vec<&'static str> {
    debug_assert_eq!(TEMPLATES.len(), TEMPLATE_COUNT);
    let mut names: Vec<_> = TEMPLATES.iter().map(|template| template.name).collect();
    names.sort_unstable();
    names
}

/// Alias retained for callers that use the Python/Go surface name.
pub fn list_templates() -> Vec<&'static str> {
    list_template_names()
}

/// Renders a canonical template using deterministic caller-provided data.
pub fn render(template_name: &str, context: &RenderContext) -> Result<String, TemplateError> {
    let name = canonical_name(template_name)?;
    let template = TEMPLATES
        .iter()
        .find(|template| template.name == name)
        .ok_or_else(|| TemplateError::new("unknown template"))?;
    if !context.data.is_null() && !context.data.is_object() {
        return Err(TemplateError::new("invalid render context"));
    }

    let mut environment = Environment::new();
    environment.set_keep_trailing_newline(false);
    environment.set_trim_blocks(true);
    environment.set_lstrip_blocks(true);
    environment.set_undefined_behavior(UndefinedBehavior::Lenient);
    environment.set_unknown_method_callback(|_state, value, method, args| {
        if method == "get" && value.kind() == minijinja::value::ValueKind::Map {
            let key = args
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "invalid map key"))?;
            let result = value.get_attr(key)?;
            if result.is_undefined() {
                return Ok(args.get(1).cloned().unwrap_or_else(|| Value::from(())));
            }
            return Ok(result);
        }
        if method == "replace"
            && value.kind() == minijinja::value::ValueKind::String
            && args.len() == 2
        {
            let source: String = value.clone().into();
            let old: String = args[0].clone().into();
            let new: String = args[1].clone().into();
            return Ok(Value::from(source.replace(&old, &new)));
        }
        Err(Error::from(ErrorKind::UnknownMethod))
    });
    environment.set_auto_escape_callback(|candidate| {
        if candidate.ends_with(".html.j2") {
            AutoEscape::Html
        } else {
            AutoEscape::None
        }
    });

    let variables = context_values(context)?;
    let template = environment
        .template_from_named_str(name, template.source)
        .map_err(|_| TemplateError::named("template could not be parsed", name))?;
    template
        .render(Value::from_pairs(variables))
        .map_err(|_| TemplateError::named("template could not be rendered", name))
}

/// Explicitly named variant of [`render`].
pub fn render_template(
    template_name: &str,
    context: &RenderContext,
) -> Result<String, TemplateError> {
    render(template_name, context)
}

fn canonical_name(input: &str) -> Result<&str, TemplateError> {
    if input.is_empty()
        || input.len() > MAX_NAME_BYTES
        || input.contains('\\')
        || input.starts_with('/')
        || input.chars().any(char::is_control)
    {
        return Err(TemplateError::new("invalid template name"));
    }
    let name = input
        .strip_prefix("laws/")
        .or_else(|| input.strip_prefix("templates/"))
        .unwrap_or(input);
    if name.is_empty()
        || name
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(TemplateError::new("invalid template name"));
    }
    if name != input && (input.matches('/').count() != 1 || name.contains('/')) {
        return Err(TemplateError::new("invalid template name"));
    }
    if TEMPLATES.iter().any(|template| template.name == name) {
        Ok(name)
    } else {
        Err(TemplateError::new("unknown template"))
    }
}

fn context_values(context: &RenderContext) -> Result<BTreeMap<String, Value>, TemplateError> {
    let serialized =
        serde_json::to_value(context).map_err(|_| TemplateError::new("invalid render context"))?;
    let JsonValue::Object(mut object) = serialized else {
        return Err(TemplateError::new("invalid render context"));
    };
    let extra = match object.remove("extra") {
        Some(JsonValue::Object(extra)) => extra,
        _ => Map::new(),
    };
    let now = object
        .get("now")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| TemplateError::new("invalid frozen timestamp"))?;
    let now = DateTime::parse_from_rfc3339(now)
        .map_err(|_| TemplateError::new("invalid frozen timestamp"))?;

    let mut values = BTreeMap::new();
    for (key, value) in object {
        values.insert(key, Value::from(minijinja::value::Serde(value)));
    }
    values.insert(
        "now".to_owned(),
        Value::from_object(FrozenDateTimeObject { value: now }),
    );
    for (key, value) in extra {
        values.insert(key, Value::from(minijinja::value::Serde(value)));
    }
    Ok(values)
}

/// Internal source inventory used by the drift-contract tests.
#[doc(hidden)]
pub fn embedded_template_sources() -> impl Iterator<Item = (&'static str, &'static str)> {
    TEMPLATES
        .iter()
        .map(|template| (template.name, template.source))
}

/// Internal HTML classification used by the drift-contract tests.
#[doc(hidden)]
pub fn is_html_template(name: &str) -> bool {
    TEMPLATES
        .iter()
        .find(|template| template.name == name)
        .is_some_and(|template| template.html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_inventory_is_exactly_eleven_sorted_names() {
        assert_eq!(TEMPLATES.len(), TEMPLATE_COUNT);
        let names = list_template_names();
        assert_eq!(names.len(), TEMPLATE_COUNT);
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            names
                .iter()
                .all(|name| TEMPLATES.iter().any(|template| template.name == *name))
        );
    }

    #[test]
    fn invalid_names_never_reach_the_template_engine() {
        let context = RenderContext::default();
        for name in [
            "../report.html.j2",
            "/report.html.j2",
            "templates/../report.html.j2",
            "report\\html.j2",
            "report\0.html.j2",
        ] {
            let error = render(name, &context).expect_err("invalid name must fail");
            assert!(!error.to_string().contains('\0'));
        }
    }
}
