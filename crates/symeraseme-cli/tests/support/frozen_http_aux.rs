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
        "286d43fd9b448c574007ad98534125193f7e732d",
        "4dac1c073618cfdde1b0e8fdf789fb2d139904867ce001694a6340b45905b68f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "6552dc611903786107e2085c09069552a9483c134e2243c2bdc0ce4495d9b71d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "a453dfad546a6b8cd944f385d2a5e79d569f2a8ea6ec0414a82abf665b937c1c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "72eb08c4d29e9efd58321a57ff3b89a17a3b4d4226ccba341b16ad130d7dab11",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "36e74aed6777354def38f3d957277c8983c2e39712c58521ece9a790bda0098c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "d91ff0d556f38174b7a1cd16ceb7ac1bca06838173ea7113a0ff0f3fa8ad352b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "07dcc749f3777a6d8b73f946c6e3cb656474c57032f709f688f52e9ad72848ca",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "e50d396f81fb8e4eaa1239dce46b13e065be7a6de5c48ba5d00341290509dd5d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "c0b266a52968ef1cfdb81e0b9fff97693108aef236d2ab1b8edd28ed58b9d1a7",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "873817d32ba4afa475b88cb9528963c74ab41b53e3eb5f18a89e509a2523d87a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "08cf29689138cc4e7774628d0c9aa576713bdf994c20d7aedfa08c9f37d71eb0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "51dc1390d6d137f9b8374cff162b9d9e3b48ee33217725c201bc7bf1b563dbe8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "c3d7a8989b9af9c475e0e720bbae7d1e00577174664e6f0c15903d0ffd00b463",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "a6ed0f4ef5fa6015eadb8c6967d263f8aeabe2431a07994485e22c693354b780",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        "native_bind_failures_match_checked_out_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "16c635fd2f7da590ee51696ae417457885ce15606a9306a849861938bbc1ac02",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/windows-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        "native_bind_failures_match_checked_out_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "0e49bb654785aa20918113139f9abcddf01106f5c37ed000babe9c2738647ccc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/windows-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "446ce94820a6fce61d87e020966b9aefa810bf38c94cfb6d1ab369b335fb3195",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "78c76f7dd4359982a57093f5bec584d6d480f13f0641a20b5b7026eb992e9562",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "2f707e459782af506ba05c7e563a64cd705e0187f55aa58406493d4b360af912",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "4af145f6a383a8d47b5b519869877c9fbd064f0ae66cef1ba55df56875714057",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "d66c6e17834976c1c48f99e3e0bb57df0fb15dc74bb95836feb98d31ceb75a6b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "a1690ed6c657721b2bdea694f27fde8d03b23890a3601aafaf560430dda62065",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "20c98bbf666abcdcf41740ff755b9e871ba30d46fa9c9aaf278a744fe0014f04",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "2d2d0abc651a032e0e35be34c13ae51ef9b323aee525b11e1da6c1da61e81a27",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "a605e73bc3398ab84127ff3127127167902d76260af174b1507e8801c4ee23f4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "native_bind_failures_match_checked_out_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "c388c2ec40a8d2c98c3c85bfd0b0ef8927bf9845413811df345f3dfa7e9616af",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "da199e6f2386b9bb0f7663e844e6e3075985811b82ef1809a116015d6aa34878",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "cddd46e7f00568262bfc25034b430f14d11cbfd5c6c264992dced500b235871a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "a851c8c36fe0d7e70f61c2b4a8aa010c6d4490c95358d96e3e2ae6309af3ed3e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "286d43fd9b448c574007ad98534125193f7e732d",
        "cd7264c1a313458f0dcc4bad8edc38b4a92d5de292a2de7678d2cfdc6a683308",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-aux-286d/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
            if go_source_pin::current_tree_bound(name) {
                let current = std::fs::read(Path::new(ROOT).join(path)).unwrap();
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
