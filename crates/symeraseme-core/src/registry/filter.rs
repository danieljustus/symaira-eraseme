use super::model::{Broker, Category, Jurisdiction, Law, Priority, Status};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BrokerFilter {
    pub jurisdiction: Option<String>,
    pub law: Option<String>,
    pub priority: Option<String>,
    pub category: Option<String>,
    pub include_disabled: bool,
    pub status: Option<String>,
    pub include_inactive: bool,
}

pub fn filter_brokers<'a>(brokers: &'a [Broker], filter: &BrokerFilter) -> Vec<&'a Broker> {
    brokers
        .iter()
        .filter(|broker| {
            if !filter.include_disabled && broker.disabled == Some(true) {
                return false;
            }
            if let Some(value) = filter.jurisdiction.as_deref()
                && !value.is_empty()
                && !broker
                    .jurisdictions
                    .iter()
                    .any(|candidate| candidate.registry_name() == value)
            {
                return false;
            }
            if let Some(value) = filter.law.as_deref()
                && !value.is_empty()
                && !broker
                    .laws
                    .iter()
                    .any(|candidate| candidate.registry_name() == value)
            {
                return false;
            }
            if let Some(value) = filter.priority.as_deref()
                && !value.is_empty()
                && broker.priority.registry_name() != value
            {
                return false;
            }
            if let Some(value) = filter.category.as_deref()
                && !value.is_empty()
                && broker.category.registry_name() != value
            {
                return false;
            }
            if !filter.include_inactive {
                let expected = filter
                    .status
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .unwrap_or("active");
                if broker.status.registry_name() != expected {
                    return false;
                }
            }
            true
        })
        .collect()
}

pub fn filter_brokers_owned(brokers: &[Broker], filter: &BrokerFilter) -> Vec<Broker> {
    filter_brokers(brokers, filter)
        .into_iter()
        .cloned()
        .collect()
}

trait RegistryEnumName {
    fn registry_name(self) -> &'static str;
}

impl RegistryEnumName for Jurisdiction {
    fn registry_name(self) -> &'static str {
        match self {
            Jurisdiction::At => "AT",
            Jurisdiction::Ch => "CH",
            Jurisdiction::De => "DE",
            Jurisdiction::Dk => "DK",
            Jurisdiction::Eu => "EU",
            Jurisdiction::Fi => "FI",
            Jurisdiction::Fr => "FR",
            Jurisdiction::Gb => "GB",
            Jurisdiction::Ie => "IE",
            Jurisdiction::Il => "IL",
            Jurisdiction::Lu => "LU",
            Jurisdiction::Nl => "NL",
            Jurisdiction::No => "NO",
            Jurisdiction::Se => "SE",
            Jurisdiction::Uk => "UK",
            Jurisdiction::Us => "US",
        }
    }
}

impl RegistryEnumName for Law {
    fn registry_name(self) -> &'static str {
        match self {
            Law::Gdpr => "GDPR",
            Law::Ccpa => "CCPA",
            Law::Cpra => "CPRA",
            Law::Lgpd => "LGPD",
            Law::Pipeda => "PIPEDA",
        }
    }
}

impl RegistryEnumName for Priority {
    fn registry_name(self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
        }
    }
}

impl RegistryEnumName for Category {
    fn registry_name(self) -> &'static str {
        match self {
            Category::PeopleSearch => "people-search",
            Category::Marketing => "marketing",
            Category::Credit => "credit",
            Category::Analytics => "analytics",
            Category::BackgroundCheck => "background-check",
            Category::SocialMedia => "social-media",
            Category::Other => "other",
        }
    }
}

impl RegistryEnumName for Status {
    fn registry_name(self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Deprecated => "deprecated",
            Status::Merged => "merged",
            Status::OutOfBusiness => "out-of-business",
        }
    }
}
