//! Authenticated Windows observations; expected bytes never come from the Rust result.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Component, Path};
use std::process::Command;

pub const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "a3db5a02f9a9aa08d172228caf709c621af3c0bb";
const SIDECAR: &[u8] = include_bytes!(
    "../../../../tests/fixtures/go-frozen/refresh-a3db5a02/consent-acl/privacy-derivation.json"
);
const SIDECAR_SHA: &str = "2c1107044ef7ef418c66e656d60a9f6a4523c7820fbbce2e509648ee4c3448d0";
const RECORDS: [(&str, &[u8]); 2] = [
    (
        "windows/amd64",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a3db5a02/consent-acl/windows-amd64.json"
        ),
    ),
    (
        "windows/arm64",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a3db5a02/consent-acl/windows-arm64.json"
        ),
    ),
];
pub const CASES: [&str; 4] = [
    "inherited",
    "owner-only-parent",
    "readonly-directory",
    "readonly-token",
];
pub const NAME_ROLE: &str = "consent_<CONSENT_ROLE>.json";
const GROUP_ROLE: &str = "<PRIMARY_GROUP_ROLE>";

pub fn digest(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}

pub fn check_modes(capture: Option<&OsStr>, live: Option<&OsStr>) {
    assert!(capture.is_none(), "consent capture is retired");
    assert!(live.is_none_or(|v| v == "0"), "live Go is retired");
}

pub fn reject_retired_modes() {
    check_modes(
        std::env::var_os("SYMERASEME_CAPTURE_CONSENT_ACL").as_deref(),
        std::env::var_os("SYMERASEME_PARITY_LIVE_GO").as_deref(),
    );
}

fn pin(raw: &[u8], value: &Value) {
    assert!(value.as_object().is_some_and(|v| v.len() == 2));
    assert!(
        value["bytes"].as_u64() == Some(raw.len() as u64),
        "recorded byte length"
    );
    assert!(
        value["sha256"].as_str() == Some(digest(raw).as_str()),
        "recorded byte digest"
    );
}

pub fn blob(value: &Value, field: &str) -> Vec<u8> {
    let encoded = value[format!("{field}_base64")]
        .as_str()
        .expect("recorded base64");
    assert!(encoded.len() <= 128 * 1024);
    let raw = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .expect("valid base64");
    assert!(base64::engine::general_purpose::STANDARD.encode(&raw) == encoded);
    pin(&raw, &value[field]);
    raw
}

fn git(args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .current_dir(ROOT)
        .args(args)
        .output()
        .expect("immutable source available");
    assert!(
        output.status.success(),
        "immutable consent source unavailable"
    );
    output.stdout
}

pub struct Record {
    value: Value,
    provenance: Value,
}

pub fn load(target: &str) -> Record {
    reject_retired_modes();
    let (_, raw) = RECORDS
        .iter()
        .find(|r| r.0 == target)
        .expect("supported recorded Windows target");
    let record = parse(SIDECAR, raw, target);
    let mut names: BTreeSet<String> = git(&[
        "ls-tree",
        "-r",
        "-z",
        "--name-only",
        REVISION,
        "--",
        "cmd",
        "internal",
    ])
    .split(|b| *b == 0)
    .filter(|p| p.ends_with(b".go"))
    .map(|p| String::from_utf8(p.to_vec()).unwrap())
    .collect();
    names.extend(
        [
            "go.mod",
            "go.sum",
            "Cargo.lock",
            "rust-tests/parity/oracle/consent-windows-acl/main.go",
            "crates/symeraseme-core/tests/consent_windows_acl.rs",
            "crates/symeraseme-core/tests/support/capture_consent_acl.rs",
            "crates/symeraseme-core/tests/support/consent_acl.ps1",
            ".github/workflows/capture-consent-retirement.yml",
            ".github/workflows/consent-windows.yml",
        ]
        .map(str::to_owned),
    );
    let sources = record.value["source_files"].as_object().unwrap();
    assert!(sources.keys().cloned().collect::<BTreeSet<_>>() == names && names.len() == 178);
    for (name, measured) in sources {
        assert!(
            !name.contains('\\')
                && Path::new(name)
                    .components()
                    .all(|p| matches!(p, Component::Normal(_)))
        );
        let bytes = git(&[
            "-c",
            "core.autocrlf=true",
            "cat-file",
            "--filters",
            &format!("{REVISION}:{name}"),
        ]);
        pin(&bytes, measured);
    }
    record
}

fn parse(sidecar: &[u8], raw: &[u8], target: &str) -> Record {
    assert!(
        sidecar.len() < 128 * 1024 && digest(sidecar) == SIDECAR_SHA,
        "independent sidecar anchor"
    );
    let manifest: Value = serde_json::from_slice(sidecar).unwrap();
    assert!(manifest["source_revision"] == REVISION && manifest["original_run"] == 38374291011_u64);
    let entries = manifest["records"].as_array().unwrap();
    assert!(entries.len() == 2);
    let provenance = entries
        .iter()
        .find(|r| r["target"] == target)
        .expect("target in sidecar")
        .clone();
    assert!(raw.len() < 1024 * 1024 && provenance["derived_bytes"] == raw.len());
    assert!(
        provenance["derived_sha256"] == digest(raw),
        "whole consent record anchor"
    );
    let value: Value = serde_json::from_slice(raw).unwrap();
    let record = Record { value, provenance };
    record.validate(target);
    record
}

pub fn token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7; 16])
}
pub fn filename() -> String {
    format!("consent_{}.json", &digest(token().as_bytes())[..16])
}

fn replace_once(raw: &[u8], from: &str, to: &str) -> Vec<u8> {
    let text = std::str::from_utf8(raw).expect("recorded UTF-8");
    assert!(
        text.matches(from).count() == 1,
        "exact privacy role occurrence"
    );
    text.replacen(from, to, 1).into_bytes()
}

impl Record {
    fn validate(&self, target: &str) {
        let r = &self.value;
        assert!(r["schema"] == "symeraseme.actual-go.consent-windows-acl.v1");
        assert!(
            r["source_revision"] == REVISION
                && r["source_clean"] == true
                && r["actual_success"] == true
        );
        assert!(r["native_target"] == target);
        let arch = target.strip_prefix("windows/").expect("Windows target");
        assert!(["amd64", "arm64"].contains(&arch));
        for key in ["GOOS", "GOHOSTOS"] {
            assert!(r["go"]["env"][key] == "windows");
        }
        for key in ["GOARCH", "GOHOSTARCH"] {
            assert!(r["go"]["env"][key] == arch);
        }
        assert!(r["go"]["version"] == "go1.26.6" && r["go"]["env"]["GOVERSION"] == "go1.26.6");
        let build = r["go"]["embedded_build_info"].as_str().unwrap();
        for expected in [
            format!("vcs.revision={REVISION}"),
            "vcs.modified=false".into(),
            "GOOS=windows".into(),
            format!("GOARCH={arch}"),
        ] {
            assert!(build.contains(&expected));
        }
        let keys: BTreeSet<_> = CASES.into_iter().map(str::to_owned).collect();
        assert!(
            r["cases"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                == keys
        );
        let inventory = r["case_inventory"].as_array().unwrap();
        assert!(
            inventory.len() == 4
                && inventory
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect::<BTreeSet<_>>()
                    == keys
        );
        let mut labels = BTreeSet::new();
        for case in CASES {
            let c = &r["cases"][case];
            assert!(c["assertions_completed"] == true && c["go_run"]["exit_status"] == 0);
            assert!(blob(&c["go_run"], "stderr").is_empty());
            let stdout = blob(&c["go_run"], "stdout");
            let go: Value = serde_json::from_slice(&stdout).unwrap();
            assert!(
                go["schema"] == "symeraseme.go-oracle.consent-windows-acl.v1"
                    && go["platform"] == target
                    && go["go_version"] == "go1.26.6"
            );
            assert!(
                go["failed"] == (case == "readonly-token")
                    && go["error_class"]
                        == if case == "readonly-token" {
                            "permission"
                        } else {
                            "ok"
                        }
            );
            for implementation in ["go", "rust"] {
                assert!(
                    c["outcome"][implementation]["failed"] == go["failed"]
                        && c["outcome"][implementation]["error_class"] == go["error_class"]
                );
            }
            assert!(c["go_files"]["listing"] == go["files"]);
            let restored = replace_once(&stdout, NAME_ROLE, &filename());
            pin(
                &restored,
                &self.provenance["original_stream_pins"]["go"][case]["stdout"],
            );
            let files = self.files(case);
            assert!(files.len() == 2 && files["sentinel"] == b"unrelated sentinel");
            labels.insert(format!("parent-{case}"));
            if case != "inherited" {
                labels.insert(case.to_owned());
            }
            for implementation in ["go", "rust"] {
                labels.insert(format!("{implementation}-dir-{case}"));
                for file in [NAME_ROLE, "sentinel"] {
                    labels.insert(format!("{implementation}-file-{case}-{file}"));
                }
            }
        }
        let acls = r["acl_observations"].as_object().unwrap();
        assert!(acls.len() == 31 && acls.keys().cloned().collect::<BTreeSet<_>>() == labels);
        for (label, acl) in acls {
            assert!(acl["exit_status"] == 0);
            let parsed: Value = serde_json::from_slice(&blob(acl, "stdout")).unwrap();
            assert!(
                parsed.as_object().unwrap().len() == 3
                    && parsed["protected"].is_boolean()
                    && parsed["readonly"].is_boolean()
            );
            let sddl = parsed["sddl"].as_str().unwrap();
            assert!(
                sddl.starts_with(&format!("O:LAG:{GROUP_ROLE}D:"))
                    && sddl.matches(GROUP_ROLE).count() == 1
            );
            let mode = if CASES.contains(&label.as_str()) {
                "protect"
            } else {
                "read"
            };
            assert!(blob(acl, "stderr") == format!("consent ACL script started: {mode}\r\nconsent ACL script completed: {mode}\r\n").as_bytes());
        }
    }

    pub fn files(&self, case: &str) -> BTreeMap<String, Vec<u8>> {
        assert!(CASES.contains(&case));
        let files = &self.value["cases"][case]["go_files"];
        let entries = files["entries"].as_object().unwrap();
        assert!(
            entries.len() == 2
                && entries.contains_key(NAME_ROLE)
                && entries.contains_key("sentinel")
        );
        assert!(
            files["listing"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<BTreeSet<_>>()
                == entries.keys().cloned().collect()
        );
        let mut result = BTreeMap::new();
        for (name, entry) in entries {
            let bytes = blob(entry, "bytes");
            if name == NAME_ROLE {
                let restored = replace_once(
                    &bytes,
                    "\"TOKEN_ROLE\"",
                    &serde_json::to_string(&token()).unwrap(),
                );
                pin(
                    &restored,
                    &self.provenance["original_stream_pins"]["go"][case]["consent"],
                );
                result.insert(filename(), restored);
            } else {
                result.insert(name.clone(), bytes);
            }
        }
        result
    }

    pub fn acl(&self, label: &str, group: &str) -> Value {
        assert!(
            group.starts_with("S-1-5-")
                && group
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-'),
            "independent group SID syntax"
        );
        let raw = blob(&self.value["acl_observations"][label], "stdout");
        let expanded = replace_once(&raw, GROUP_ROLE, group);
        serde_json::from_slice(&expanded).unwrap()
    }

    pub fn assert_outcome(&self, case: &str, failed: bool) {
        let outcome = &self.value["cases"][case]["outcome"]["go"];
        assert!(
            outcome["failed"] == failed
                && outcome["error_class"] == if failed { "permission" } else { "ok" },
            "native consent outcome"
        );
    }
}

/// Called only after the native verifier has independently re-observed this identity.
pub fn verified_group(baseline: &Value, revision: &str, script_hash: &str, arch: &str) -> String {
    assert!(baseline["schema"] == "symeraseme.windows-identity-baseline.v1");
    assert!(baseline["source_revision"] == revision && baseline["script_sha256"] == script_hash);
    let machine = match arch {
        "X64" => "0x8664",
        "ARM64" => "0xAA64",
        _ => panic!("unsupported identity architecture"),
    };
    let host = &baseline["host"];
    assert!(host["platform"] == "Windows" && host["expected_arch"] == arch);
    assert!(
        host["native_machine"] == machine
            && host["process_machine"] == "0x0000"
            && host["pointer_size"] == 8
    );
    let checks = baseline["checks"].as_object().unwrap();
    let expected: BTreeSet<_> = [
        "source_revision_matches_head",
        "source_tree_clean",
        "native_architecture",
        "user_matches_launch_process_token",
        "user_sid_observed",
        "primary_group_sid_observed",
        "rid500_local_account_unique",
        "rid500_matches_sddl_la",
        "artifact_under_runner_temp",
        "runner_temp_outside_repo",
        "output_does_not_exist",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert!(
        checks.keys().cloned().collect::<BTreeSet<_>>() == expected
            && checks.values().all(|v| v == true)
    );
    let user = baseline["process"]["user_sid"].as_str().unwrap();
    let group = baseline["process"]["primary_group_sid"].as_str().unwrap();
    assert!(
        user == baseline["local_admin"]["rid500_sid"]
            && user == baseline["local_admin"]["sddl_la_sid"]
            && user.ends_with("-500")
    );
    assert!(
        group != user
            && group.starts_with("S-1-5-")
            && group
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
    );
    group.to_owned()
}

pub fn assert_acl(expected: &Value, actual: &Value) {
    assert!(
        expected == actual,
        "native consent security descriptor differs"
    );
}
pub fn assert_files(expected: &BTreeMap<String, Vec<u8>>, actual: &BTreeMap<String, Vec<u8>>) {
    assert!(
        expected == actual,
        "native consent file inventory or bytes differ"
    );
}

#[test]
fn retained_records_and_runtime_comparators_reject_corruption() {
    for (target, raw) in RECORDS {
        let r = load(target);
        for case in CASES {
            let files = r.files(case);
            assert_files(&files, &files);
            let mut bad = files.clone();
            bad.get_mut("sentinel").unwrap().push(b'!');
            assert!(std::panic::catch_unwind(|| assert_files(&files, &bad)).is_err());
            let mut missing = files.clone();
            missing.remove("sentinel");
            assert!(std::panic::catch_unwind(|| assert_files(&files, &missing)).is_err());
            let name = filename();
            for field in ["command", "issued_at", "expires_at", "token"] {
                let mut bad = files.clone();
                let mut content: Value = serde_json::from_slice(&bad[&name]).unwrap();
                content[field] = Value::Null;
                bad.insert(name.clone(), serde_json::to_vec(&content).unwrap());
                assert!(std::panic::catch_unwind(|| assert_files(&files, &bad)).is_err());
            }
            r.assert_outcome(case, case == "readonly-token");
            assert!(
                std::panic::catch_unwind(|| r.assert_outcome(case, case != "readonly-token"))
                    .is_err()
            );
            let label = format!("go-file-{case}-{NAME_ROLE}");
            // Synthetic identity is only comparator input, not native evidence or oracle output.
            let synthetic_group = ["S-1-5-", "21-1-2-3"].concat();
            let acl = r.acl(&label, &synthetic_group);
            assert_acl(&acl, &acl);
            for (old, new) in [
                ("O:LA", "O:SY"),
                ("G:S-1-5-", "G:S-1-4-"),
                (";;;LA)", ";;;SY)"),
            ] {
                let mut bad = acl.clone();
                let changed = bad["sddl"].as_str().unwrap().replacen(old, new, 1);
                assert!(
                    changed != bad["sddl"].as_str().unwrap(),
                    "nonvacuous ACL mutation"
                );
                bad["sddl"] = changed.into();
                assert!(std::panic::catch_unwind(|| assert_acl(&acl, &bad)).is_err());
            }
            for key in ["sddl", "protected", "readonly"] {
                let mut bad = acl.clone();
                bad[key] = if key == "sddl" {
                    "different security descriptor".into()
                } else {
                    (!bad[key].as_bool().unwrap()).into()
                };
                assert!(std::panic::catch_unwind(|| assert_acl(&acl, &bad)).is_err());
            }
        }
        let mut bad = raw.to_vec();
        bad.push(b'!');
        assert!(std::panic::catch_unwind(|| parse(SIDECAR, &bad, target)).is_err());
        assert!(
            std::panic::catch_unwind(|| parse(SIDECAR, &raw[..raw.len() - 1], target)).is_err()
        );
        assert!(std::panic::catch_unwind(|| parse(b"{}", raw, target)).is_err());
        assert!(std::panic::catch_unwind(|| parse(SIDECAR, raw, "linux/amd64")).is_err());
        let mut bad = r.value.clone();
        bad["cases"].as_object_mut().unwrap().remove("inherited");
        assert!(
            std::panic::catch_unwind(|| Record {
                value: bad,
                provenance: r.provenance.clone()
            }
            .validate(target))
            .is_err()
        );
    }
    for value in ["", "1", "invalid"] {
        assert!(std::panic::catch_unwind(|| check_modes(Some(OsStr::new(value)), None)).is_err());
        assert!(std::panic::catch_unwind(|| check_modes(None, Some(OsStr::new(value)))).is_err());
    }
    check_modes(None, Some(OsStr::new("0")));
}

#[test]
fn verified_identity_shape_rejects_mismatched_roles_and_provenance() {
    // Synthetic parser inputs only: the Windows test additionally invokes the
    // real OS verifier. These values are not a captured identity or native proof.
    let group = ["S-1-5-", "21-1-2-3"].concat();
    let user = format!("{group}-500");
    let revision = "0".repeat(40);
    let hash = "1".repeat(64);
    let mut baseline = serde_json::json!({
        "schema": "symeraseme.windows-identity-baseline.v1",
        "source_revision": revision, "script_sha256": hash,
        "host": {"platform":"Windows", "expected_arch":"X64", "native_machine":"0x8664", "process_machine":"0x0000", "pointer_size":8},
        "process": {"user_sid":user, "primary_group_sid":group},
        "local_admin": {"rid500_sid":user, "sddl_la_sid":user},
        "checks": {}
    });
    for check in [
        "source_revision_matches_head",
        "source_tree_clean",
        "native_architecture",
        "user_matches_launch_process_token",
        "user_sid_observed",
        "primary_group_sid_observed",
        "rid500_local_account_unique",
        "rid500_matches_sddl_la",
        "artifact_under_runner_temp",
        "runner_temp_outside_repo",
        "output_does_not_exist",
    ] {
        baseline["checks"][check] = true.into();
    }
    assert!(verified_group(&baseline, &revision, &hash, "X64") == group);
    for pointer in [
        "/schema",
        "/source_revision",
        "/script_sha256",
        "/host/platform",
        "/host/expected_arch",
        "/host/native_machine",
        "/host/process_machine",
        "/host/pointer_size",
        "/process/user_sid",
        "/process/primary_group_sid",
        "/local_admin/rid500_sid",
        "/local_admin/sddl_la_sid",
        "/checks/native_architecture",
    ] {
        let mut bad = baseline.clone();
        *bad.pointer_mut(pointer).unwrap() = Value::Null;
        assert!(
            std::panic::catch_unwind(|| verified_group(&bad, &revision, &hash, "X64")).is_err()
        );
    }
    let mut bad = baseline.clone();
    bad["process"]["primary_group_sid"] = user.into();
    assert!(std::panic::catch_unwind(|| verified_group(&bad, &revision, &hash, "X64")).is_err());
    baseline["host"]["expected_arch"] = "ARM64".into();
    baseline["host"]["native_machine"] = "0xAA64".into();
    assert!(verified_group(&baseline, &revision, &hash, "ARM64") == group);
}
