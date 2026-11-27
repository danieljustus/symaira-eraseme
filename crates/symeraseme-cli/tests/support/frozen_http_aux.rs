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
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "1119f8498a93e89308cdc29c96e52e71fc80d7de90e050095f751533f48b1bb9",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "f58272f10b7421aac467ab609e486212be4a51e8d46e2931a6882363df2124f0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "e3788433f70c6bacb131cb5014054de4d365d43807d4e3adfb014bc3bd1d95e2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "6ed20e0c7c3b731f2793a315638704f9aff096f1c148f2e2616b33129e2e6749",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "71fae0bed526385ac12712cda0da20d712d76867a0c9bc18558fac1bb424b19f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "b960dc6e3bb546a4fe54bcd2dfd92290bf533cc003aebff5554b75a2284cce5a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "338f312bcb1c8b8b5c0a1b458217492fca86ad44a6bab4a2451e246663a3897e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "bc0eccc9b1f782926589dcf12c83f961b367c8005e63cce256f3395741ecb6d4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "3ee0b17bc3b5754c04af726e98e506f08da0a6e20aadfe6fa3001bc7db6c677c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "e7e8662804c475c77d547b7ed6b773034012f6cab63751fc05dd41df34f0ceec",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "e8b508e29ae9d99cd3115ecb308dbf87a6109ed8ae8bf3c6a5777383df8731b6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "f917960a8cfb3bae6afa428a334b9fe153a10c407089f84e582ced82ba73d5fc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "23f7639aa62144aa7978816e2067bc88d6c789d1ffac5a7d4a8e07ce88bfe142",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "0da127a1d5855ee47aaa086acb06375c9f3b30ad092dc9d2107251107ac0dbc1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "325c147e933521606352be58f3a4dc8c3d01920d052ddd843e58088b8a0e4558",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "031727c0b3b2fc57cb1fcf5f6d25b0b35645982d3487f5558f107c4e55e874c8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "5da32f7e4415dd73067c945c866e83a6274e2ffb115d81c2256ea09f622c4161",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "19f2b249c2e15271cf1d76bf8f94a7345a31d8e0c511ddb10db477b84a97d9be",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "2b08e9dce5255cbbf966b57bd79e4d35b1873cef5b2dd40b19e8c2e007504a12",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "385be04fb1555a9e0e9166d23aaf2bf5ad310c35268b7fe39535499e39d33232",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "17c7d69658eeb22c4ac96a3c0d4facfb6a372274",
        "6686e7fa08a31d8fff5d16f8fb6aa8c9ce7069c4817fad7de6b92d009c8ce857",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-5f5b27a2/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
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
