//! Byte-exact parity between `scheduler::generate` and the committed Go
//! oracle at `rust-tests/parity/oracle/scheduler`.
//!
//! Unlike the config oracle, `scheduler.Generate` reads no environment or
//! filesystem state beyond its `Config` argument, so this test needs no
//! isolated-process sandbox: it rebuilds and runs the committed oracle
//! in-process-equivalent (a single bounded subprocess call, no child mode)
//! to prove the frozen fixture below still matches current Go behavior, then
//! compares the Rust port's output for the same named cases to that same
//! frozen fixture. Case configs must stay in sync with
//! `rust-tests/parity/oracle/scheduler/main.go`'s `cases` map; both sides
//! are hand-authored because `scheduler.Generate` takes a typed struct, not
//! a data-driven JSON input, in either language.

use serde_json::Value;
use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use symeraseme_engine::scheduler::{self, Config, Platform};

const GO_FIXTURE: &str =
    include_str!("../../../rust-tests/parity/oracle/scheduler/scheduler_cases.json");
const ORACLE_TIMEOUT: Duration = Duration::from_secs(30);

fn frozen_fixture() -> BTreeMap<String, BTreeMap<String, String>> {
    let document: Value = serde_json::from_str(GO_FIXTURE).expect("valid Go scheduler fixture");
    serde_json::from_value(document["cases"].clone()).expect("fixture cases shape")
}

/// Rebuilds and runs the committed Go oracle, returning its live output.
/// `scheduler.Generate` performs no I/O, so a plain bounded-wait subprocess
/// call is sufficient; there is no child process tree to isolate or kill.
fn run_go_scheduler_oracle() -> BTreeMap<String, BTreeMap<String, String>> {
    let repo_root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let temp_root = std::env::temp_dir().join(format!(
        "symeraseme-scheduler-oracle-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).expect("create oracle build directory");
    let executable = temp_root.join(if cfg!(windows) {
        "scheduler-oracle.exe"
    } else {
        "scheduler-oracle"
    });

    let build_status = Command::new("go")
        .current_dir(repo_root)
        .args(["build", "-o"])
        .arg(&executable)
        .arg("./rust-tests/parity/oracle/scheduler")
        .status()
        .expect("Go must be available for the committed scheduler oracle");
    assert!(build_status.success(), "Go scheduler oracle build failed");

    let child = Command::new(&executable)
        .current_dir(repo_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Go scheduler oracle");
    let (tx, rx) = mpsc::channel();
    // `scheduler.Generate` performs no I/O and cannot legitimately block;
    // this bound only guards against an unexpected hang, so a background
    // wait is enough — no process-group kill is needed for a program that
    // spawns no children of its own.
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(ORACLE_TIMEOUT)
        .expect("Go scheduler oracle exceeded its bounded timeout")
        .expect("run Go scheduler oracle");
    assert!(
        output.status.success(),
        "Go scheduler oracle exited unsuccessfully"
    );
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("Go scheduler oracle must emit valid JSON");
    let _ = std::fs::remove_dir_all(&temp_root);
    serde_json::from_value(document["cases"].clone()).expect("oracle cases shape")
}

/// Case configs mirroring `rust-tests/parity/oracle/scheduler/main.go`.
fn rust_case_config(name: &str) -> Config {
    match name {
        "deterministic_custom_cron" => Config {
            platform: Some(Platform::Cron),
            project_dir: "/work/project with spaces".to_string(),
            binary_path: "/opt/symaira/bin/symeraseme".to_string(),
            tick_hour: 6,
            tick_minute: 45,
            poll_hours: vec![8, 20],
            ..Config::default()
        },
        "all_backends_cron" => Config {
            platform: Some(Platform::Cron),
            binary_path: "/usr/local/bin/symeraseme".to_string(),
            project_dir: "/srv/erase".to_string(),
            tick_hour: 10,
            poll_hours: vec![8, 12, 16, 20],
            ..Config::default()
        },
        "all_backends_launchd" => Config {
            platform: Some(Platform::Launchd),
            binary_path: "/usr/local/bin/symeraseme".to_string(),
            project_dir: "/srv/erase".to_string(),
            tick_hour: 10,
            poll_hours: vec![8, 12, 16, 20],
            ..Config::default()
        },
        "all_backends_systemd" => Config {
            platform: Some(Platform::Systemd),
            binary_path: "/usr/local/bin/symeraseme".to_string(),
            project_dir: "/srv/erase".to_string(),
            tick_hour: 10,
            poll_hours: vec![8, 12, 16, 20],
            ..Config::default()
        },
        "default_poll_hours_cron" => Config {
            platform: Some(Platform::Cron),
            binary_path: "/usr/local/bin/symeraseme".to_string(),
            project_dir: "/srv/erase".to_string(),
            tick_hour: 10,
            tick_minute: 0,
            poll_hours: Vec::new(),
            ..Config::default()
        },
        "venv_activate_cron" => Config {
            platform: Some(Platform::Cron),
            binary_path: "/usr/local/bin/symeraseme".to_string(),
            project_dir: "/srv/erase".to_string(),
            tick_hour: 10,
            poll_hours: vec![8, 20],
            venv_activate: "/srv/erase/venv/bin/activate with spaces'and'quotes".to_string(),
            ..Config::default()
        },
        other => panic!("unknown fixture case: {other}"),
    }
}

#[test]
fn frozen_fixture_matches_live_go_oracle() {
    let frozen = frozen_fixture();
    let live = run_go_scheduler_oracle();
    assert_eq!(
        frozen, live,
        "the committed scheduler fixture has drifted from current Go behavior; \
         regenerate rust-tests/parity/oracle/scheduler/scheduler_cases.json"
    );
}

#[test]
fn rust_generate_matches_frozen_go_fixture_byte_for_byte() {
    let frozen = frozen_fixture();
    assert!(!frozen.is_empty(), "fixture must contain cases");
    for (name, expected_files) in &frozen {
        let cfg = rust_case_config(name);
        let actual_files = scheduler::generate(&cfg)
            .unwrap_or_else(|error| panic!("generate case {name}: {error}"));
        assert_eq!(
            &actual_files, expected_files,
            "case {name} diverges from the Go oracle"
        );
    }
}
