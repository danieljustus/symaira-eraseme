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
const RECORDS: &[(&str, &str, &str, &str, &[u8])] = &[
    (
        "linux/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "4bcc69143d29556560eda81bb7e3d62ea1462e44e40dfaee279463f509b57643",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "844f56256feb7ed528e8afe8be68174aea6151cccc496d4d0df6413e4f75b2ee",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "42790eab779ff6fa18486a6718bc56c27dd86a27b923e47cdc0c052fd971e1ff",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "53cb7a4c20c63bd491f9d92418623755f1394bc739ce5c1bd27ff2ab0543b7f9",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "dfef1eaefa2b625264d2540b9d8da1b31b2531a90af5c51df0da9b4bf135018d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "580654c4a425ac87dd1eaa71df1298f21c54561e53cae0de83e256a518bb4066",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "5904b0a3389da131af16e389f080e3ac19cfb3e8488f82d7d7ea6a88fa35bbf6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "d1dd221c7ec91729b8fad0613e343bdce069d2496170ded0f3b6c0ac4ba0df84",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "1e5ca0c3d8b01c7d4488333b8d08990d0d69c769ac8bdcff25a24d2a7bcbcfed",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "9275dbd065c2cde4fabb17726fc4ab117959c306ecf1c1f3ba8a47f95c3272ad",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "d8e1c295065ea5bf7edb0db0aed9816a04aa8ed1e226ab2dccb16ab944667ea6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "1006298b1ec9adc8b8d2ae534fef6048851563a4fcd9e4b11281084f8d24cdff",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "1669af4042dfb6e6d7943c9674a2f9183a45398bd70804f3f24b6361f704c72f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "24c640e27f8d6d209fa3197142898fca155f0dec20cb29c23526d628ae52dabc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        "native_bind_failures_match_checked_out_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "f048203825caff9fe2e73788a83d450766d0a65f4cb30b746d10edb6cbfa93ab",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/windows-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        "native_bind_failures_match_checked_out_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "8487bfeaa5efad73201136b156cda7fb3dced2be639f54c81013b3f04967ec77",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/windows-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "8947e66cf817da8b6402a66f5e6d6974db4f9a4d529ab785f6b1ed9bb3f56ffd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "9ea2fc5cf0d2308253dc32220ef9992683f2f908e49b0fb7b7b9afafdce035e0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "df53f33a0091613ea3b854682efa2e5b541fac0e7cea234fc8bedccb73b509c2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "c7e75e269003d305dd248a04242b95010e4745911bc4e6ef7ec650d59c9852e2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "031365edb87a35f69d2420ba037ab2f8e448460928a036fb479000de78b395d9",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "0b52a609af8562e3a9e45384c9eb3b598e74c99e3b5b9c2fb12f62a159201e74",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "a3549613fc43a8f79be8f36a05c697ec1484e64c503db75e24d2ea7bd968124b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "bcb706c7f9f532e884cc8827521fa134d61d4d2d096320692cf1d01f6c31e91a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "d7befb08502d4bce74a4339fb1f3c2ff4bab489e984b017df2ffb3913e9bca92",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "native_bind_failures_match_checked_out_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "780283281214fca124e334f58b7a42d819216ad443cc5984750dbc6d4fdb0005",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "ccd25048861fcc7d4788968eee61da0924dbcd7cdad4a8631dc4167284ed2bdb",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "e315ded07e68a504fd0d43f98c67948bc3525d52f52a65873e327103251a16d4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "f5afe8551aa5a849af1014fd97b47d6e5fbb4b637521bd472273071a5bbd0c44",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "dff9503bdb890a8d7bfffef84e753b11064215d201d5613f7b0b6d0f9d42a248",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-67d3/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
        ("macos", "x86_64") => "darwin/amd64",
        ("macos", "aarch64") => "darwin/arm64",
        ("windows", "x86_64") => "windows/amd64",
        ("windows", "aarch64") => "windows/arm64",
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
            let current = go_source_pin::current_tree_bound(name)
                .then(|| std::fs::read(Path::new(ROOT).join(path)).unwrap());
            for bytes in std::iter::once(&archive).chain(&current) {
                assert_eq!(recorded["bytes"], bytes.len(), "{name}");
                assert_eq!(recorded["sha256"], digest(bytes), "{name}");
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
