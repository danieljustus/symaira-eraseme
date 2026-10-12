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
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "4b3438e03a2aea9f72fd76527fa435f9e45c9420898f9731cdf71d4b08d831c4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "c3f37b611e9260984944c30ef0269090e2216547f39d8fd3da217904c551224e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/arm64",
        "native_bind_failures_match_checked_out_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "56858b2b0ff3848c0c17b2d2b41d1cfad2eeb8777f47f972d3ad862345bf129f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "9c5c99ab6e0f864a57e2dd8ab0ca58d54dcb052a227d0e35135cd8aa12d9fb26",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "f5962b252ffc0a900fdb16b4c2ea1fb29a594353565e3aa4c5f16dcb9596cf2f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "de761352815993da1be97db8aa2b220d1c0d428ca7934b10bbb08358b15fd6b8",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/arm64",
        "unavailable_local_address_error_matches_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "dc356a0e1435f8b0cc32ab7d5dfb50438acb98943cd0af8b6789b883a961ed1a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "db43f1f199f83a10b2fa77efa02fcda9a88e6bfa0721b9a9acd762b4f7f3685b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "3b96677842448f57606729180afd4440ad3f6ebf2d42f359d4b7bc6ef62ba4bf",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "linux/amd64",
        "native_bind_failures_match_checked_out_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "76d85bd255856705e7dac67444230212d8c5558f6f7ea7b3187be8693e7e1e68",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "linux/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "ee97e91cda9d85d755c001c406731f545ecfc6fdeef3d9a5cb620c70de5b3ac3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "linux/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "229642f365976d26c646de7543213f2b26b1f575f69e5012ee857da4d319710c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "linux/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "d40fe03cdef7d2397918734a863b5756c1d03c306fe641f0f42c4e2ab34f366c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "linux/amd64",
        "unavailable_local_address_error_matches_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "957535b95a65ec1cace5be17b0d710a620525b383ccd99fd8e75282a820b9f3b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/linux-amd64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        "native_bind_failures_match_checked_out_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "ee4894a58aabefbe5fdd140c8cf3a5022a91df1dea5a4d91d62ce9d0b8d5e371",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/windows-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        "native_bind_failures_match_checked_out_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "2f4540e64d0e21f320c6989f879e21a27f445b8cf21f58706ef0927cf99a0b89",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/windows-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "8f29cf4a24f3705e6c22ae02ecd0ac1469b585cb21b05d903226f9e0800dbe85",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-arm64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "1306eeb016975468b28a0cd767620c8f9de27a66cd569b231bcf9fcfdbb4dcc5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-arm64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "native_bind_failures_match_checked_out_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "5ccae33a51528f5a1f286bb521ade37361dfcd7076bccf936d7391fd2f382280",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/arm64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "58c01ddd0de449f4fd3c17c43f4f303c85042b8ac953d8338d566adc652c98ee",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-arm64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/arm64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "a4b0388df3df03a209994614dc75293c488c36865c3f14d6ba1fb5921103bd2c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-arm64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/arm64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "d87b1f005fcdc8e4b5e8a622a809752d1977ef79688578789a401f05f00752e7",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-arm64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/arm64",
        "unavailable_local_address_error_matches_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "5630492071b4d420f89253b30b2b1d6285976dd0969f5707449ca990ad804447",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-arm64/unavailable_local_address_error_matches_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_and_reaps_host_agent_like_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "854e5329b81b57f18d83f47713a7f7bace949784de34b38911ab26c4e997831e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-amd64/live_http_disconnect_cancels_and_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "live_http_disconnect_cancels_provider_request_like_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "eb783d146b21059ce1b5383a8ed4d97f6879f69c16d83200f298c791e368c93c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-amd64/live_http_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "native_bind_failures_match_checked_out_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "fe801de1141346b1bc693dabfd75b9fc51c6549c99ac1630549bacb0123a460e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-amd64/native_bind_failures_match_checked_out_go.json"
        ),
    ),
    (
        "darwin/amd64",
        "obs_text_origin_rejection_matches_go_and_rust_processes",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "7386ee3a9b903af1a46b91b0aabff0f05d4a9507461410c4f37353aa4ccb229d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-amd64/obs_text_origin_rejection_matches_go_and_rust_processes.json"
        ),
    ),
    (
        "darwin/amd64",
        "occupied_bind_error_matches_go_for_ipv4_and_ipv6",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "aabf11606cb5d5a37c342336ba70fc25c715e6c163d250076f4683e6d9469620",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-amd64/occupied_bind_error_matches_go_for_ipv4_and_ipv6.json"
        ),
    ),
    (
        "darwin/amd64",
        "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "a6ed152bfd007a6a332a6cbc108327a29c56ed058a902f4eeeda210c4db8e2fa",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-amd64/startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness.json"
        ),
    ),
    (
        "darwin/amd64",
        "unavailable_local_address_error_matches_go",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "e16b0cd68ed2fbf83e28c8ce8da9110d37fd0d0d0e168f906c32aa7d8575a3b3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-amd64/unavailable_local_address_error_matches_go.json"
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
                && name == "crates/symeraseme-cli/tests/mcp_http_process.rs";
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
