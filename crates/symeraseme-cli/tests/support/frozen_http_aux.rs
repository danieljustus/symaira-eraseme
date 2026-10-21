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
        "e61132f85b89580c96d95d098289f7238e810255",
        "19f591f0db768cc3dd7f5cb5f6e6b1ec59a0604e99348f527781d169be7901ce",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "f224b355d562c33fb3c45d03a828b1f32fa9b7c090db761cd6ef046ac240f058",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "2a8b3c236e1f6075a59a56f5f0c9389fa04ee41534b68885055891bebc2fe821",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e61132f85b89580c96d95d098289f7238e810255",
        "e9fa02e94c7ec455bf716921cfd3350c930ebf893a70ae6ec6e6647ec2e29cfd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e61132f85b89580c96d95d098289f7238e810255",
        "ac2d0e9a7e38d87765ebcf27c73fa409efdbfc698fb8ef53b085b2fa54b7014c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e61132f85b89580c96d95d098289f7238e810255",
        "5e7edd7fca8b5a27e2952cc6b47344a8b4133dc1ffa8b3bb7fcbfdcf2613125c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "30e59575f20a1c1ac6c98eb3c40054324bc32397747afc3a987c5585171d1305",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "5e68174ec462a6725569b99350650e193101830eab416374f962cde88cb97c78",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "9d453aadf35ba1d7355089ff4a8b6c88acb6f463e9bd215c13326d781b79a345",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "c7f2223035022afe8da0fcf51fbfceac94f976ede30ed26b665cf768d946233b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e61132f85b89580c96d95d098289f7238e810255",
        "8f207a906f05f7604b3fb5b223ab49b42c3704cb0dfa231637816fa46d832898",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e61132f85b89580c96d95d098289f7238e810255",
        "0045189d2b8a54d28b88bd88c416abea8b00db4e85973ba31a6c2207e6721482",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e61132f85b89580c96d95d098289f7238e810255",
        "eb2a0700e59156fe8920bf9c64f43216956408b9d73322ee09db73738fc5c1a3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "a2e6200c5a37ff0f10b53fdecf1f9a1931609525da736a407a3d760a79725198",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        "native_bind_failures_match_checked_out_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "9c06aa2e3c89d3f6a978c11548a98b8f2fb70455d036557cbe694c65900c79a9",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/windows-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        "native_bind_failures_match_checked_out_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "4ef5b00eeb521f3ac8e125895b07d6bf4eb126b9b617e674aad963b598fd57b9",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/windows-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "35bdcaeb84e9ec272052644eaca05275c92fc79945b94234de3fd1430e8484d2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "11bab0a053d6d0e28e1f99f4e42603b95f3d0c2feaf84925780981c8baa5c0cd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "e5f8f0fa2399892485b61b7f0e4853879a3c46a19ed3efec8b60bac260439dae",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e61132f85b89580c96d95d098289f7238e810255",
        "d83ef97399d73723d0e3e2c9b0bec96fc5225d1d8a117834f28e0f12ea45e833",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e61132f85b89580c96d95d098289f7238e810255",
        "34873fbf1e9c2a9fe8da7ddd6ec7747e5561e031ccc7426cfd5a3ad36d6b635b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e61132f85b89580c96d95d098289f7238e810255",
        "2d605bbb93271f29f7f602fa9df0e1a453b867bb711ece61818080c907a53b4b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "8d7f4850b7341488f83dfd5b185ae6df18d8b66fe695b8f18156be028a7ad4ae",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "08fbf6608416cc995201e74abbec6ea0337cc20d67f52fea3e22b62b66ad10e1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "3f2a53fcb320e7eaab6fad84e9d5c0b2746b9736aadd2c62bde8002de3cceaad",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "native_bind_failures_match_checked_out_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "85c3936803ac87159c0a533a78006bee999048f930be1cc9a6e84a65f6cdb412",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e61132f85b89580c96d95d098289f7238e810255",
        "8224c738b10b9e8ae0ec8f418fe125f3511e7d4fa82a9e159e448085ec0131f7",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e61132f85b89580c96d95d098289f7238e810255",
        "8c45c0b733962e1e4f4eff4a142e148bf3c19167c239e74b27d37fab090b6426",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e61132f85b89580c96d95d098289f7238e810255",
        "24721165869a55e1fd4c8b7fc5303965be8f11f6bc512a5a374a3b69a13ef4a5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "e61132f85b89580c96d95d098289f7238e810255",
        "ab56c3bca46da21a31d22e93a00d990b6acd78114e25338a59ff0b19d60eed95",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-aux/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
