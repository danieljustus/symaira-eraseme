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
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "3a8a9dfde14c4e96b592511cb324aab743b2aa53cf1f5476d791a1df41d58003",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "f76d17e460c58d576c92eee7ae80185f8368228236a421c6b0ec8dfc8b709506",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "f43d166aa0401f69731d5f1ca0162f4eda3245de2d56cd0fa42417f6062a6a1b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "2cde7724293a5d31903033fab53b4e0a22f7db2b3a53ea91e8acaa6ca8526200",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "8164965f2400c73fb2fd326ccfb6d962460b3da3d6ee9ac51a34759fa79cce47",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "0f092dc0d728b8e326829c53d49e374bc70674b60912b03a8ab1d86f4dbb5759",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "81734a183acd9fda52420b419b1cfbbb920c2e1f265d957a0bb58c210bb58e0e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "a8ce1adbb993fad0fb854b887cb603dee21600617309c6feb31ceca993e740cc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "572380a2b23ebf9eff9e9b0a05152abb2bcef3ca5d5851c7bc3be4d50556c2c1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "2e76c3f17395d09d081f238f7581aba2ad32f963b27f1739d869a9405765b59a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "da3abf18144467ad36990903ab2b79105fd6b400fac68dc2821c8827837944f3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "29b205fabd3800fcb173833ef802bb50676d89e48d2ed63e2ae7d8de4fa66dfd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "c891881e058958605da5fb29100434516c07557f8e12c8ee09e16a6b636d27e3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "7deb4eea188a93d9ec0d99efff91ecb86338e3bc6b795a67f46403fc14850639",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "56d8ef91d1a7bb4b799a2390ccf46bcd68a30c6df69e67585f77705bc9bbe983",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "cdd1a668ee9e622b5dbbf5e6e6f9081b87b58c199c98f4e4820dbcc88c87842a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "59f4d04858245428619c4e284597b434ed2ec7edddec43b737eb6e41b17510eb",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "9741ffbbe5bc65ec767b96f2c6fb9a0504a156e344777b3abdae67d1b260ef87",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "9517dca673d28b6c98d6518c1b6fb5b4a95c9a22c7ac96ad3a14da4957aaf99c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "8d0c3795fdb5d57b6b7db5f309a49ffbe416a31a2113690863d5209841cf3a72",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "d508140be588d8e6a4e2db30896e90b6d4468c9e9ec5b1bc32a1b9e7d4dc5a11",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
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
