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
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "743b466503a47c2f62a65547fa1c399919631f7b497ce44c23ab25cd33d2a6d3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "e30208c548fbffa41642fb84bbb9050b434e95a65021dcc7bf7f71e492c8aa4a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
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
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "24279dc346b16e02e7372b83ea1e3a8779e8d78a48f1dbc3814e9b618cbd3ec1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "e240b76adcaf3fe027e14098e09f84a3387d575d14bd9bdc1ad73aa2528d4d6f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "1555989ebe42aebce7e5b33f95d677bd53c05cc9ef12fc6f646bd3f24aafd951",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "7a1bbf6943b3fe3f106b525d5aa9c6a1cb8f1a95775b5f3e3671b722ffa4197f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "3852b1d78eb514eed9335f7c86289636b8e641d0c11ac7cdd45f38382884b9e6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "0ded067c08a75232b35ce532cc67e77a0877b729e7835757d86ecfdef62bce20",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
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
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "d37a9a71bf826caf34b4db56c75a30fd26567bc18bd6108d58b9175810dacf45",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "b4e3fa108d17f4b191c151ba5f0ea98d4c7841c66149bcd8ef45a87ccf0857c8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "2c2850ded821940d49c5b9e81ac92723a3c497d895922d0d9beba36f698f4ba5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "49d7d55bdeecf71135ffca3f2d0314094b592864adcd66f0abea8f7fd2a910bc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
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
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "eef255706be9cadc79324b4938f4a09113fd47970c01818e630dd3bd21cbc1cd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "a797d6ab4a435201e379f496a8e07f630b65a5cf71923e7f88e55554291810eb",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
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
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "3e72ca6fdcc7ff1368520ac0cb007265be2628d51c0bf7cee45d8510083ced6b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "a4cb8a48b9631d5a556b542e384f6a4e45fd70004fcb42b97d7e94187637dc52",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "0e416bc73d1708d00766a6ba4a8558f72c3962bde9ff84b6fc11f089c19d06e2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "e480b43b1121be280699b9388c64cf93c54da71a8eb9b0390084f01a79d5251d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "c7ba9a05ce712f07b6a9a5dad5ad05e5146f1a22f6f0f8e1601606d2b7672ff3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "ea7c059e773ae4908413b678bebb315403f5a9d9bd91e888a711e51c9304150a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
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
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "cf3a6178b0b07252e3c4124c1c96c413af44ac68ba2edd390870f055132be812",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "47a8b74823cfd3f88ef56e18f082844204797a4be2ded447d252771568d41fea",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "6f0c2bca0c6f0ba8a2ccc54a7c3fa0c08d6e4f2efd87bc0f69379435fb90defd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "5df99f8f5178ba9ae07e6c12be5067ae121e115b",
        "6048e00b2c41ff26f9784e7d0d9f2a19ec2ade3a7fe8bd8de6fed11b2c65dbf1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-refresh-5df99f8f/http-aux/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
