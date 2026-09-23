#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use symeraseme_core::storage::repository::Repository;
use symeraseme_core::storage::store::Store;

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/cli-triage/cases.json"
);
const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

const FAKE_AGENT_SCRIPT: &str = r#"#!/bin/sh
case "$*" in
  *"--model oracle-env-model"*) printf '%s\n' '{"classification":"confirmed","confidence":0.93,"summary":"model selected from environment","extracted_fields":{"ticket":"T-42"}}' ;;
  *"--model oracle-flag-model"*) printf '%s\n' '{"classification":"confirmed","confidence":0.93,"summary":"model selected from flag","extracted_fields":{"ticket":"T-42"}}' ;;
  *"rejection classifier"*) printf '%s\n' '{"classification":"address_mismatch","confidence":0.91,"summary":"address differs","key_points":[],"jurisdiction":"GDPR"}' ;;
  *"email classifier"*) printf '%s\n' '{"classification":"confirmed","confidence":0.93,"summary":"deletion confirmed","extracted_fields":{"ticket":"T-42"}}' ;;
  *) printf '%s\n' '{"classification":"other","confidence":0.1,"summary":"unexpected prompt","key_points":[],"jurisdiction":"unknown"}' ;;
esac
"#;

#[derive(Debug, Deserialize)]
struct Fixture {
    schema: String,
    generator_sha256: String,
    fake_agent_sha256: String,
    sources: Vec<Source>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Source {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    argv: Vec<String>,
    environment: std::collections::BTreeMap<String, String>,
    exit_code: i32,
    stdout_base64: String,
    stderr_base64: String,
    state: Snapshot,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct Snapshot {
    classification: Option<String>,
    confidence: Option<f64>,
    summary: Option<String>,
    events: Vec<Event>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct Event {
    request_id: i64,
    event_type: String,
    source: String,
    payload_json: String,
}

#[test]
fn cli_triage_matches_source_bound_go_oracle() {
    let fixture_bytes = fs::read(FIXTURE_PATH).expect("CLI triage fixture readable");
    let fixture: Fixture = serde_json::from_slice(&fixture_bytes).expect("fixture parses");
    assert_eq!(fixture.schema, "symeraseme.go-oracle.cli-triage.v1");

    let generator_path = Path::new(REPO_ROOT).join("rust-tests/parity/oracle/cli-triage/main.go");
    let generator = fs::read(&generator_path).expect("Go oracle generator readable");
    assert_eq!(sha256(&generator), fixture.generator_sha256);
    assert_eq!(
        sha256(FAKE_AGENT_SCRIPT.as_bytes()),
        fixture.fake_agent_sha256
    );
    for source in &fixture.sources {
        let bytes = fs::read(Path::new(REPO_ROOT).join(&source.path))
            .unwrap_or_else(|error| panic!("read Go source {}: {error}", source.path));
        assert_eq!(
            sha256(&bytes),
            source.sha256,
            "Go oracle source changed: {}",
            source.path
        );
    }

    let go = Command::new("go")
        .args(["run", "./rust-tests/parity/oracle/cli-triage"])
        .current_dir(REPO_ROOT)
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .output()
        .expect("run source-bound Go CLI oracle");
    assert!(
        go.status.success(),
        "Go CLI oracle failed:\n{}\n{}",
        String::from_utf8_lossy(&go.stdout),
        String::from_utf8_lossy(&go.stderr)
    );
    assert_eq!(go.stdout, fixture_bytes, "Go CLI oracle fixture drifted");

    for case in &fixture.cases {
        replay_case(case);
    }
}

fn replay_case(case: &Case) {
    let root = TestRoot::new();
    let home = root.path().join("home");
    let data_dir = root.path().join("data");
    let bin_dir = root.path().join("bin");
    fs::create_dir_all(&home).expect("home directory");
    fs::create_dir_all(&data_dir).expect("data directory");
    fs::create_dir_all(&bin_dir).expect("fake agent directory");
    let fake_agent = bin_dir.join("claude");
    fs::write(&fake_agent, FAKE_AGENT_SCRIPT).expect("fake agent written");
    let mut permissions = fs::metadata(&fake_agent)
        .expect("fake agent metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_agent, permissions).expect("fake agent executable");

    seed_store(&data_dir);
    let output = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(&case.argv)
        .current_dir(root.path())
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &bin_dir)
        .env("SYMERASEME_DATA_DIR", &data_dir)
        .envs(case.environment.iter())
        .env("TERM", "dumb")
        .output()
        .unwrap_or_else(|error| panic!("spawn Rust {}: {error}", case.id));
    assert_eq!(
        output.status.code().unwrap_or(-1),
        case.exit_code,
        "{} exit code; stderr={}",
        case.id,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        base64::engine::general_purpose::STANDARD
            .decode(&case.stdout_base64)
            .expect("fixture stdout base64"),
        "{} stdout",
        case.id
    );
    let expected_stderr = base64::engine::general_purpose::STANDARD
        .decode(&case.stderr_base64)
        .expect("fixture stderr base64");
    assert_eq!(
        normalize_provider_order(&output.stderr),
        normalize_provider_order(&expected_stderr),
        "{} stderr",
        case.id
    );
    assert_eq!(
        read_snapshot(&data_dir),
        case.state,
        "{} side effects",
        case.id
    );
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "symeraseme-cli-triage-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create unique case directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn seed_store(data_dir: &Path) {
    let store = Store::open(data_dir.join("symeraseme.db")).expect("seed store open");
    let request_id = Repository::new(&store)
        .create_removal_request(
            "oracle-broker",
            "email",
            "oracle-campaign",
            "DE",
            "gdpr-art17.de.md.j2",
            "",
        )
        .expect("seed removal request");
    store
        .connection()
        .execute(
            "INSERT INTO inbox_replies (request_id, message_id, thread_id, from_addr, subject, snippet)\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                request_id,
                "oracle-message",
                "oracle-thread",
                "privacy@example.invalid",
                "We need your current address",
                "Your current address does not match our records.",
            ),
        )
        .expect("seed inbox reply");
}

fn read_snapshot(data_dir: &Path) -> Snapshot {
    let store = Store::open(data_dir.join("symeraseme.db")).expect("result store open");
    let (classification, confidence, summary) = store
        .connection()
        .query_row(
            "SELECT classified_as, classifier_confidence, llm_summary FROM inbox_replies WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read reply result");
    let mut statement = store
        .connection()
        .prepare(
            "SELECT request_id, event_type, source, payload_json FROM request_events ORDER BY id",
        )
        .expect("prepare events");
    let events = statement
        .query_map([], |row| {
            Ok(Event {
                request_id: row.get(0)?,
                event_type: row.get(1)?,
                source: row.get(2)?,
                payload_json: row.get(3)?,
            })
        })
        .expect("query events")
        .map(|row| row.expect("event row"))
        .collect();
    Snapshot {
        classification,
        confidence,
        summary,
        events,
    }
}

// Go iterates its provider registry map in randomized order in unknown-provider
// errors. Canonicalize only that list so the fixture stays stable without hiding
// missing or unexpected providers.
fn normalize_provider_order(stderr: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(stderr);
    let Some(index) = text.find("Known providers: ") else {
        return stderr.to_vec();
    };
    let start = index + "Known providers: ".len();
    let end = text[start..]
        .find('\n')
        .map_or(text.len(), |offset| start + offset);
    let mut providers = text[start..end].split(", ").collect::<Vec<_>>();
    providers.sort_unstable();
    format!("{}{}{}", &text[..start], providers.join(", "), &text[end..]).into_bytes()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
