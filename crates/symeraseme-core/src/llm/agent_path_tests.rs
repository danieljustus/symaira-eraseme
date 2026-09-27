#[cfg(unix)]
#[test]
fn relative_path_hit_matches_real_go_agent_availability() {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use serde_json::Value;
    use sha2::{Digest, Sha256};

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/agent-path/current-relative-entry.json"
    ));

    let fixture: Value = serde_json::from_str(FIXTURE).expect("Go agent path fixture parses");
    assert_eq!(fixture["schema"], "symeraseme.go-oracle.agent-path.v1");
    for (path, expected) in fixture["sources_sha256"].as_object().unwrap() {
        let source: &[u8] = match path.as_str() {
            "internal/llm/agent.go" => include_bytes!("../../../../internal/llm/agent.go"),
            "go.mod" => include_bytes!("../../../../go.mod"),
            other => panic!("unexpected source {other}"),
        };
        assert_eq!(
            hex::encode(Sha256::digest(source)),
            expected.as_str().unwrap(),
            "{path}"
        );
    }

    let root = tempfile::tempdir().expect("isolated working directory");
    let executable = root.path().join(fixture["cli"].as_str().unwrap());
    fs::write(&executable, "fake CLI").expect("write fake CLI in current directory");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("make fake CLI executable");
    assert!(
        super::is_executable(&executable),
        "oracle file exists and is executable"
    );
    assert_eq!(fixture["executable_exists"], true);
    assert_eq!(fixture["available"], false);

    let path = OsStr::new(fixture["path"].as_str().unwrap());
    assert!(!super::cli_on_path_from(
        fixture["cli"].as_str().unwrap(),
        Some(path),
        false,
        Some(root.path())
    ));
    let agent = super::AgentClient::with_probe("auto", "claude", Vec::new(), &|name| {
        super::cli_on_path_from(name, Some(path), false, Some(root.path()))
    });
    assert_eq!(agent.is_available(), fixture["available"]);
    assert!(super::cli_on_path_from(
        fixture["cli"].as_str().unwrap(),
        Some(root.path().as_os_str()),
        false,
        Some(root.path())
    ));
    assert!(super::cli_on_path_from(
        fixture["cli"].as_str().unwrap(),
        Some(path),
        true,
        Some(root.path())
    ));
}

#[test]
fn production_factory_defers_agent_resolution_until_first_availability_check() {
    use serde_json::Value;
    use sha2::{Digest, Sha256};

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/agent-path/current-relative-entry.json"
    ));

    let fixture: Value = serde_json::from_str(FIXTURE).expect("Go agent path fixture parses");
    for (path, expected) in fixture["sources_sha256"].as_object().unwrap() {
        let source: &[u8] = match path.as_str() {
            "internal/llm/agent.go" => include_bytes!("../../../../internal/llm/agent.go"),
            "go.mod" => include_bytes!("../../../../go.mod"),
            other => panic!("unexpected source {other}"),
        };
        assert_eq!(
            hex::encode(Sha256::digest(source)),
            expected.as_str().unwrap(),
            "{path}"
        );
    }

    let options = super::CreateOptions {
        provider: "agent".to_owned(),
        agent_backend: "claude".to_owned(),
        ..Default::default()
    };
    let client = super::create(&options, &|_| None).expect("create agent client");
    assert!(client.backend_resolution.get().is_none());
    assert_eq!(fixture["available_after_path_change"], true);

    // This injected probe models the PATH that Go observes after construction.
    // The production factory's unresolved cache must use the first probe result.
    assert!(client.resolve_backend(&|name| name == "claude").1);
    assert_eq!(client.resolved_backend(), "claude");
}
