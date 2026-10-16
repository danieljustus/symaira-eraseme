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
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "210b8ca20a2df699a4d65d5d3e5b3c4c7adf044b53bbaf3c5cebefc4e85102d0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "e15e5c180775022835747bcf0a0a53246012b9948906be61b35668456bb1fe23",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "a4cd0c4ec40c5b59a1d31d8e7465b951ce60821c7d45481d745f5445c339c1b1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "bc229f84c25ab573f88823e0778867dd159cb3b0882e82623fde20d4dbd1e952",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "e9096c7b0974b2889d6e388b4d62f311812e5294066de2cc5b8714bb7cfbf32c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "1a665deba77997c95c5daca16a5200c83b5b2e973f93487b85348b27f1321f27",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "f15e5dd14f0893d7df631dc645fee5a7e3550cea53322ba6062260da5ad1004a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "3bd4153f0e21abffb47630864c420009997d88c3860b07b0b4b9eb6af6d58e39",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "f066bd416be38a33917ebf9d01e27040c91ccc396959639f4522df57f612ae06",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "c1185e147700eef8f9ba52d62a194a2e78c97889a6043cc861604b51a0ce7ac2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "1ac055e6fa0733aa31f5a03e562f65faca56676b216097197c66504f183cb25a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "cccb88ba7421530d4ef52469e0a047a6fcd87f3021e883d6252d49bddffa1172",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "8f7608c429b9daf7fba7a2f919ff3394a4514eb88b5976cad2223b29b121af1e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "e7d9ead292f7694ebb5ef09dcba10d279ebf2b2f5f3d6217e53e5e821b0fca8d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        "native_bind_failures_match_checked_out_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "84529321df55e4f8082d03aef6dde529088dbdd1a252506297a63618b80d584b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/windows-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        "native_bind_failures_match_checked_out_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "1ff0232990d07c04696ed152be6de60a33873b0413ae5958d54fc7e29f6e19c2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/windows-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "7dce1a9519caefa72820df58c20880f7b2aa677def803df57a78b6c692e6dfec",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "7df33d73d4d2ba3972a89106c16d51531893e1bf89230698f1617c9faf75655f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "3196ea6c44b25e9ebcb32e3167ecaf2920a24c0c439d61862cd9027622675a3e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "65e1b33943ffee03465e641db3b053053d1ef00bfe0176660f1d18e83a2eace5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "676d8f63d9676eaed7f62f45f58f69b6cf8827b0912cf88ebbef13b3aed4d4e3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "1f42b7f2b9320fa129272feb9656079af1201240cb4e4f9980532ffbb632aa51",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "a578164fd567b56d81920a0c88c8606017473784c041c68a861924415fea9bda",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "86a535571d318529010e05ae3405f30f5e02382a83eb80b1af3df96a8bfb8e75",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "bae29c4f43897eb38f46f12ef8b0b10227b0396a6cc1b758b146e61c00df9610",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "native_bind_failures_match_checked_out_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "528b13a149681108265e6a15aa85ce41ea608ff3dc74e6a40b65b5e2dd890b99",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "ba623d681e407eecf99fbe057b3aba748711299a293eeb007efeb8ef8786d33d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "1afbd00f5a886f641c315426eaf4455e9b79c9fd934b58928c2141749d5b7486",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "777979de1d7634befa2272a702c04c24f3afcbd52897eb4653875258db1a23d6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33",
        "e9f9a162b14f6c5a0275ca7fb8695787528939b2d04b0f4c303480599208551d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/http-aux/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
