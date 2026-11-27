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
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "997bfc7d4afb651343f2328d9033a8da3120a881053afae3145cb31c1bb74751",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "2b2c4afc4a9d69d6a3f0500bb74e1f7ba18c5cbedc86c6534bc24ea2dfb79f76",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "ed714ad568d860c3d7f17f5f6b88192206960c8c5b7fb0297649da642c90ff04",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "23e99c0e80fd5066329ba1426abf96306e788e6e671f2d236bd1d1ab0809a196",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "628b2a8623eff7931eae43a0c8b80ad1504d429c05652167c55da0205b06a150",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "792ca54d2762b330d37b968937234fe01b23eb228a246ad48e336769564d7372",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "3a7ef5bfcb02919c52200d4ff1950a8c5ef9d87d2cece5ef276bbd9abb671b4e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "84c8d5a26920f457657cad83deb685d6c08f9ab6df6e746bd511c6f3b3170880",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "9f5d9cbeafc3a43d28b5d023046e9ae7a5c036607ab2bd85b0d9ed29c68cefc4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "76cca11169f8bc15fff1ccf1e3884494210995ad9b52e013689796ca22bb1d54",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "ee5aa9db6aa891487f63ac61df06440e55a922605485f2c27c9236b7f6e449ee",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "0923774e0912c0dd8a6e64b3067e1e817d1c000da84f189dfa4c0bc0be7bd7ae",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "a12b99a74a57a99e6e2d6e90804d09b1dbb727e206f375f3316206b7340674e1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "08ec8e904cc9ccbd6327bbe01d09c5f181a18fb56b4f2f0699ab15826b2c1158",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "164edc6f0c75dd95c7d0c7304647719e3f5ab4e23dd6e1fecb8a22e69a72a05b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "7174ed985b806ab78fa275f54dca8b081369a8d49dfaf38285e65c3bfa3cbda8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "bcb8ed0f2a1dd7f3104cfddfdeb98c0a9a3fbde107276d2dc30eec9a693c3cf0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "1ce0c0cb882c58558a1922dae11e8e785ec937526def90f0787e16f25ccea57f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "1be2acc77e898e385c7d41ff9b39b6419b2dbe676e1364698da49a9ea2097e5d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "27555babe66dce1e0d28babc084da5abbce3df05c2b398000a25d0e556d1daab",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "09d8269371cbd3111c8b6584a402caa8ff6ce6df",
        "78110f380e5054d3bf13167b0151cc0f72c5d27b35e3ee6719a9b1f63a733acc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-711fd570/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
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
