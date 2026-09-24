//! Language-neutral fixtures captured by the pinned production migration.Run.
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, process::Command};
use symeraseme_engine::migration::{self, Options};

const FIXTURE: &str =
    include_str!("../../../rust-tests/parity/oracle/migration-state/fixture.json");
fn paths(mut data: Vec<u8>, root: &Path, expand: bool) -> Vec<u8> {
    for name in ["source", "destination", "backup"] {
        let path = root.join(name);
        let encoded = serde_json::to_string(path.to_str().unwrap()).unwrap();
        let actual = &encoded.as_bytes()[1..encoded.len() - 1];
        let placeholder = format!("@ROOT@/{name}");
        let (from, to) = if expand {
            (placeholder.as_bytes(), actual)
        } else {
            (actual, placeholder.as_bytes())
        };
        let mut result = Vec::new();
        let mut i = 0;
        while i < data.len() {
            if data[i..].starts_with(from) {
                result.extend_from_slice(to);
                i += from.len();
            } else {
                result.push(data[i]);
                i += 1;
            }
        }
        data = result;
    }
    data
}
fn snapshot(root: &Path) -> Value {
    fn walk(root: &Path, path: &Path, out: &mut serde_json::Map<String, Value>) {
        let name = path
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        if path.is_dir() {
            out.insert(format!("{name}/"), json!(""));
            for entry in fs::read_dir(path).unwrap() {
                walk(root, &entry.unwrap().path(), out);
            }
        } else {
            let bytes = fs::read(path).unwrap();
            // Byte replacement preserves malformed UTF-8 and CRLF verbatim.
            let data = paths(bytes, root, false);
            out.insert(name, json!(hex::encode(data)));
        }
    }
    let mut out = serde_json::Map::new();
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() != "home" {
            walk(root, &entry.path(), &mut out);
        }
    }
    Value::Object(out)
}
fn execute(case: &Value, root: &Path) -> Value {
    for name in ["source", "destination", "home"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    fs::write(root.join("source/config.toml"), b"source config\n").unwrap();
    fs::write(
        root.join("destination/config.toml"),
        b"destination sentinel\n",
    )
    .unwrap();
    for (name, key) in [
        ("destination/.migration-state.json", "state_hex"),
        ("backup/.complete.json", "marker_hex"),
    ] {
        if let Some(encoded) = case[key].as_str() {
            let raw = hex::decode(encoded).unwrap();
            let data = paths(raw, root, true);
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, data).unwrap();
        }
    }
    let before = snapshot(root);
    let options = Options {
        source_root: root.join("source").to_str().unwrap().into(),
        destination_root: root.join("destination").to_str().unwrap().into(),
        backup_dir: root.join("backup").to_str().unwrap().into(),
        home_dir: root.join("home").to_str().unwrap().into(),
        platform: "cron".into(),
        binary_path: root.join("bin/symeraseme").to_str().unwrap().into(),
        project_dir: root.to_str().unwrap().into(),
        ..Default::default()
    };
    let (report, error) = migration::run(&options);
    let report = report.expect("detection completed");
    json!({
        "error":error.unwrap_or_default(), "resumed":report.resumed, "complete":report.complete,
        "backup_reported":!report.backup_dir.is_empty(),
        "statuses":report.items.unwrap().iter().map(|i| i.status.as_str()).collect::<Vec<_>>(),
        "before":before,"after":snapshot(root),
    })
}

#[test]
fn migration_json_replay() {
    // Per-case subprocesses give the engine a disposable HOME and cwd without
    // unsafe process-wide environment mutation in the multithreaded test runner.
    if let Ok(id) = std::env::var("MIGRATION_JSON_CASE") {
        let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
        let actual = execute(case, &std::env::current_dir().unwrap());
        assert_eq!(actual, case["expected"], "case {id}");
        return;
    }
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    let declared: BTreeSet<_> = fixture["declared_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    assert_eq!(
        declared.len(),
        cases.len(),
        "duplicate/missing declarations"
    );
    let mut executed = BTreeSet::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let raw = hex::decode(case["raw_stdout_hex"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&raw).unwrap(),
            case["expected"],
            "raw oracle {id}"
        );
        let root = tempfile::Builder::new()
            .prefix("migration-json-")
            .tempdir()
            .unwrap();
        let root = root.path().canonicalize().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "migration_json_replay", "--nocapture"])
            .current_dir(&root)
            .env_clear()
            .env("MIGRATION_JSON_CASE", id)
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("TZ", "UTC");
        for key in [
            "HOME",
            "USERPROFILE",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
            "TMPDIR",
            "TMP",
            "TEMP",
        ] {
            let path = root.join("home").join(key);
            fs::create_dir_all(&path).unwrap();
            command.env(key, path);
        }
        // File-backed capture cannot deadlock on a large assertion diagnostic.
        let stdout_path = root.join("home/stdout");
        let stderr_path = root.join("home/stderr");
        command
            .stdout(fs::File::create(&stdout_path).unwrap())
            .stderr(fs::File::create(&stderr_path).unwrap());
        let mut child = command.spawn().unwrap();
        let started = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > std::time::Duration::from_secs(15) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("case {id} timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let stdout = fs::read_to_string(stdout_path).unwrap();
        let stderr = fs::read_to_string(stderr_path).unwrap();
        assert!(status.success(), "{id}: {stdout}{stderr}");
        assert!(
            stdout.contains("1 passed; 0 failed"),
            "child did not execute {id}"
        );
        assert!(executed.insert(id), "duplicate case {id}");
    }
    assert_eq!(declared, executed);
    println!(
        "migration JSON replay: {} declared = {} executed",
        declared.len(),
        executed.len()
    );
}

#[test]
fn migration_json_provenance() {
    use sha2::{Digest, Sha256};
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(
        fixture["source_revision"],
        "4e582f284a639bdaa01260b6b8ab6000482e6c0d"
    );
    assert_eq!(
        fixture["source_sha256"]["internal/migration/migration.go"],
        hex::encode(Sha256::digest(include_bytes!(
            "../../../internal/migration/migration.go"
        )))
    );
    for (name, bytes) in [
        (
            "generate.py",
            include_bytes!("../../../rust-tests/parity/oracle/migration-state/generate.py")
                .as_slice(),
        ),
        (
            "probe.go",
            include_bytes!("../../../rust-tests/parity/oracle/migration-state/probe.go").as_slice(),
        ),
    ] {
        assert_eq!(
            fixture["generator_sha256"][name],
            hex::encode(Sha256::digest(bytes)),
            "generator drift: {name}"
        );
    }
    // This capture identity is deliberately separate from the editable sidecar.
    // Regeneration is an explicit review step, never an automatic test update.
    assert_eq!(
        hex::encode(Sha256::digest(FIXTURE.as_bytes())),
        "3b6839287256b08d74e891203ed663a88fdc9851c6779d41ed5c9397f8a84d9f"
    );
}
