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
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "ea64b34398eee811838a8d4110864e3740e7a93cc6ff5af2ab817f34f0cf9410",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "90f3720cbaa2eebdb5f4e991a95b4d1f1fd601edc2c50d78b1b8ee3f0f51a7d8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "e2c8215445ed8636d757e0a17c70bbecc1928244592273af8798f139c75f96ae",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "0d3f82f34211df01d2bcf3b3dae0437e26ebc801ba6015f9a2709bba264e885f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "7ef533b084111e3e1fca89364bf852e4b3af7e9a48c9a52db1940cf3f70a5f47",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "2b77df33814b1eab2058cc73d83a5e018b2ab9b078497e8e35e0cd2d0b98796a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "7bc740a6926e32fac0b59f7e79d124299e9612333f0c0cd34edadd482e04a9d6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "da15e250465e7ed39e1fd2049968346a16cf7ef42ad6bec54e3241fae3ea6306",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "e94a054a7e740a4aadffb1682fa01b84d698bdd1d6a5109d181ea12c55e84701",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "6e36a9c37b6e5d04187c5ae599277c7e6ecf0ff982f9256f539be55f6cc321bc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "2c69aea60a7615df97ae12e35324751e81f8a5fb166974c93b0be3ad6fe6ae43",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "b983f13f3b51009a556085e0aafc513c63b66418c95b532859074ccb54ae663b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "7eaa20f809b8cd0f61197d1c3c0d0ac7ca1b58866d541203bc9f22dbc11abffc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "edbc1b501035e261e4df206c82b7afd39e762a5dfbdc10a7ec2f7dc5414a27a0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "62f3fff31d46060f1e478307bdfc90ff4ae899e6cc70937bfde23bb658678819",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "75b96056cfa4284d787c4aa1f9901e8bb94635b904d3b0744c97717399b7499f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "369b25cfe4a3e058cd375a4568405c506ee607cfad28e860859861d1dd02c826",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "ded033647305b7379debfb124069a0594cfc3caa2fe41827b1d506db7c2622bd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "ac3830ade4462d95c07badf763a16e96bc85a20ed94b23fe1d7cebefb4cff652",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "e8885d63941c4a992bbd8720bf589bce7a700b031103ae058479676839ed6993",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "c129252f76e28cc9d0bdd7fe25152f9e63db3840",
        "b4c0279790e70fa7948c7420e69f77899117c4258496d15c4531ca8efbc23ee2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-c129252f/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
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
