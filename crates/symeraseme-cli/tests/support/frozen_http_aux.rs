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
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "70d51e62b83f00ad0eaf336bef922fcc65a26fff0784e4b12639930b522e58fd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "3e7237d1b1498154ab741582b0e31b1f7afebec35216f51a5a6268d5b118ad1b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "dab55b2b452ec72d575cfd5cdbb8d6098f27ee26e122976ccc0d185cd4d8a559",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "0645ed14819c68db82c77a172770c2127ec9ea2818e0766d4912b8367f36ef7b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "3594c9aee68cd5c3184c78e90b469b45c10fa2d8724cabe1ec4a184e246505b2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "5944fa92e75228cb8742d52f10201f2ba50e557332e733570f87bc2fb9b65336",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "707b1606ef4be7b4ae7008da573d3881d2ea3a26fe59001e970d3c17607aa186",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "876cd31e578856cf3d67e1604947cf0be55714dd785cb15e9968c7919e3d4f80",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "ddb46d578b26ce2a7f5c97a94d316252968ddb4306df5851c842c6f035a74eb8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "025e1d1ba407950871f6d0ea21cac4d487cfd5edeb081bf9443aef4647382a83",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "949f593f05b139be66d93e91c31d050110f6184b737060f8b39980bf09f198ce",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "a6db036a52830e941781a47657d83b9ae6f16f223181a6cf15af099be4aca016",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "80fd40384fd73350616fa8ecc5876a0285d20c430f434ea2412759dc26d99c1e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "5274b193e43b38148cdcb7fe188c240cd6e3f314ff7654db9f2bb341357a0bb6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        "native_bind_failures_match_checked_out_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "65e936fa2009afd718b4cec773393b2526017e69e89f94b7662d593d69551c50",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/windows-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        "native_bind_failures_match_checked_out_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "883f3c15308e8bdc3038e8485da46356c2cf3c6a29729e1dc1d35e70e4a6253e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/windows-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "35ea3603d29e6e4950530a412533fc0138678c14dd80de9b0558751ddcb1cae5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "3139f8af8edee69625ff7e7984e0b45252dd88317200a1e17568c92c54da54d1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "279a97284ccd52de66d9cd91343b348e856f9baa504e5cc896af83c4f8c9aa4f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "660fc132a0d3b7e52e59e8129463d29a6db9b1a44e3ad9cffe579d800ad6c671",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "062847acc8974473189b89bde68a984f1b000c723cfb8c2e1418e001f74bfd7d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "5cfc84231691f3cf444f8bea1e9d5b10d54822250e7b3c101fa688f28a4eccab",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "d318cbbeff51dac403728b2e326c69f1e9a8ea7f0b4207109b98780df8388916",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "26f2b62c8e0c7a4ec255669f430058b40759cdc69ee744c48c286ea863214472",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "9de34c94f0494abd2e875afb9d046e653af439a1a646e1cbdc88edd5bc77167a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "native_bind_failures_match_checked_out_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "38f2dbbf9d322c0e43496a909379f7f4d8500fc0a01debe3f24a66be61b0647f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "729beda219b7a768f8fba160b3c78081cb570aa1559c65ff341268f90bb09d3b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "6aeb120e838e6a7a4b165df254beba6ddc37ab6116ea24802ed24c9a45073b04",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "0adcb00da1b02f989a5cd99f92fd4f1ed1fd75c8099f4a95016539b777a6267e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "7a6679334f19ff41b12a51d2d50231b349f679a695fd21b515adcbf73c0a60ff",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/http-aux/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
