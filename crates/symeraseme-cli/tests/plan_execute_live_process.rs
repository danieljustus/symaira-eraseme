//! Consented live \`plan execute\` process parity against the Go CLI built from
//! this checkout. A synthetic web-form broker has no executor, so neither
//! process can contact a provider or submit a form.

use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use symeraseme_core::storage::{
    Repository, Store,
    types::{EventType, Source},
};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "symeraseme-cli-live-plan-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("temporary root");
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn prepare(root: &Path) -> (PathBuf, PathBuf, PathBuf, String) {
    let home = root.join("home");
    let data = root.join("data");
    let cwd = root.join("cwd");
    let resources = root.join("resources");
    for directory in [
        &home,
        &data,
        &cwd,
        &resources.join("brokers/us"),
        &resources.join("schemas"),
    ] {
        fs::create_dir_all(directory).expect("isolated fixture directory");
    }
    fs::write(
        resources.join("manifest.json"),
        br#"{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}"#,
    )
    .expect("registry manifest");
    fs::write(
        resources.join("schemas/broker.schema.json"),
        br#"{"schema_version":1}"#,
    )
    .expect("broker schema");
    fs::write(
        resources.join("brokers/us/synthetic-live-form.yaml"),
        b"id: synthetic-live-form\nname: Synthetic Live Form\nwebsite: https://synthetic.example\ncategory: other\njurisdictions: [US]\nlaws: [CCPA]\npriority: low\nopt_out:\n  - type: web_form\n    url: https://synthetic.example/optout\n    form_spec:\n      steps:\n        - click: '#submit'\nadded_date: '2025-05-21'\nstatus: active\n",
    )
    .expect("synthetic broker");

    let store = Store::open(data.join("symeraseme.db")).expect("isolated store");
    let repository = Repository::new(&store);
    repository
        .create_campaign("live", "initial", "")
        .expect("campaign");
    let request = repository
        .create_removal_request("synthetic-live-form", "web_form", "live", "US", "", "")
        .expect("planned request");
    store
        .append_and_project(
            request,
            &EventType::Planned,
            &Map::new(),
            &Source::System,
            chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
        )
        .expect("planned event");
    drop(repository);
    drop(store);

    let token = symeraseme_core::identity::ConsentStore::new(&data)
        .issue_token("execute", 600)
        .expect("private execute consent");
    (home, data, cwd, token)
}

fn build_go_cli(root: &Path) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = root.join("symeraseme-go");
    let cache = root.join("go-cache");
    fs::create_dir_all(&cache).expect("internal temporary Go cache");
    let go = std::env::var_os("SYMERASEME_GO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("go"));
    let built = Command::new(go)
        .current_dir(repo)
        .env("GOWORK", "off")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .env("GOCACHE", cache)
        .env("GOTOOLCHAIN", "go1.26.6")
        .args(["build", "-o"])
        .arg(&output)
        .arg("./cmd/symeraseme")
        .output()
        .expect("build source-bound Go CLI");
    assert!(
        built.status.success(),
        "source-bound Go CLI build failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    output
}

fn run(
    program: &Path,
    home: &Path,
    data: &Path,
    cwd: &Path,
    resources: &Path,
    token: &str,
) -> Output {
    Command::new(program)
        .args([
            "--output",
            "json",
            "plan",
            "execute",
            "--campaign",
            "live",
            "--consent",
            token,
        ])
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("SYMERASEME_DATA_DIR", data)
        .env("SYMERASEME_RESOURCES", resources)
        .output()
        .expect("CLI process")
}

type TaskRow = (i64, String, String, String, String, String);
type EventRow = (String, Value, String);

fn persisted_effects(data: &Path) -> (String, Vec<EventRow>, Vec<TaskRow>) {
    let store = Store::open(data.join("symeraseme.db")).expect("read execution effects");
    let state: String = store
        .connection()
        .query_row(
            "SELECT current_status FROM request_state WHERE request_id=1",
            [],
            |row| row.get(0),
        )
        .expect("request state");
    let events = Repository::new(&store)
        .get_events(1, 0)
        .expect("execution events")
        .into_iter()
        .map(|event| {
            (
                event.event_type.to_string(),
                Value::Object(event.payload),
                event.source.as_str().to_owned(),
            )
        })
        .collect();
    let mut statement = store
        .connection()
        .prepare(
            "SELECT id, broker_id, broker_name, form_url, reason, status \
             FROM manual_tasks ORDER BY id",
        )
        .expect("manual task query");
    let tasks = statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .expect("manual task rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("manual tasks");
    (state, events, tasks)
}

#[test]
fn consented_live_plan_execute_matches_source_bound_go_process() {
    let root = TempRoot::new();
    let go = build_go_cli(&root.0);
    let go_root = root.0.join("go");
    let rust_root = root.0.join("rust");
    fs::create_dir_all(&go_root).expect("Go fixture root");
    fs::create_dir_all(&rust_root).expect("Rust fixture root");
    let (go_home, go_data, go_cwd, go_token) = prepare(&go_root);
    let (rust_home, rust_data, rust_cwd, rust_token) = prepare(&rust_root);
    let resources = go_root.join("resources");

    let go_output = run(&go, &go_home, &go_data, &go_cwd, &resources, &go_token);
    let rust_output = run(
        Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
        &rust_home,
        &rust_data,
        &rust_cwd,
        &resources,
        &rust_token,
    );

    assert_eq!(
        go_output.status.code(),
        rust_output.status.code(),
        "exit status"
    );
    assert_eq!(go_output.stderr, rust_output.stderr, "stderr");
    assert_eq!(
        go_output.status.code(),
        Some(0),
        "Go stdout: {}",
        String::from_utf8_lossy(&go_output.stdout)
    );
    assert!(
        go_output.stderr.is_empty(),
        "Go stderr: {}",
        String::from_utf8_lossy(&go_output.stderr)
    );
    assert_eq!(go_output.stdout, rust_output.stdout, "stdout bytes");
    let go_json: Value = serde_json::from_slice(&go_output.stdout).expect("Go JSON stdout");
    let rust_json: Value = serde_json::from_slice(&rust_output.stdout).expect("Rust JSON stdout");
    assert_eq!(go_json, rust_json, "CLI JSON result");
    assert_eq!(
        persisted_effects(&go_data),
        persisted_effects(&rust_data),
        "persisted live execution effects"
    );
}
