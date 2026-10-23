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
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "8430debfbaa7e863ef4c42f1f894980596a16cf27a1457d75243af203c8c9c7a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "9cfa3be2c4b045e5bf579dbcaa5a0274def7a03507eb94ae68d4041478d49f79",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "0da56ccc2267333132936a20544c7950e9f36d3b8ad01450fb1846ffce6a7785",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "6a928279245f313d7ab184f1e3d2d330e9c843b6bfde154830c9d45c587f2614",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "6744d424a35ecde34b6e9c18c8ec7be3f93e5b8b01f0065805d4687e5d7b2d0c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "15c9b5676760a62f82e92089980da66ae4a43f2f646c1f5024f078956a06cbd6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "0c7a6b769f6e07233377fd8f25c17051ab86fd770909c6aed0dfe752322ec6ed",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "ec5fa4617282f8e1d1745507a7320992b8c75ea86da220db690b27032897d473",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "6c3774eaf6f39fff4deb098449dbbf81319ef69edbff081384259c78a4a9777a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "e67a3afbddc0b579061aa1f2fd8f77b3b040e1565cb5e3ea921cca8fd89d504f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "b7ce76d190211d70ed03101a172a2925c1d3663e2e2d6606bf050372f89148c3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "bedee9bb7b97ec8113fef59fb725cd76e07c80fb9e18f7724a074943e6ca92a8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "8c7dde7e717611e4fdbcfa9cc876489ccd6bd40be950ce23638cc927d869becf",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "b5ea40ba4aaad0fbfd9e57898a5ac161d7935e67b290e9f63eed13f306bba599",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        "native_bind_failures_match_checked_out_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "e135cf54a1335e3b883b114ed9761a22be2036ce4a9eb22ceb1bd858c1521472",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/windows-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        "native_bind_failures_match_checked_out_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "51f820fde11d3c6d7089154859d9c5a283b13a2ea09c5a3445bdde8b033fd729",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/windows-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "eedc5f12fb1c7ad71ba13e330c166e5e73b7d31c05a1820aa3bc0baab0f46bc8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "45c41931cd79578b4a384cbca52601e5d9ed6aacb7fc10ce6a095beb05e6f914",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "034868fdadbe2d762ab59636ed18ffec0b04ffedbf496502878c7c5a9e828d11",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "d4b3696e771f514405fba294853d12af103cf0922c1a0f6341c55287eb52b8cd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "194e2d159262bbebe94a8a4bc279002ed91b2149eb284c7c906155c250211b53",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "6447e11de058f3d37b915510123bb26523506776525d869356589df85a51b48c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "5e0536919f1f8498290b4c24ec142380aea602cf79a7600b7c9e7c316b40340e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "77eb8351ccb55727553bacc2532fbfcab31bd0a081e7b5b85db8fb8c87a7339d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "654e7e8f0ab9e70746e8bde1f2f9ba8661724651a8c2b24deebac902bb393596",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "native_bind_failures_match_checked_out_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "673d55e2c225e0a084a06991ef326e7beeae0f5612e9794e0c5d050950d9774f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "b9f053179152e1a77c9fcb0822fbf4a8e2d07cf8c438be9878bc9f0b00c4996a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "d4df60679470e00fe2f49d184e2d72807fc61ca63ce3193150685e47f17004d7",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "713c4a604316a0298a58ca4d1203c57f9f6fa514cfc8531733629d3e7879ab6d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "00c206f1b5e0cc138a8882777c72bb5150a25e9faed80cb4614f6f3814d6178f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-aux/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
