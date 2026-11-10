//! Auxiliary HTTP process comparators backed only by measured native Go observations.
//!
//! A target without a record keeps executing actual Go, exactly as before.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path};
use std::process::Command;

#[path = "go_source_pin.rs"]
#[allow(clippy::duplicate_mod)] // mcp_http_process also loads it via frozen_http_wire.
mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const SOURCE_ROOTS: [&str; 4] = [
    "cmd",
    "internal",
    "rust-tests/parity/oracle/agent-cancel",
    "rust-tests/parity/oracle/provider-cancel",
];
// (target, test, source revision, whole-record SHA-256, record). Populate only
// after retaining and independently verifying actual native captures.
pub(crate) const RECORDS: &[(&str, &str, &str, &str, &[u8])] = &[
    (
        "linux/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "6f2faf50323a6b6a79745f3e19f8f21d4aa1f0f84bae203e8f7b747848f7ae86",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "1485bdfb78729497b66519b20d3fd2b88d09fa3a46d630fab2cb2343f34c7127",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "f7d61a616bfc3fd9a79891eafa375af8c2c4efcb475474c901c7bfa0af7ad93b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "c6b4de840326a39d2fa19ed9978afc0dc76d42ca9038febb29600af22c4e84e4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "924bb78a74c461334e6f9cbcc65fc3c4643cafaa678c5235c46d61f0d5522fa1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "e45199efcfcc2dabc99fcef16e5ed71730bdfc0e1bfc4285a4a55ca570a5b7e6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "1ffebde37ca2e7140aa8613ff5a50f883b6c34c28592ceffa89d25d07eb44096",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "f47524fcd8060af70014eb291dcaa312c0a470e4e7ec9f36e62545d8dea4d040",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "1e2d9897104fa6c1d43d4dcd8bc9f6d1cd6af99f373fc09a09864d664afa79a6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "74e26ec06e501bf2f6d63705579147cc035b892b1e51475a0746d793ec7bf189",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "6c02f5c8a19afd1052c612820c2c18d06cae509a690c5c3f809a8b635f2a2d25",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "54ac87e5e08630a573d43b8d1ccf8fb05ae91039c198ffb97691f04a07e9974e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "76d5eae8cf3e1551cf91386e5e33ef76ba88217a5d2317fa0f83ecff89a48e0f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "c962e069b3276d5846c691864c6f548d77b4d5ea37bb8c8d9ef3ce57bc154866",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "ca1563427461e859e3607579d55dae4eebfff1ed37ec9a7c1ab991bcb02453a8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "00254d8c5b689d567a4286eb06e149b4016367767b23b8bdc6b6818c1d7db33c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "5ac48cec5e458454f21817f0f171a1d73f86bf3795cd43491ce3e86e27ddd401",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "c0a54afe08f748fa007931c437d3c6b83e764d1042a28875275f3d7adf7711be",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "51b168712067845023ad0951c6e6814d23dd0ff135fa9ecb0d2a92fc9449489f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "631dbc0fe78178830034431eaa4d28c22a023b193545155a06cc3ff3a606af92",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "629abd891322bed05a7624f333d2c1c4e4dcb01a8b0a3eb0172c4a2e1a775439",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
];

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn git(args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(ROOT)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "immutable auxiliary HTTP producer must remain verifiable"
    );
    output.stdout
}

pub fn native_target() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux/amd64",
        ("linux", "aarch64") => "linux/arm64",
        ("macos", "aarch64") => "darwin/arm64",
        _ => return None,
    })
}

pub fn verify(
    raw: &[u8],
    pin: &str,
    revision: &str,
    target: &str,
    test_file: &str,
    test: &str,
) -> Value {
    assert!(raw.len() <= 1024 * 1024);
    assert_eq!(digest(raw), pin, "whole measured auxiliary HTTP record");
    let record: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(record["schema"], "symeraseme.actual-go.http-aux.v1");
    assert_eq!(record["source_revision"], revision);
    assert_eq!(record["native_target"], target);
    assert_eq!(record["go_version"], "go1.26.6");
    assert_eq!(record["test_file"], test_file);
    assert_eq!(record["test"], test);
    assert!(record["observation"].is_object());
    let build = record["embedded_build_info"].as_str().unwrap();
    let (os, arch) = target.split_once('/').unwrap();
    for expected in [
        format!("vcs.revision={revision}"),
        "vcs.modified=false".into(),
        format!("GOOS={os}"),
        format!("GOARCH={arch}"),
    ] {
        assert!(build.contains(&expected));
    }
    let autocrlf = record["checkout_autocrlf"].as_str().unwrap();
    assert!(["true", "false", "input"].contains(&autocrlf));

    let mut sources: BTreeSet<String> = git(&[
        &["ls-tree", "-r", "-z", "--name-only", revision, "--"][..],
        &SOURCE_ROOTS[..],
    ]
    .concat())
    .split(|byte| *byte == 0)
    .filter(|name| name.ends_with(b".go"))
    .map(|name| String::from_utf8(name.to_vec()).unwrap())
    .collect();
    sources.extend(["go.mod".to_owned(), "go.sum".to_owned()]);
    let generators: BTreeSet<String> = [
        test_file,
        "crates/symeraseme-cli/tests/support/mcp_http_port.rs",
        "crates/symeraseme-cli/tests/support/capture_http_aux.rs",
        "Cargo.lock",
    ]
    .map(str::to_owned)
    .into();
    for (field, expected) in [
        ("source_files", sources),
        ("archived_generators", generators),
    ] {
        let files = record[field].as_object().unwrap();
        assert_eq!(
            files.keys().cloned().collect::<BTreeSet<_>>(),
            expected,
            "{field}"
        );
        for (name, recorded) in files {
            let path = Path::new(name);
            assert!(
                !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
            );
            let archive = git(&[
                "-c",
                &format!("core.autocrlf={autocrlf}"),
                "cat-file",
                "--filters",
                &format!("{revision}:{name}"),
            ]);
            assert_eq!(recorded["bytes"], archive.len(), "{name}");
            assert_eq!(recorded["sha256"], digest(&archive), "{name}");
            let retired_producer = field == "archived_generators"
                && matches!(
                    name.as_str(),
                    "crates/symeraseme-cli/tests/mcp_http_process.rs"
                        | "crates/symeraseme-cli/tests/mcp_http_native_contract.rs"
                        | "crates/symeraseme-cli/tests/support/capture_http_aux.rs"
                );
            if retired_producer {
                let producer = git(&["cat-file", "blob", &format!("{revision}:{name}")]);
                go_source_pin::assert_retired_generator_matches_archive(ROOT, name, &producer);
            } else if go_source_pin::current_tree_bound(name) {
                let current = go_source_pin::current_source(ROOT, name);
                go_source_pin::assert_current_matches_archive(name, &archive, &current);
            }
        }
    }
    record
}

/// Measured bytes written by `capture_http_aux::bytes`.
pub fn bytes(value: &Value) -> Vec<u8> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(value["base64"].as_str().unwrap())
        .unwrap();
    assert!(raw.len() <= 1024 * 1024);
    assert_eq!(value["bytes"], raw.len());
    assert_eq!(value["sha256"], digest(&raw));
    raw
}

/// A recorded startup failure, with this run's port restored in its diagnostic.
pub fn startup_failure(case: &Value, label: &str, port: u16) -> (Option<i32>, Vec<u8>) {
    assert_eq!(case["case"], label);
    let stderr = case["stderr"].as_str().unwrap();
    assert_eq!(stderr.matches(":<port>:").count(), 1);
    let exit_code = match &case["exit_code"] {
        Value::Null => None,
        code => Some(i32::try_from(code.as_i64().unwrap()).unwrap()),
    };
    (
        exit_code,
        stderr
            .replace(":<port>:", &format!(":{port}:"))
            .into_bytes(),
    )
}

/// The recorded Go observation for `test`, or `None` when actual Go must run:
/// explicit capture/live modes and targets without a verified record.
pub fn observation(test_file: &str, test: &str) -> Option<Value> {
    if std::env::var_os("SYMERASEME_CAPTURE_HTTP_AUX").is_some_and(|value| !value.is_empty()) {
        return None;
    }
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => return None,
        Ok("0") | Err(std::env::VarError::NotPresent) => {}
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
    let target = native_target()?;
    let (_, _, revision, pin, raw) = RECORDS
        .iter()
        .find(|record| record.0 == target && record.1 == test)?;
    let record = verify(raw, pin, revision, target, test_file, test);
    let mut corrupt = raw.to_vec();
    corrupt.push(b'!');
    assert!(
        std::panic::catch_unwind(|| verify(&corrupt, pin, revision, target, test_file, test))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| verify(
            &raw[..raw.len() - 1],
            pin,
            revision,
            target,
            test_file,
            test
        ))
        .is_err()
    );
    let mut changed = record.clone();
    changed["source_revision"] = "fabricated-source".into();
    assert!(
        std::panic::catch_unwind(|| verify(
            &serde_json::to_vec(&changed).unwrap(),
            pin,
            revision,
            target,
            test_file,
            test
        ))
        .is_err()
    );
    Some(record["observation"].clone())
}
