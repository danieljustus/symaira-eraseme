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

fn base_broker() -> &'static str {
    "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n"
}

#[test]
fn validated_entry_point_rejects_strict_variants_and_nulls() {
    let broker_unknown = format!("{}bogus: true\n", base_broker());
    assert!(Broker::from_yaml("test", &broker_unknown).is_err());

    let channel_unknown = format!(
        "{}  - type: email\n    endpoint: a@example.test\n    bogus: true\n",
        base_broker().replace("  - type: email\n    endpoint: a@example.test\n", "")
    );
    assert!(Broker::from_yaml("test", &channel_unknown).is_err());

    let channel_variant = base_broker().replace(
        "endpoint: a@example.test",
        "endpoint: a@example.test\n    url: https://example.test",
    );
    assert!(Broker::from_yaml("test", &channel_variant).is_err());

    let channel_null = base_broker().replace(
        "endpoint: a@example.test",
        "endpoint: a@example.test\n    template: null",
    );
    assert!(Broker::from_yaml("test", &channel_null).is_err());
    let broker_null = format!("{}disabled: null\n", base_broker());
    assert!(Broker::from_yaml("test", &broker_null).is_err());
}

#[test]
fn explicit_nulls_are_rejected_but_omitted_fields_serialize_without_nulls() {
    for field in [
        "data_sensitivity",
        "verification",
        "disabled",
        "added_date",
        "source",
        "status",
        "notes",
    ] {
        let source = format!("{}{field}: null\n", base_broker());
        assert!(Broker::from_yaml("test", &source).is_err(), "{field}");
    }
    for field in [
        "endpoint",
        "template",
        "locale",
        "required_fields",
        "supports_suppression",
        "expected_response_days",
        "disabled",
    ] {
        let source = format!("{}  {field}: null\n", base_broker());
        assert!(
            Broker::from_yaml("test", &source).is_err(),
            "channel {field}"
        );
    }

    let web = "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: web_form\n    url: https://example.test\n    form_spec:\n      steps:\n        - goto: https://example.test\n";
    for field in ["timeout_seconds", "rate_limit_delay", "headless"] {
        let source = format!("{}      {field}: null\n", web);
        assert!(Broker::from_yaml("test", &source).is_err(), "form {field}");
    }
    let step_null = format!("{}          wait_seconds: null\n", web);
    assert!(Broker::from_yaml("test", &step_null).is_err());

    let broker = Broker::from_yaml("test", base_broker()).unwrap();
    let json = serde_json::to_value(&broker).unwrap();
    let object = json.as_object().unwrap();
    for field in ["verification", "disabled", "added_date", "source", "notes"] {
        assert!(!object.contains_key(field), "serialized {field} as null");
    }
    assert_eq!(object["status"], "active");
    let explicit_false =
        Broker::from_yaml("test", &format!("{}disabled: false\n", base_broker())).unwrap();
    assert_eq!(
        serde_json::to_value(explicit_false).unwrap()["disabled"],
        false
    );
}

#[test]
fn nonfinite_numeric_values_are_rejected() {
    let web = "id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: web_form\n    url: https://example.test\n    form_spec:\n      steps:\n        - goto: https://example.test\n";
    for field in ["timeout_seconds", "rate_limit_delay"] {
        for value in [".nan", ".inf", "-.inf"] {
            let source = format!("{}      {field}: {value}\n", web);
            assert!(
                Broker::from_yaml("test", &source).is_err(),
                "{field} {value}"
            );
        }
    }
    for value in [".nan", ".inf", "-.inf"] {
        let source = format!("{}          wait_seconds: {value}\n", web);
        assert!(Broker::from_yaml("test", &source).is_err(), "wait {value}");
    }
    for value in [".nan", ".inf", "-.inf"] {
        let source = format!(
            "{}          solve_captcha:\n            type: turnstile\n            site_key: 12345678\n            min_score: {value}\n",
            web
        );
        assert!(Broker::from_yaml("test", &source).is_err(), "score {value}");
    }
}

#[test]
fn added_date_is_calendar_valid_and_uri_format_remains_compatibility_only() {
    for (date, valid) in [
        ("2024-02-29", true),
        ("2000-02-29", true),
        ("2023-02-29", false),
        ("1900-02-29", false),
        ("2024-00-10", false),
        ("2024-13-01", false),
        ("2024-04-31", false),
    ] {
        let source = format!("{}added_date: {date}\n", base_broker());
        assert_eq!(Broker::from_yaml("test", &source).is_ok(), valid, "{date}");
    }
    let source = base_broker().replace("https://example.test", "privacy@host");
    assert!(Broker::from_yaml("test", &source).is_ok());
}

#[test]
fn loader_rejects_symlinks_duplicates_deep_paths_and_oversize_documents() {
    let root = tempfile_root();
    let brokers = root.path().join("brokers");
    fs::create_dir_all(brokers.join("us")).unwrap();
    fs::write(brokers.join("us/test.yaml"), base_broker()).unwrap();
    let target = root.path().join("outside.yaml");
    fs::write(&target, base_broker()).unwrap();
    symlink_file(&target, &brokers.join("us/link.yaml")).unwrap();
    assert!(load_from_dir(root.path()).is_err());

    let duplicate = tempfile_root();
    fs::create_dir_all(duplicate.path().join("brokers/us")).unwrap();
    fs::create_dir_all(duplicate.path().join("brokers/eu")).unwrap();
    fs::write(duplicate.path().join("brokers/us/test.yaml"), base_broker()).unwrap();
    fs::write(duplicate.path().join("brokers/eu/test.yaml"), base_broker()).unwrap();
    assert!(load_from_dir(duplicate.path()).is_err());

    let deep = tempfile_root();
    let mut path = deep.path().join("brokers");
    for index in 0..10 {
        path = path.join(format!("d{index}"));
    }
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("test.yaml"), base_broker()).unwrap();
    assert!(load_from_dir(deep.path()).is_err());

    let large = tempfile_root();
    fs::create_dir_all(large.path().join("brokers/us")).unwrap();
    fs::write(
        large.path().join("brokers/us/test.yaml"),
        format!("{}{}", base_broker(), "x".repeat(1 << 20)),
    )
    .unwrap();
    assert!(load_from_dir(large.path()).is_err());
}

#[test]
fn yaml_scalars_are_parsed_in_context() {
    let source = format!(
        "{}notes: |\n  hand &foot *bar #tag\n  apostrophe's\n",
        base_broker().replace("name: Test", "name: O'Reilly")
    );
    let broker = Broker::from_yaml("test", &source).unwrap();
    assert_eq!(broker.name, "O'Reilly");
    assert!(broker.notes.unwrap().contains("&foot *bar #tag"));
}

#[test]
fn loader_rejects_non_regular_yaml_entries_and_enforces_file_caps() {
    let directory = tempfile_root();
    fs::create_dir_all(directory.path().join("brokers/us/not-a-file.yaml")).unwrap();
    assert!(load_from_dir(directory.path()).is_err());

    #[cfg(unix)]
    {
        use std::os::unix::net::UnixListener;
        let socket = tempfile_root();
        fs::create_dir_all(socket.path().join("brokers/us")).unwrap();
        let socket_path = socket.path().join("brokers/us/socket.yaml");
        let _listener = UnixListener::bind(&socket_path).unwrap();
        assert!(load_from_dir(socket.path()).is_err());
    }

    let many_files = tempfile_root();
    fs::create_dir_all(many_files.path().join("brokers/us")).unwrap();
    for index in 0..=4_096 {
        fs::write(
            many_files
                .path()
                .join("brokers/us")
                .join(format!("broker-{index}.yaml")),
            "",
        )
        .unwrap();
    }
    let error = load_from_dir(many_files.path()).unwrap_err();
    assert!(error.to_string().contains("broker YAML file limit"));
}

#[test]
fn loader_enforces_aggregate_bytes_and_nodes() {
    let bytes_root = tempfile_root();
    fs::create_dir_all(bytes_root.path().join("brokers/us")).unwrap();
    for index in 0..17 {
        let document = base_broker().replace("id: test", &format!("id: broker-{index}"));
        let padding = "x".repeat((1 << 20) - document.len() - 32);
        let padded = format!("{document}notes: |\n  {padding}\n");
        fs::write(
            bytes_root
                .path()
                .join("brokers/us")
                .join(format!("broker-{index}.yaml")),
            &padded,
        )
        .unwrap();
    }
    let error = load_from_dir(bytes_root.path()).unwrap_err();
    assert!(
        error.to_string().contains("aggregate input byte limit"),
        "unexpected aggregate byte error: {error}"
    );

    let nodes_root = tempfile_root();
    fs::create_dir_all(nodes_root.path().join("brokers/us")).unwrap();
    let jurisdictions = (0..7_500).map(|_| "US").collect::<Vec<_>>().join(",");
    let source_template = base_broker().replace(
        "jurisdictions: [US]",
        &format!("jurisdictions: [{jurisdictions}]"),
    );
    for index in 0..140 {
        let source = source_template.replace("id: test", &format!("id: broker-{index}"));
        fs::write(
            nodes_root
                .path()
                .join("brokers/us")
                .join(format!("broker-{index}.yaml")),
            &source,
        )
        .unwrap();
    }
    let error = load_from_dir(nodes_root.path()).unwrap_err();
    assert!(error.to_string().contains("aggregate YAML node limit"));
}

#[test]
fn loader_rejects_aliases_and_malformed_yaml_deterministically() {
    let root = tempfile_root();
    fs::create_dir_all(root.path().join("brokers/us")).unwrap();
    let anchored = base_broker().replace(
        "endpoint: a@example.test",
        "endpoint: &email a@example.test",
    );
    fs::write(root.path().join("brokers/us/test.yaml"), anchored).unwrap();
    assert!(load_from_dir(root.path()).is_err());

    let malformed = tempfile_root();
    fs::create_dir_all(malformed.path().join("brokers/us")).unwrap();
    fs::write(
        malformed.path().join("brokers/us/test.yaml"),
        "id: test\n: malformed\n",
    )
    .unwrap();
    assert!(load_from_dir(malformed.path()).is_err());

    for marker in ["&email a@example.test", "*email"] {
        let guarded = tempfile_root();
        fs::create_dir_all(guarded.path().join("brokers/us")).unwrap();
        let source =
            base_broker().replace("endpoint: a@example.test", &format!("endpoint: {marker}"));
        fs::write(guarded.path().join("brokers/us/test.yaml"), source).unwrap();
        assert!(load_from_dir(guarded.path()).is_err(), "{marker}");
    }

    let nested = tempfile_root();
    fs::create_dir_all(nested.path().join("brokers/us")).unwrap();
    let deeply_nested = format!("{}null{}", "[".repeat(70), "]".repeat(70));
    fs::write(
        nested.path().join("brokers/us/test.yaml"),
        format!("id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out: {deeply_nested}\n"),
    )
    .unwrap();
    assert!(load_from_dir(nested.path()).is_err());

    let too_many_nodes = tempfile_root();
    fs::create_dir_all(too_many_nodes.path().join("brokers/us")).unwrap();
    let values = (0..20_000).map(|_| "null").collect::<Vec<_>>().join(",");
    let prefix = base_broker().split("opt_out:").next().unwrap();
    fs::write(
        too_many_nodes.path().join("brokers/us/test.yaml"),
        format!("{prefix}opt_out: [{values}]\n"),
    )
    .unwrap();
    assert!(load_from_dir(too_many_nodes.path()).is_err());
}

#[cfg(unix)]
fn symlink_file(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

#[cfg(windows)]
fn symlink_file(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(original, link)
}

fn tempfile_root() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
