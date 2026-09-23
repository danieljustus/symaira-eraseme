//! DOM-002 GetPlan and execution transitions against the executable Go oracle.

use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{Mutex, MutexGuard},
};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use symeraseme_core::{
    campaign::{ExecuteOpts, execute_campaign, get_plan},
    identity::Profile,
    registry::load_from_dir,
    storage::{EventType, Repository, Source, Store},
};
use tempfile::tempdir;

const ORACLE_DIR: &str = "rust-tests/parity/oracle/campaign-execution";
static DATA_DIR_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug)]
struct DataDir {
    _guard: MutexGuard<'static, ()>,
    previous: Option<String>,
}

impl DataDir {
    fn set(path: &std::path::Path) -> Self {
        let guard: MutexGuard<'static, ()> = DATA_DIR_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous = std::env::var("SYMERASEME_DATA_DIR").ok();
        // SAFETY: this integration-test binary has one test, and the mutex
        // serializes all tests that write this process-wide variable.
        unsafe { std::env::set_var("SYMERASEME_DATA_DIR", path) };
        Self {
            _guard: guard,
            previous,
        }
    }
}

impl Drop for DataDir {
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var("SYMERASEME_DATA_DIR", value),
                None => std::env::remove_var("SYMERASEME_DATA_DIR"),
            }
        }
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pinned_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T00:00:00+00:00")
        .expect("pinned instant")
        .with_timezone(&Utc)
}

fn normalize_plan(mut plan: Map<String, Value>) -> Value {
    if let Some(rows) = plan.get_mut("requests").and_then(Value::as_array_mut) {
        for row in rows {
            if let Some(row) = row.as_object_mut() {
                for key in [
                    "created_at",
                    "last_event_at",
                    "sent_at",
                    "acknowledged_at",
                    "resolved_at",
                    "deadline_at",
                    "next_action_at",
                ] {
                    row.insert(key.to_owned(), Value::Null);
                }
            }
        }
    }
    Value::Object(plan)
}

#[test]
fn get_plan_and_execution_transitions_match_source_bound_go_oracle() {
    let root = repo_root();
    let oracle_dir = root.join(ORACLE_DIR);
    let go_oracle = fs::read(oracle_dir.join("main.go")).expect("read Go oracle source");
    let cases = fs::read(oracle_dir.join("cases.json")).expect("read oracle cases");
    let go_sources = [
        "internal/campaign/batch.go",
        "internal/campaign/execution.go",
        "internal/campaign/planning.go",
        "internal/campaign/webform.go",
        "internal/manualtasks/manualtasks.go",
        "internal/eventstore/repo.go",
        "internal/eventstore/store.go",
        "internal/eventstore/projection.go",
        "internal/registry/loader.go",
        "internal/identity/profile.go",
        "internal/identity/secrets.go",
        "registry/brokers/eu/adventori-eu.yaml",
    ];
    let mut provenance = Vec::new();
    for path in go_sources {
        let source =
            fs::read(root.join(path)).unwrap_or_else(|error| panic!("read {path}: {error}"));
        provenance.push(json!({
            "path": path,
            "sha256": hex::encode(Sha256::digest(source)),
        }));
    }
    assert_eq!(
        hex::encode(Sha256::digest(go_oracle)),
        "244627adcb446cb791cafedeffc98906d5cfcaaec119ae55db72e7098d4bf4c3",
        "Go oracle source changed; review and repin this executable contract"
    );
    assert_eq!(
        hex::encode(Sha256::digest(&cases)),
        "c3cceb4a1518e08fd328c3838b43887b4631b29c486be93b0190778c4cbb96fc",
        "campaign execution oracle inputs changed; review before repinning"
    );
    assert_eq!(
        serde_json::to_value(&provenance).expect("serialize source provenance"),
        json!([
            {"path":"internal/campaign/batch.go","sha256":"eedbcc3d38b79f933979f7e86b066dacbeed459f5698b919701d23c6396c0689"},
            {"path":"internal/campaign/execution.go","sha256":"eb68d2e1ae49b4908407c26849c4f69d212dd115caae451ca3bcb4f54febaf7c"},
            {"path":"internal/campaign/planning.go","sha256":"ee3599dd7bf23acbc36848e47776fc37379ef41abf73d2a2400f52176c945bfd"},
            {"path":"internal/campaign/webform.go","sha256":"2ba434e161ccd6ed64b8f883e44c1e211ee2c9c45a23a936bda9c7511746a398"},
            {"path":"internal/manualtasks/manualtasks.go","sha256":"15611b93090fda0959989360caf0e74eece1a11a63efe1937c9242763cde9974"},
            {"path":"internal/eventstore/repo.go","sha256":"339cf7d383d03de0fbb1f3cba994f8282b45c060a9a5294891e36893374a4625"},
            {"path":"internal/eventstore/store.go","sha256":"fd1dd416606f29aa4726a62ffe6ad83ef9d7c9eb6c6f42d81e87987913968df3"},
            {"path":"internal/eventstore/projection.go","sha256":"edfa14b15261c65672ca5a35eb05c16190a0d55bd07f9ce3df267b531295c43a"},
            {"path":"internal/registry/loader.go","sha256":"0f8ad425d8b660b7b7a894882fea7d7473823d765a0ca6baf9a7380816319dff"},
            {"path":"internal/identity/profile.go","sha256":"c637ff49dd7bdd278e11b4ca874e6115e18a982da4bf36631bb7053e259b25bf"},
            {"path":"internal/identity/secrets.go","sha256":"462c8fda343842bd674e50e974e59e1f6970794c9b60946dee3b39207362dd44"},
            {"path":"registry/brokers/eu/adventori-eu.yaml","sha256":"a6166b3246607f85968518f96b0491a4127add14f1ee3a2fe9cc89247d249c3f"}
        ]),
        "Go implementation sources changed; review and repin the oracle"
    );

    let output = Command::new("go")
        .args(["run", &format!("./{ORACLE_DIR}")])
        .current_dir(&root)
        .output()
        .expect("run local Go campaign oracle");
    assert!(
        output.status.success(),
        "Go campaign oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Value = serde_json::from_slice(&output.stdout).expect("parse Go oracle output");

    let input: Value = serde_json::from_slice(&cases).expect("parse campaign oracle input");
    let campaign_id = input["campaign_id"].as_str().expect("campaign id");
    let broker_id = input["broker_id"].as_str().expect("broker id");
    let broker_name = input["broker_name"].as_str().expect("broker name");
    let endpoint = input["endpoint"].as_str().expect("endpoint");
    let email_broker_id = input["email_broker_id"].as_str().expect("email broker id");
    let email_endpoint = input["email_endpoint"].as_str().expect("email endpoint");
    let brokers = load_from_dir(root.join("registry")).expect("load campaign registry");
    let data_dir = tempdir().expect("isolated manual-task data dir");
    let _data_dir = DataDir::set(data_dir.path());
    let store_dir = tempdir().expect("isolated event store");
    let store = Store::open(store_dir.path().join("store.sqlite")).expect("open event store");
    let repository = Repository::new(&store);
    repository
        .create_campaign(campaign_id, "initial", "")
        .expect("create campaign");
    let request_id = repository
        .create_removal_request(broker_id, "web_form", campaign_id, "DE", "", "")
        .expect("create request");
    let email_id = repository
        .create_removal_request(email_broker_id, "email", campaign_id, "DE", "", "")
        .expect("create email request");
    store
        .append_and_project(
            request_id,
            &EventType::Planned,
            &json!({"broker_name": broker_name, "endpoint": endpoint})
                .as_object()
                .expect("planned payload object")
                .clone(),
            &Source::System,
            pinned_now(),
        )
        .expect("append planned event");
    store
        .append_and_project(
            email_id,
            &EventType::Planned,
            &json!({"broker_name": email_broker_id, "endpoint": email_endpoint})
                .as_object()
                .expect("email planned payload object")
                .clone(),
            &Source::System,
            pinned_now(),
        )
        .expect("append email planned event");

    let plan = get_plan(&store, campaign_id, "").expect("get campaign plan");
    let all_plans = get_plan(&store, "", "").expect("get all plans");
    assert_eq!(all_plans["campaign_id"], "all");
    assert_eq!(all_plans["total"], 2);
    let empty_plan = get_plan(&store, "missing", "").expect("get missing campaign");
    assert_eq!(empty_plan["total"], 0);
    assert!(empty_plan["requests"].is_null());
    let profile = Profile {
        full_name: "Oracle Person".to_owned(),
        email_addresses: vec!["oracle@example.invalid".to_owned()],
        ..Profile::default()
    };
    let result = execute_campaign(
        &store,
        campaign_id,
        &ExecuteOpts {
            brokers: &brokers,
            ..ExecuteOpts::default()
        },
        Ok(Some(&profile)),
        5,
        pinned_now(),
    )
    .expect("execute campaign without send adapters");
    let mut events = Vec::new();
    for id in [request_id, email_id] {
        events.extend(
            Repository::new(&store)
                .get_events(id, 0)
                .expect("read transition events")
                .into_iter()
                .map(|event| {
                    json!({
                        "type": event.event_type.as_str(),
                        "request_id": event.request_id,
                        "payload": event.payload,
                    })
                }),
        );
    }
    let statuses = get_plan(&store, campaign_id, "SEND_FAILED").expect("read final request state")
        ["requests"]
        .as_array()
        .expect("failed requests array")
        .iter()
        .map(|request| {
            (
                request["id"].as_i64().expect("request id").to_string(),
                request["current_status"].clone(),
            )
        })
        .collect::<Map<_, _>>();
    let actual = json!({
        "plan": normalize_plan(plan),
        "result": result,
        "events": events,
        "statuses": statuses,
    });
    assert_eq!(actual, expected);
}
