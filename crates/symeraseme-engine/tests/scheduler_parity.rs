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

use serde::Deserialize;
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct LegacyCaseFixture {
    content: String,
    is_python: bool,
}

fn frozen_fixture() -> BTreeMap<String, BTreeMap<String, String>> {
    let document: Value = serde_json::from_str(GO_FIXTURE).expect("valid Go scheduler fixture");
    serde_json::from_value(document["cases"].clone()).expect("fixture cases shape")
}

fn frozen_legacy_fixture() -> BTreeMap<String, LegacyCaseFixture> {
    let document: Value = serde_json::from_str(GO_FIXTURE).expect("valid Go scheduler fixture");
    serde_json::from_value(document["legacy_cases"].clone()).expect("fixture legacy_cases shape")
}

/// Rebuilds and runs the committed Go oracle, returning its full live output
/// document (`cases` and `legacy_cases`) as one raw `Value` so a single
/// oracle build/run covers both fixture comparisons below.
/// `scheduler.Generate` and the fixed-content `DetectLegacyPythonUnit` calls
/// perform no I/O beyond the oracle's own scratch temp dir, so a plain
/// bounded-wait subprocess call is sufficient; there is no child process tree
/// to isolate or kill.
fn run_go_scheduler_oracle_document() -> Value {
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
    document
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
    let frozen_legacy = frozen_legacy_fixture();
    let live = run_go_scheduler_oracle_document();
    let live_cases: BTreeMap<String, BTreeMap<String, String>> =
        serde_json::from_value(live["cases"].clone()).expect("oracle cases shape");
    let live_legacy: BTreeMap<String, LegacyCaseFixture> =
        serde_json::from_value(live["legacy_cases"].clone()).expect("oracle legacy_cases shape");
    assert_eq!(
        frozen, live_cases,
        "the committed scheduler fixture has drifted from current Go behavior; \
         regenerate rust-tests/parity/oracle/scheduler/scheduler_cases.json"
    );
    assert_eq!(
        frozen_legacy, live_legacy,
        "the committed legacy-detection fixture has drifted from current Go behavior; \
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

#[test]
fn rust_detect_legacy_python_unit_matches_frozen_go_fixture() {
    let frozen = frozen_legacy_fixture();
    assert!(!frozen.is_empty(), "legacy fixture must contain cases");
    let temp = tempfile::tempdir().expect("tempdir");
    for (name, case) in &frozen {
        let path = temp.path().join(format!("{name}.unit"));
        std::fs::write(&path, &case.content).expect("write legacy fixture content");
        let actual = scheduler::detect_legacy_python_unit(&path)
            .unwrap_or_else(|error| panic!("detect_legacy_python_unit case {name}: {error}"));
        assert_eq!(
            actual, case.is_python,
            "case {name} diverges from the Go oracle (content: {:?})",
            case.content
        );
    }
}

#[test]
fn detect_legacy_python_unit_treats_missing_file_as_not_python() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing.unit");
    assert!(!scheduler::detect_legacy_python_unit(&missing).expect("missing file is not an error"));
}

#[test]
fn detect_legacy_python_units_filters_non_python_and_missing_paths() {
    // Mirrors internal/scheduler's TestLegacyPythonDetection: a Python-era
    // unit is reported, a Go-generated wrapper and a missing path are not.
    let temp = tempfile::tempdir().expect("tempdir");
    let legacy = temp.path().join("symeraseme-tick.service");
    std::fs::write(
        &legacy,
        "ExecStart=/bin/bash /home/me/.venv/bin/python\n# Generated by symeraseme generate-scheduler\n",
    )
    .expect("write legacy unit");
    let go_wrapper = temp.path().join("symeraseme-tick.sh");
    std::fs::write(
        &go_wrapper,
        "#!/usr/bin/env bash\n# Generated by symeraseme generate-scheduler (Go)\nexec '/tmp/symeraseme' tick\n",
    )
    .expect("write go wrapper");
    let missing = temp.path().join("missing");

    let units = scheduler::detect_legacy_python_units(&[legacy.clone(), go_wrapper, missing])
        .expect("detect_legacy_python_units");
    assert_eq!(units.len(), 1, "unexpected detected units: {units:?}");
    assert!(units[0].is_python);
    assert_eq!(units[0].path, legacy);
}

#[test]
fn scan_legacy_python_units_reports_known_names_per_platform() {
    // Mirrors internal/scheduler's TestScanLegacyUnitsAcrossPlatforms.
    let home = tempfile::tempdir().expect("tempdir");
    let launchd_dir = home.path().join("Library").join("LaunchAgents");
    std::fs::create_dir_all(&launchd_dir).expect("create LaunchAgents dir");
    std::fs::write(
        launchd_dir.join("com.symeraseme.tick.plist"),
        "python3 -m symeraseme",
    )
    .expect("write tick plist");
    std::fs::write(
        launchd_dir.join("com.symeraseme.poll.plist"),
        "native symeraseme",
    )
    .expect("write poll plist");

    let home_str = home.path().to_str().expect("utf8 tempdir path");
    let units = scheduler::scan_legacy_python_units(Some(home_str), Some(Platform::Launchd))
        .expect("scan launchd units");
    assert_eq!(units.len(), 2, "launchd units = {units:?}");
    assert!(units[0].is_python);
    assert!(!units[1].is_python);
    assert_eq!(units[0].kind, "launchd");
    assert_eq!(units[0].platform, Some(Platform::Launchd));

    let cron_units = scheduler::scan_legacy_python_units(Some(home_str), Some(Platform::Cron))
        .expect("scan cron units");
    assert!(cron_units.is_empty(), "cron legacy scan = {cron_units:?}");
}
