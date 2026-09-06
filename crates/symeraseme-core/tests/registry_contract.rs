use std::fs;
use std::path::{Path, PathBuf};

use symeraseme_core::registry::{Broker, Channel, RegistryError, load_from_dir};

const FIXTURE_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/registry-contract"
);
const REGISTRY_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../registry");

fn fixture(name: &str) -> (String, String) {
    let path = Path::new(FIXTURE_ROOT).join(name);
    let id = path.file_stem().unwrap().to_string_lossy().into_owned();
    (id, fs::read_to_string(path).unwrap())
}

#[test]
fn golden_fixtures_load_unchanged() {
    for name in [
        "golden-webform-us.yaml",
        "golden-email-eu.yaml",
        "golden-multi-uk.yaml",
        "golden-minimal-us.yaml",
    ] {
        let (id, source) = fixture(name);
        let broker = Broker::from_yaml(&id, &source).unwrap();
        assert_eq!(broker.id, id);
        assert!(!broker.opt_out.is_empty());
    }
}

#[test]
fn invalid_fixture_and_focused_negative_cases_reject() {
    let (id, source) = fixture("invalid-unknown-field.yaml");
    assert!(Broker::from_yaml(&id, &source).is_err());

    let base = "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n";
    let cases = [
        (format!("{base}bogus: true\n"), "unknown"),
        (
            base.replace("category: other", "category: unknown"),
            "unknown",
        ),
        (
            base.replace(
                "opt_out:\n  - type: email\n    endpoint: a@example.test\n",
                "opt_out: []\n",
            ),
            "opt_out",
        ),
        (
            base.replace(
                "opt_out:\n  - type: email\n    endpoint: a@example.test\n",
                "opt_out:\n  - type: web_form\n    url: https://example.test\n",
            ),
            "form_spec",
        ),
        (
            base.replace(
                "  - type: email\n    endpoint: a@example.test\n",
                "  - type: email\n    endpoint: a@example.test\n    url: https://example.test\n",
            ),
            "url",
        ),
        (
            base.replace(
                "    endpoint: a@example.test\n",
                "    endpoint: a@example.test\n    template: unknown\n",
            ),
            "unknown",
        ),
        (
            base.replace(
                "opt_out:\n  - type: email\n    endpoint: a@example.test\n",
                "opt_out:\n  - type: web_form\n    url: https://example.test\n    form_spec:\n      steps:\n        - fill:\n            '@': value\n",
            ),
            "selector",
        ),
        (format!("{base}added_date: tomorrow\n"), "added_date"),
    ];
    for (source, expected) in cases {
        let error = Broker::from_yaml("test", &source).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn every_live_registry_broker_validates_and_matches_corpus_count() {
    let brokers = load_from_dir(REGISTRY_ROOT).unwrap();
    assert_eq!(brokers.len(), 1_277);
    assert!(brokers.windows(2).all(|pair| pair[0].id < pair[1].id));
}

#[test]
fn models_preserve_channel_variants_and_defaults() {
    let (id, source) = fixture("golden-minimal-us.yaml");
    let broker = Broker::from_yaml(&id, &source).unwrap();
    assert_eq!(broker.data_sensitivity, 5);
    assert_eq!(broker.status.to_string(), "active");
    assert!(matches!(broker.opt_out[0], Channel::Email { .. }));
    assert!(broker.verification.is_none());
}

#[test]
fn filename_stem_is_part_of_validation() {
    let (id, source) = fixture("golden-email-eu.yaml");
    let error = Broker::from_yaml("different-id", &source).unwrap_err();
    assert!(error.to_string().contains("file stem"));
    assert_eq!(PathBuf::from(id).file_stem().unwrap(), "golden-email-eu");
}

#[test]
fn errors_are_classified_as_registry_errors() {
    let error = Broker::from_yaml("test", "id: test\n").unwrap_err();
    assert!(matches!(
        error,
        RegistryError::Validation { .. } | RegistryError::Yaml(_)
    ));
}
