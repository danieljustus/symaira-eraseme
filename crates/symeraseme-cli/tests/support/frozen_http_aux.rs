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
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "066c1c0450812d341856c9972a452e6369d4236f85981acc43bcfb9f3d4800b4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "4b4cdd5a26caa7a7cb41262ed1dc65eb2844164ec1fe30730fce01b78291e612",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "8c63625d0dce65f6bf29b2828f2a6f75d128d574e7f11945c99150fb83a092c0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "58dae565c47bda0e503ecbb272cfbf9566e1afce5c8536ad3ceb133118b486a8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "d147ef0642adea5ab2c59a61293fb8e8b78282f4c2faadbc4776603b331e2c83",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "9de602d37d38d8988ba1bfc70aa40f9dbac6b1cc135f5010db4a12f6938e1d9d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "39e3b97eec133319c70831b49ed4e2ea81f34a4005f3dc892b469a7fede5779f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "9271ad060e44326aa23090fc285e6df8213061c333b1774c915290313cc3a833",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "62da66fb3e31753c639fcaa065493c13aea392e206836c3b5e7534d974be9fdf",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "d7a15eec5d2956e805258f37f0e0e09a5485d0d40ed7c140f89d084d7476589c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "2e7c742c9952492da1beed949afafb50be0919802ef0f95f7f592b9a5d60a704",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "357cea9b99ae1f31d05d026fbb5cae08068c46ace52ab1ef5ed9b8abf5731aa3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "96855c0e3f4a617f22dbc4c62c710c3f538332fe816acce6108cf049d2c6c948",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "83727cc9f5ed23f42f4e4a36b7b82d3754133c0f91cde49a7fa6ff6c3b873810",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "56825be906054036c264fcc0132b15d484fd98f31724981556c380d82b071371",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "c657c68c2912f457866f377bfa54a69356bc0311c199703d4faa298e30bb0593",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "67dec485d4d6ab79de9669c2223204a054aff695fd0d6ec6b8fddca5fe0a98be",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "a554258ba72a2db35dd778f36bcb4d93838acf1ef9da06623d14c9fd4a44aa01",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "eb584217de37d0c55d2b821b4b6c426f8451773b4cb96bd4b9477537bb97d1bb",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "2b576cadd605b85e914959891e0332be601fdc0987891a4926906e28a7c41361",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "fc446060e4346a5896b2c80f5788a5a92be58992",
        "5b9ae035094327798ada5bf8f9d0a2d8e1a05518d64fcd29484f047ded84e855",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-fc446060/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
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
