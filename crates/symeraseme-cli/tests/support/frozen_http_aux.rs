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
        "433622f6e6c884694a304f483ae481f93069b455",
        "bef3120a678cea7181f2e22b01aa8df7902a0b6fa83eb6155c6e72994f27f6b8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "3fbeb285a4ac31c7268a4f168bea696dcdf34343da2d8cead58a2e51a96d91b8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "03fad85721cb4cd96627b99aad8d5cdec5af0cc1c02b4c0552af71398e85edbe",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "433622f6e6c884694a304f483ae481f93069b455",
        "f9acee53e667c976bd7621b8c0597ca1e31d5ebf144e40c0dd1a908c133f6770",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "433622f6e6c884694a304f483ae481f93069b455",
        "cd9196c1ad8132fb71f8aed778eb645023fe8c80449ae14d90e21606bcf93e03",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "433622f6e6c884694a304f483ae481f93069b455",
        "14a514ddf5a66f5ffff50165be466c355425c8b69ce65083d55a9648f3bb5bf4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "9e93f1005beb316e907e7bd699852adc1ca8bee92e2058b16fbcdbd27448ec31",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "a20d20bc873e8d6dc15cc05f5723e85fc5c3eb304ddccd6a6c9001b3c82f469a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "9e115b54792a6e0ecab0b2bb812c4ab7789298e354c8e58440dd1b9f131b5f54",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "ebe6df6c41c7557b3f4c2c9431d263bc8c5c9d207572016e526e8576a5db20c3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "433622f6e6c884694a304f483ae481f93069b455",
        "5f0686905c6791f3dbb66c8ae957a87dc3a002c82e36d9b8812c1a639e98bf14",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "433622f6e6c884694a304f483ae481f93069b455",
        "c8e46587c36b80c62613f125c436d5cd26323eeb2217d696b5b73e17f1559d9a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "433622f6e6c884694a304f483ae481f93069b455",
        "034d4279936d4b2889e55bcd6802e53907f07abf4ae26b7455afc52f4936b214",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "50fc54b8d3f2a762d10f9e2549328c5ee70122786775b4582e78e1bb5031da22",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        "native_bind_failures_match_checked_out_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "abfbbe6bd66d37895d28d026e6cf476b052347b405fa4b3084511d6d2e34fd04",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/windows-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        "native_bind_failures_match_checked_out_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "d918ffbba8b0139775162e7db6c96caa027efa11c1774ed1124068ba8b6e0e45",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/windows-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "09c5779ebd4951f70ac815c919d49aa59902bd896cd1c16ca179c1b9c86591e0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "f27b1f4398f603c65c91cb1721d3a5600ccfd66198be3a1d0181e2c3289be9d5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "8a1331b64083fed75ba8a59f62b62fc93fd1273b7330aa376c27714f393849d1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "433622f6e6c884694a304f483ae481f93069b455",
        "d7f6b3c90e3930f8714b0307b7ce33ea8d90b0536d0ee3120d9d81dbf2b80f94",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "433622f6e6c884694a304f483ae481f93069b455",
        "308d94893945c2da48443444703c378b37c43c611c20e9236b6e2d75ad01d00e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "433622f6e6c884694a304f483ae481f93069b455",
        "84c962253085728cf2a0d9f7b2202debfa9493b6f2b8ceb4bb7b6dded6afc9b8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "c5420fa40aae81e09f43b5d0d803f6c8ba2d6f6ec87d1d27e3c22425daa59840",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "e6f38da5071be8949cfeac43ef7d88ea4b4819c2d439659a69945b93d5e90a88",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "329e2c81b4f95fe7f3904f0a75c5a4df39e5774d83b6d4ca368dd6868ad7fbed",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "native_bind_failures_match_checked_out_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "0033a0b52670487bbb482778fdf63d6034cce71336d6858d78dd894e3c975d1e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "433622f6e6c884694a304f483ae481f93069b455",
        "41fd529b1442a9dade8fa5450f40636a15d2441576df7dbe11e8906c3abfe73d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "433622f6e6c884694a304f483ae481f93069b455",
        "52278034dfd220aac5983d9b27b826002ff98542419006ff0ece5594ad1b9222",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "433622f6e6c884694a304f483ae481f93069b455",
        "3867e3db4a5444bbf1d38fba9d3d917aaf89612d652ee863f8b4a0919ec7b413",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "433622f6e6c884694a304f483ae481f93069b455",
        "a2c48e6aab0b61df1bfcf7f19862e9ab0fd6c883d890c27b3cdb1a23796cb215",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/http-aux/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
