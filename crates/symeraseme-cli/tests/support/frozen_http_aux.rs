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
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "0bdaf6c1125d0f2e5e85de9a817dc584fad1499091252406f95a53a1754d1668",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "19ef28bb42b5edb4a9a86c6436eaabbe03d68cbd89e93451124f253ea5460b41",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "8438bd8d1503135e5e6b8495147a372d5cd9883c3999c70877332c0797d7fa1e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "497b0b697b01ea4a8ae843c708492db082822abd4ba80b8db9bdf40658b0d8f2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "4bfbc48093febfe87094073e53d05d1de3b5763bf9145505b8f3cea7ac389ceb",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "10566525c175558e2259fcb1d6f0e95dc4731090a9c2797c9e5a76f0fe21e83d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "a19558589f36e980382adecf443da4d5d00696541406eac83c135a540e888cf1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "af22a596a9e7c28bbfde9ef22ef1fa4bb5ac58dc641437e2a9b0e8b21e02eb24",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "794a93177750324ac59e2e48cd6b7ad943d8d81705bf59d61858b3bbd93dc068",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "b7237220f71370583c475f72dc8c6c04e73cb6c65ea3012b10803736eb4d19c4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "e4469820edd00e321b0344e27b66de7cf6977a811e83d5326a332248daacf62f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "9f4be7727bdefe570f03d525dcbef7e0ba2fdda6a2c45cf66cd6a3d2167eea26",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "fdc1a1fe367e7e54fd8fbb32eee54b8805e5b4c6cd135516520f5282ce3e32ce",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "5bf564e06069dfffd0f46bc982a83c29287a82dab5549606324e69e45f5dfe63",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        "native_bind_failures_match_checked_out_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "52e744e55720e8b669737a22c20cf103b58c46a36fd5c1cea7ca8d54609d2a67",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/windows-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        "native_bind_failures_match_checked_out_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "224e79a5247dd525d8d41d023a680316e024d7fc2d955a3342800b8d341fd857",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/windows-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "46bea024a6cb34f8754c122075d75e5fce78d4a08a271d6fc0333a476a6434e3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "9a01f84c67e6f58b34d5e4f5de8a7a45817ccf256626bcc347a5c207df9bcf73",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "30e776173bfdfbb10743d646b6c976a2352e3e6a3ace74dc6b91849daae09f6b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "52a76f946944456cefb3d2d0919d2f67b98583743edbe840898a7c459ed52dca",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "767d71ced4c032957c7829ff9221095479a30bf20b2808f8b37b582e95d768be",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "03f014a72e74d009d95a20d0c9d9a7bb26b4263970f6f76dc5dcd1f7115c1db4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "b2087186dafc389e819f614984bb62df944ba5e91ddb311926b06234d51bbe5e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "15a640e7ba66098b625741aa2d10a5720551b085674f3fd5277a22d827221e28",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "a3977f905701eea723e471666dc78390af908c26857e271a7f8f4abe15ae8d6f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "native_bind_failures_match_checked_out_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "1a1a0baea113b6dbce5027a24a05fbf547315aa9db406f444aa311aabae2f095",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "cb72d6108e62be52c3df16a9d59f761f331354f2b3cbf9a4b4db2a4e14e5be9d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "ec481ed7e3cefe8c2e2f54691312c144e29e00f612a5fc1f643e8fa2ecab1d88",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "5ad9eea182106b68c8d3b788257a5b0fdabc0e2ec32f5a718c19e55f25366654",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "439db11407209ce7acde2c31101f356d0bafbbbcf416aaf3bbe6084fff9c66ee",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-aux/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
