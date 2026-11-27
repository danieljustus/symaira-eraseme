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
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "d47c1955a2352b1996125f640fb17e227d011764a1e737356a1fee25b535cf86",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "05909e9b6b710154c9bcffbd0eae255059ede71fe45cce42ec8b940f81e78169",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "c7f8d4fefb6ae1d488639fefd2a9d94ab4a87abb9156ecd9b3b8dfa068c4650f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "e3c47e4ceacda59ef35ae84fa4965fc6458c432a7f7d49504e97cd6c9f6e8ce5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "abaf108c1c76142855b65efb829767c1135054443f33082db4a5de16f823fe36",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "188f88bbb938889e549155aa87334ac32e380db37345a77e8ec69abe1442417d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "af8a5cc4d0dbf31ba2cceba8d739359d1a199acbd3fc9609e6f46724c6714fde",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "3d822bb67c2d79b14f6a4521eacccc53638ae0afe7ad8f05cbb3e858f51f84dc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "999ee55d6124cd3431e3e3054d2e6f0ba99aa79e93bc4987d01dea434a853963",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "6ef376878091af3c21f985c34a022b843dfc094ef0343d9176db26ad68d53865",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "976b4276745be3f7daefe2eb7c042157a4e4ac18944d0eeca759403800f9e377",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "bc93a08d315ee9ae31a6f9b41e82b3c549fe506bfdddeb14475c6d6710ae09c7",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "b7fdd4d41368e91cf00fd5edc8090830f1ce8abc5992ec080a92c063329e8865",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "3ab658ab30f2312624ff36a38eb2d5c11a12d4e98b326d95177d4d06807cc771",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "5ef640342f58a3afbc0a8760779087c2bc8158691db666a3c21691c7db845836",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "a951bec4bf316a5d48fa244925e5db6dc2b4c63a27a17d45cbf37e12f1bad370",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "a357a79c8e0f818c2924dd759cf20a4c462c09e7a9b3d75a8cc4bff642dc32d4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "c05c4e104c2027d14503ea9c51b86ebf463cb9e273efa7101f72dee4f90e7433",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "b18547e3817f282e4da908c3d946c2779cb504c9f3c39e0da48c6d14b0eb328c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "b57c6a719af69b655cf2a5c8002ba05af117d3f98b059c891e1240e7443ab3ce",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "e0a4c294542b09f53ef8e8b7cd1585c574a8c365",
        "b4c541555c5892734e37ef0f473daa178f520d36c8fe41fbe96843b0481b432f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-8dc0b7bb/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
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
