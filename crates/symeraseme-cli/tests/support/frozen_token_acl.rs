//! Retained native Go token-ACL expectations for the Rust-only Windows replay.
//!
//! Each authenticated sidecar is pinned whole and parsed once. Its producer
//! source, generator and script bytes come only from immutable Git blobs at the
//! frozen producer revision, never from the working tree. Live Go is retired:
//! a missing or unverifiable record fails explicitly.

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path};
use std::process::Command;

/// Producer revision of the native Go token capture (run 38374294030).
pub const PRODUCER_REVISION: &str = "a3db5a02f9a9aa08d172228caf709c621af3c0bb";
pub const IDENTITY_SCHEMA: &str = "symeraseme.windows-identity-baseline.v1";
const RECORD_SCHEMA: &str = "symeraseme.actual-go.http-aux.v1";
const TEST: &str = "native_windows_token_dacl_and_rotation_match_go";
const TEST_FILE: &str = "crates/symeraseme-cli/tests/mcp_http_native_contract.rs";
const SCRIPT: &str = "crates/symeraseme-cli/tests/support/mcp_token_acl.ps1";
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const SOURCE_ROOTS: [&str; 4] = [
    "cmd",
    "internal",
    "rust-tests/parity/oracle/agent-cancel",
    "rust-tests/parity/oracle/provider-cancel",
];
const GENERATORS: [&str; 4] = [
    TEST_FILE,
    "crates/symeraseme-cli/tests/support/mcp_http_port.rs",
    "crates/symeraseme-cli/tests/support/capture_http_aux.rs",
    "Cargo.lock",
];
const GROUP_MARKER: &str = "G:<PRIMARY_GROUP>";
const RUNTIME_ROOT: &str = "<RUNTIME_ROOT>";
pub const SENTINEL: &[u8] = b"unrelated private sentinel";
const MAX_RECORD: usize = 1024 * 1024;
const NOTIFY_PREFIX: &[u8] = b"write MCP auth token: ";

const PRIVACY_PIN: &str = "d9d9f378e832a147dd8cd56ee1f25e2aaaa0bc6f9fc55fc8396b3c1de4a0f576";
const PRIVACY: &[u8] = include_bytes!(
    "../../../../tests/fixtures/go-frozen/refresh-a3db5a02/token-acl/privacy-derivation.json"
);
/// (target, authenticated whole-sidecar bytes, whole-sidecar SHA-256 pin).
const RECORDS: [(&str, &[u8], &str); 2] = [
    (
        "windows/amd64",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a3db5a02/token-acl/windows-amd64.json"
        ),
        "ededfa77f748594d507b393f6932af9d89b6ba4a92eff1a0e38ade57072b1a1f",
    ),
    (
        "windows/arm64",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a3db5a02/token-acl/windows-arm64.json"
        ),
        "538dbdcda423adc72d4c117d209712b7e25d3fff50912de43543f592d384c8b9",
    ),
];

pub(super) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn git(args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(ROOT)
        .output()
        .map_err(|_| "git is unavailable for the immutable producer pin".to_owned())?;
    if !output.status.success() {
        return Err("immutable producer revision is unavailable".into());
    }
    Ok(output.stdout)
}

/// Archived bytes of `object` under the recorded checkout newline policy.
fn filtered(autocrlf: &str, object: &str) -> Result<Vec<u8>, String> {
    git(&[
        "-c",
        &format!("core.autocrlf={autocrlf}"),
        "cat-file",
        "--filters",
        object,
    ])
}

fn str_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing or non-string field: {key}"))
}

fn bool_field(value: &Value, key: &str) -> Result<bool, String> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing or non-boolean field: {key}"))
}

fn uint_field(value: &Value, key: &str) -> Result<u64, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing or non-integer field: {key}"))
}

/// A measured byte blob: base64 payload, byte count and SHA-256 must agree.
fn measured(value: &Value) -> Result<Vec<u8>, String> {
    if !value.as_object().is_some_and(|v| v.len() == 3) {
        return Err("measured payload field set differs".into());
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(str_field(value, "base64")?)
        .map_err(|_| "measured payload is not base64".to_owned())?;
    if raw.len() > MAX_RECORD || uint_field(value, "bytes")? != raw.len() as u64 {
        return Err("measured byte count differs".into());
    }
    if str_field(value, "sha256")? != digest(&raw) {
        return Err("measured digest differs".into());
    }
    Ok(raw)
}

fn is_sid(value: &str) -> bool {
    value.strip_prefix("S-1-").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'-')
    })
}

fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// Security descriptor from the PowerShell ACL reader, with exact owner, group and DACL text.
#[derive(Clone, PartialEq)]
pub struct Acl {
    pub sddl: String,
    pub protected: bool,
    pub readonly: bool,
}

impl Acl {
    pub fn from_json(value: &Value) -> Result<Self, String> {
        if !value.as_object().is_some_and(|v| v.len() == 3) {
            return Err("ACL field set differs".into());
        }
        Ok(Self {
            sddl: str_field(value, "sddl")?.to_owned(),
            protected: bool_field(value, "protected")?,
            readonly: bool_field(value, "readonly")?,
        })
    }

    /// Replaces only the recorded `G:<PRIMARY_GROUP>` field with the independently observed group.
    fn expand(&self, group: &str) -> Result<Self, String> {
        if self.sddl.matches(GROUP_MARKER).count() != 1
            || self.sddl.matches('<').count() != 1
            || !self.sddl.starts_with("O:LAG:<PRIMARY_GROUP>")
        {
            return Err("unexpected ACL placeholder or owner".into());
        }
        if !is_sid(group) {
            return Err("primary group is not a SID".into());
        }
        Ok(Self {
            sddl: self.sddl.replacen(GROUP_MARKER, &format!("G:{group}"), 1),
            protected: self.protected,
            readonly: self.readonly,
        })
    }
}

/// One case: recorded expectation, or the same fields observed from actual Rust.
#[derive(Clone)]
pub struct Observation {
    pub name: String,
    pub parent_protected: bool,
    pub token_before_bytes: usize,
    pub token_after_bytes: usize,
    pub rotated: bool,
    pub old_accepted: bool,
    pub new_accepted: bool,
    pub before: Acl,
    pub after: Acl,
    pub directory: Acl,
    pub readonly: Acl,
    pub retained: Acl,
    pub failed_exit_code: Option<i32>,
    pub failed_stdout: Vec<u8>,
    pub failed_stderr: Vec<u8>,
    pub retained_token: bool,
    pub sentinel: Vec<u8>,
}

impl Observation {
    pub fn expand_group(&self, group: &str) -> Result<Self, String> {
        let mut expanded = self.clone();
        expanded.before = self.before.expand(group)?;
        expanded.after = self.after.expand(group)?;
        expanded.directory = self.directory.expand(group)?;
        expanded.readonly = self.readonly.expand(group)?;
        expanded.retained = self.retained.expand(group)?;
        Ok(expanded)
    }
}

fn parse_case(value: &Value, name: &str) -> Result<Observation, String> {
    let failed_exit_code = match value.get("failed_exit_code") {
        Some(Value::Null) => None,
        Some(code) => Some(
            i32::try_from(code.as_i64().ok_or("exit code is not an integer")?)
                .map_err(|_| "exit code out of range".to_owned())?,
        ),
        None => return Err("missing failed_exit_code".into()),
    };
    Ok(Observation {
        name: name.to_owned(),
        parent_protected: name == "owner-only-parent",
        token_before_bytes: uint_field(value, "token_before_bytes")? as usize,
        token_after_bytes: uint_field(value, "token_after_bytes")? as usize,
        rotated: bool_field(value, "rotated")?,
        old_accepted: bool_field(value, "old_accepted")?,
        new_accepted: bool_field(value, "new_accepted")?,
        before: Acl::from_json(&value["before_acl"])?,
        after: Acl::from_json(&value["after_acl"])?,
        directory: Acl::from_json(&value["directory_acl"])?,
        readonly: Acl::from_json(&value["readonly_acl"])?,
        retained: Acl::from_json(&value["retained_acl"])?,
        failed_exit_code,
        failed_stdout: measured(&value["failed_stdout"])?,
        failed_stderr: measured(&value["failed_stderr"])?,
        retained_token: bool_field(value, "retained_token")?,
        sentinel: measured(&value["sentinel"])?,
    })
}

/// Semantic checks on one case that do not need the live runtime.
fn validate_case(case: &Observation) -> Result<(), String> {
    let name = case.name.as_str();
    if case.token_before_bytes != 43 || case.token_after_bytes != 43 {
        return Err(format!("{name}: token length differs"));
    }
    if !case.rotated || case.old_accepted || !case.new_accepted {
        return Err(format!("{name}: rotation acceptance differs"));
    }
    if case.before != case.after {
        return Err(format!("{name}: recorded rotation changed the ACL"));
    }
    if case.before.readonly || case.after.readonly || !case.readonly.readonly {
        return Err(format!("{name}: readonly state differs"));
    }
    if case.retained != case.readonly || !case.retained_token {
        return Err(format!("{name}: retained token or ACL differs"));
    }
    if !matches!(case.failed_exit_code, Some(code) if code != 0) {
        return Err(format!("{name}: failed exit status is not nonzero"));
    }
    if !case.failed_stdout.is_empty() {
        return Err(format!("{name}: failed stdout is not empty"));
    }
    if !case.failed_stderr.starts_with(NOTIFY_PREFIX)
        || count_occurrences(&case.failed_stderr, RUNTIME_ROOT.as_bytes()) != 1
    {
        return Err(format!("{name}: failed stderr shape differs"));
    }
    if case.sentinel != SENTINEL {
        return Err(format!("{name}: sentinel bytes differ"));
    }
    for acl in [
        &case.before,
        &case.after,
        &case.directory,
        &case.readonly,
        &case.retained,
    ] {
        acl.expand("S-1-5-32-545")
            .map_err(|reason| format!("{name}: {reason}"))?;
    }
    Ok(())
}

fn verify_pinned(raw: &[u8], pin: &str) -> Result<Value, String> {
    if raw.len() > MAX_RECORD {
        return Err("authenticated record is too large".into());
    }
    if digest(raw) != pin {
        return Err("whole authenticated record digest differs".into());
    }
    serde_json::from_slice(raw).map_err(|_| "authenticated record is not JSON".to_owned())
}

fn check_files(
    record: &Value,
    field: &str,
    expected: &BTreeSet<String>,
    autocrlf: &str,
) -> Result<(), String> {
    let files = record[field]
        .as_object()
        .ok_or_else(|| format!("{field} is missing"))?;
    let names: BTreeSet<String> = files.keys().cloned().collect();
    if names != *expected {
        return Err(format!("{field}: file set differs"));
    }
    for (name, recorded) in files {
        let path = Path::new(name);
        if path.is_absolute() || !path.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(format!("{field}: unsafe path"));
        }
        let archive = filtered(autocrlf, &format!("{PRODUCER_REVISION}:{name}"))?;
        if uint_field(recorded, "bytes")? != archive.len() as u64
            || str_field(recorded, "sha256")? != digest(&archive)
        {
            return Err(format!("{field}: archived bytes differ"));
        }
    }
    Ok(())
}

fn verify_sources(record: &Value, autocrlf: &str) -> Result<(), String> {
    let mut args = vec![
        "ls-tree",
        "-r",
        "-z",
        "--name-only",
        PRODUCER_REVISION,
        "--",
    ];
    args.extend(SOURCE_ROOTS);
    let listed = git(&args)?;
    let mut sources: BTreeSet<String> = listed
        .split(|byte| *byte == 0)
        .filter(|name| name.ends_with(b".go"))
        .map(|name| {
            String::from_utf8(name.to_vec()).map_err(|_| "non-UTF-8 source path".to_owned())
        })
        .collect::<Result<_, _>>()?;
    sources.extend(["go.mod".to_owned(), "go.sum".to_owned()]);
    check_files(record, "source_files", &sources, autocrlf)?;
    let generators: BTreeSet<String> = GENERATORS.iter().map(|name| (*name).to_owned()).collect();
    check_files(record, "archived_generators", &generators, autocrlf)
}

fn verify_record(record: &Value, target: &str) -> Result<Vec<Observation>, String> {
    let (os, arch) = target.split_once('/').ok_or("target is malformed")?;
    if str_field(record, "schema")? != RECORD_SCHEMA
        || str_field(record, "source_revision")? != PRODUCER_REVISION
        || str_field(record, "native_target")? != target
        || str_field(record, "go_version")? != "go1.26.6"
        || str_field(record, "test")? != TEST
        || str_field(record, "test_file")? != TEST_FILE
    {
        return Err("record identity differs".into());
    }
    let autocrlf = str_field(record, "checkout_autocrlf")?;
    if !["true", "false", "input"].contains(&autocrlf) {
        return Err("checkout newline policy differs".into());
    }
    let build = str_field(record, "embedded_build_info")?;
    if !build.starts_with("<LOCAL_BINARY_PATH>:") {
        return Err("archived build info prefix differs".into());
    }
    for required in [
        format!("vcs.revision={PRODUCER_REVISION}"),
        "vcs.modified=false".to_owned(),
        format!("GOOS={os}"),
        format!("GOARCH={arch}"),
    ] {
        if !build.contains(&required) {
            return Err("archived build info differs".into());
        }
    }
    verify_sources(record, autocrlf)?;

    let observation = &record["observation"];
    if observation["assertions_completed"] != true {
        return Err("recorded assertions are incomplete".into());
    }
    let script = measured(&observation["acl_script"])?;
    if script != filtered(autocrlf, &format!("{PRODUCER_REVISION}:{SCRIPT}"))? {
        return Err("measured ACL script differs from the producer blob".into());
    }
    let cases = observation["cases"].as_array().ok_or("cases are missing")?;
    if cases.len() != 2 {
        return Err("case count differs".into());
    }
    let names: Vec<&str> = cases
        .iter()
        .map(|case| str_field(case, "case"))
        .collect::<Result<_, _>>()?;
    if names != ["inherited", "owner-only-parent"] {
        return Err("case set differs".into());
    }
    let mut parsed = Vec::with_capacity(cases.len());
    for case in cases {
        if str_field(case, "runtime_root")? != RUNTIME_ROOT {
            return Err("runtime root placeholder differs".into());
        }
        let observation = parse_case(case, str_field(case, "case")?)?;
        validate_case(&observation)?;
        parsed.push(observation);
    }
    Ok(parsed)
}

fn verify_privacy(target: &str, pin: &str, derived_bytes: usize) -> Result<(), String> {
    let privacy = verify_pinned(PRIVACY, PRIVACY_PIN)?;
    let record = privacy["records"]
        .as_array()
        .ok_or("privacy records are missing")?
        .iter()
        .find(|record| record["target"].as_str() == Some(target))
        .ok_or("no privacy record for target")?;
    if record["derived_sha256"].as_str() != Some(pin)
        || uint_field(record, "derived_bytes")? != derived_bytes as u64
        || uint_field(record, "cases")? != 2
    {
        return Err("privacy derivation does not bind this sidecar".into());
    }
    for role in [
        "run_account_is_rid500",
        "rid500_is_sddl_la",
        "primary_group_distinct_from_user",
    ] {
        if !bool_field(&record["roles"], role)? {
            return Err("privacy role relationship differs".into());
        }
    }
    Ok(())
}

/// Authenticated expectations for one target. Unknown targets fail; there is no live Go fallback.
pub fn load(target: &str) -> Result<Vec<Observation>, String> {
    reject_retired_env()?;
    let &(_, raw, pin) = RECORDS
        .iter()
        .find(|record| record.0 == target)
        .ok_or("no authenticated token ACL record for this target")?;
    let record = verify_pinned(raw, pin)?;
    verify_privacy(target, pin, raw.len())?;
    verify_record(&record, target)
}

/// Exact comparison of one actual Rust case with its retained expectation.
pub fn compare(expected: &Observation, actual: &Observation) -> Result<(), String> {
    let name = expected.name.as_str();
    macro_rules! same {
        ($field:ident) => {
            if expected.$field != actual.$field {
                return Err(format!("{name}: {} differs", stringify!($field)));
            }
        };
    }
    same!(name);
    same!(parent_protected);
    same!(token_before_bytes);
    same!(token_after_bytes);
    same!(rotated);
    same!(old_accepted);
    same!(new_accepted);
    same!(before);
    same!(after);
    same!(directory);
    same!(readonly);
    same!(retained);
    same!(failed_exit_code);
    same!(failed_stdout);
    same!(failed_stderr);
    same!(retained_token);
    same!(sentinel);
    if actual.before != actual.after {
        return Err(format!("{name}: rotation changed the file ACL"));
    }
    if actual.retained != actual.readonly {
        return Err(format!("{name}: retained ACL differs from read-only ACL"));
    }
    Ok(())
}

/// Accepts the single independently verified identity baseline that the test process inherits.
pub fn check_identity_baseline(
    baseline: &Value,
    expected_arch: &str,
    current_revision: &str,
    script_hash: &str,
) -> Result<String, String> {
    if str_field(baseline, "schema")? != IDENTITY_SCHEMA {
        return Err("identity baseline schema differs".into());
    }
    if str_field(baseline, "source_revision")? != current_revision
        || str_field(baseline, "script_sha256")? != script_hash
    {
        return Err("identity baseline revision differs".into());
    }
    let host = &baseline["host"];
    if str_field(host, "platform")? != "Windows"
        || str_field(host, "expected_arch")? != expected_arch
    {
        return Err("identity baseline host differs".into());
    }
    let machine = match expected_arch {
        "X64" => "0x8664",
        "ARM64" => "0xAA64",
        _ => return Err("unsupported identity architecture".into()),
    };
    if str_field(host, "native_machine")? != machine
        || str_field(host, "process_machine")? != "0x0000"
        || uint_field(host, "pointer_size")? != 8
    {
        return Err("identity baseline is not native".into());
    }
    let checks = baseline["checks"]
        .as_object()
        .ok_or("missing identity checks")?;
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
    if checks.keys().cloned().collect::<BTreeSet<_>>() != expected
        || !checks.values().all(|v| v == true)
    {
        return Err("identity baseline checks differ".into());
    }
    let user = str_field(&baseline["process"], "user_sid")?;
    let group = str_field(&baseline["process"], "primary_group_sid")?;
    let rid500 = str_field(&baseline["local_admin"], "rid500_sid")?;
    let la = str_field(&baseline["local_admin"], "sddl_la_sid")?;
    if ![user, group, rid500, la].iter().all(|sid| is_sid(sid)) {
        return Err("identity SID shape differs".into());
    }
    if user != rid500 || user != la || !user.ends_with("-500") {
        return Err("baseline user is not RID 500 and LA".into());
    }
    if group == user {
        return Err("baseline primary group equals user".into());
    }
    Ok(group.to_owned())
}

/// Replaces exactly the owned runtime root; every other diagnostic byte is kept.
pub fn normalize_runtime_root(stderr: &[u8], root: &Path) -> Vec<u8> {
    let root = root.to_string_lossy();
    let needle = root.as_bytes();
    assert!(!needle.is_empty(), "owned runtime root required");
    let mut out = Vec::with_capacity(stderr.len());
    let mut index = 0;
    let mut replacements = 0;
    while index < stderr.len() {
        if stderr[index..].starts_with(needle)
            && matches!(stderr.get(index + needle.len()), Some(b'/' | b'\\'))
        {
            out.extend_from_slice(RUNTIME_ROOT.as_bytes());
            index += needle.len();
            replacements += 1;
        } else {
            out.push(stderr[index]);
            index += 1;
        }
    }
    assert_eq!(
        replacements, 1,
        "exactly one owned runtime-root path required"
    );
    out
}

pub fn reject_retired_env() -> Result<(), String> {
    let capture = std::env::var_os("SYMERASEME_CAPTURE_HTTP_AUX");
    let live = std::env::var_os("SYMERASEME_PARITY_LIVE_GO");
    reject_retired_optins(
        capture.as_ref().map(|v| v.to_string_lossy()).as_deref(),
        live.as_ref().map(|v| v.to_string_lossy()).as_deref(),
    )
}

/// Retired capture/live opt-ins are rejected, never silently ignored.
pub fn reject_retired_optins(capture: Option<&str>, live_go: Option<&str>) -> Result<(), String> {
    if capture.is_some() {
        return Err("retired HTTP capture opt-in is not accepted".into());
    }
    match live_go {
        None | Some("0") => Ok(()),
        Some(_) => Err("retired live Go opt-in is not accepted".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Well-known SID used only for comparator unit inputs, not native evidence.
    const GROUP: &str = "S-1-5-32-545";

    fn rejects(expected: &Observation, edit: impl FnOnce(&mut Observation)) -> bool {
        let mut actual = expected.clone();
        edit(&mut actual);
        compare(expected, &actual).is_err()
    }

    #[test]
    fn authentic_records_verify_on_every_host() {
        for target in ["windows/amd64", "windows/arm64"] {
            let cases = load(target).unwrap();
            assert_eq!(cases.len(), 2);
            assert!(!cases[0].parent_protected);
            assert!(cases[1].parent_protected);
            for case in &cases {
                case.expand_group(GROUP).unwrap();
            }
        }
    }

    #[test]
    fn pinned_bytes_reject_append_truncate_and_bad_manifest() {
        let (_, raw, pin) = RECORDS[0];
        assert!(verify_pinned(raw, pin).is_ok());
        let mut appended = raw.to_vec();
        appended.push(b'\n');
        assert!(verify_pinned(&appended, pin).is_err());
        assert!(verify_pinned(&raw[..raw.len() - 1], pin).is_err());
        let mut privacy = PRIVACY.to_vec();
        privacy.push(b' ');
        assert!(verify_pinned(&privacy, PRIVACY_PIN).is_err());
        assert!(verify_privacy("windows/amd64", RECORDS[1].2, raw.len()).is_err());
    }

    #[test]
    fn record_semantics_reject_case_count_and_owner_mutation() {
        let (_, raw, pin) = RECORDS[0];
        let mut dropped = verify_pinned(raw, pin).unwrap();
        dropped["observation"]["cases"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(verify_record(&dropped, "windows/amd64").is_err());
        let mut owner = verify_pinned(raw, pin).unwrap();
        let sddl = owner["observation"]["cases"][0]["before_acl"]["sddl"]
            .as_str()
            .unwrap()
            .replacen("O:LAG", "O:SYG", 1);
        owner["observation"]["cases"][0]["before_acl"]["sddl"] = Value::String(sddl);
        assert!(verify_record(&owner, "windows/amd64").is_err());
    }

    #[test]
    fn retired_optins_are_rejected() {
        assert!(reject_retired_optins(Some("1"), None).is_err());
        assert!(reject_retired_optins(None, Some("1")).is_err());
        assert!(reject_retired_optins(Some(""), None).is_err());
        assert!(reject_retired_optins(None, Some("0")).is_ok());
        assert!(reject_retired_optins(None, None).is_ok());
        assert!(Acl::from_json(&serde_json::json!({"sddl":"O:LAG:SYD:","protected":false,"readonly":false,"extra":true})).is_err());
    }

    #[test]
    fn comparator_rejects_every_semantic_mutation() {
        let expected = load("windows/amd64")
            .unwrap()
            .remove(0)
            .expand_group(GROUP)
            .unwrap();
        assert!(compare(&expected, &expected.clone()).is_ok());
        assert!(rejects(&expected, |o| o.name = "wrong-case".into()));
        assert!(rejects(&expected, |o| o.parent_protected = !o.parent_protected));
        assert!(rejects(&expected, |o| o.before.sddl =
            o.before.sddl.replacen("O:LAG", "O:SYG", 1)));
        assert!(rejects(&expected, |o| o.directory.sddl =
            o.directory.sddl.replacen(GROUP, "S-1-5-32-546", 1)));
        assert!(rejects(&expected, |o| o.before.sddl = o
            .before
            .sddl
            .replacen("(A;ID;FA;;;LA)", "(A;ID;FR;;;LA)", 1)));
        assert!(rejects(&expected, |o| o.readonly.readonly = false));
        assert!(rejects(&expected, |o| o.failed_exit_code = Some(0)));
        assert!(rejects(&expected, |o| o.failed_exit_code = None));
        assert!(rejects(&expected, |o| o.failed_stderr.push(b'x')));
        assert!(rejects(&expected, |o| o.failed_stdout = b"x".to_vec()));
        assert!(rejects(&expected, |o| o.token_after_bytes = 42));
        assert!(rejects(&expected, |o| o.rotated = false));
        assert!(rejects(&expected, |o| o.retained.sddl.push(' ')));
        assert!(rejects(&expected, |o| o.retained_token = false));
        assert!(rejects(&expected, |o| o.sentinel = b"changed".to_vec()));
        assert!(rejects(&expected, |o| o.after.protected = true));
    }

    #[test]
    fn expansion_rejects_bad_group_and_placeholder() {
        let expected = load("windows/amd64").unwrap().remove(0);
        assert!(expected.expand_group("not-a-sid").is_err());
        let mut no_marker = expected.clone();
        no_marker.before.sddl = no_marker
            .before
            .sddl
            .replacen(GROUP_MARKER, "G:S-1-5-32-545", 1);
        assert!(no_marker.expand_group(GROUP).is_err());
    }

    #[test]
    fn identity_baseline_relationships_are_exact() {
        // Synthetic parser inputs only. Actual Windows re-observation is separate.
        let user = ["S-1-5-", "21-1-2-3-500"].concat();
        let current_revision = "f".repeat(40);
        let script_hash = "e".repeat(64);
        let mut good = serde_json::json!({
            "schema": IDENTITY_SCHEMA, "source_revision": current_revision,
            "script_sha256": script_hash,
            "host": {"platform":"Windows", "expected_arch":"X64", "native_machine":"0x8664", "process_machine":"0x0000", "pointer_size":8},
            "process": {"user_sid":user, "primary_group_sid":GROUP},
            "local_admin": {"rid500_sid":user, "sddl_la_sid":user}, "checks": {}
        });
        for key in [
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
            good["checks"][key] = true.into();
        }
        assert!(
            check_identity_baseline(&good, "X64", &current_revision, &script_hash).is_ok(),
            "fresh replay identities must not be bound to the historical capture revision"
        );
        assert!(check_identity_baseline(&good, "X64", PRODUCER_REVISION, &script_hash).is_err());
        for pointer in [
            "/schema",
            "/script_sha256",
            "/host/expected_arch",
            "/host/native_machine",
            "/host/process_machine",
            "/host/pointer_size",
            "/process/user_sid",
            "/process/primary_group_sid",
            "/local_admin/rid500_sid",
            "/local_admin/sddl_la_sid",
            "/checks/source_tree_clean",
        ] {
            let mut bad = good.clone();
            *bad.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(check_identity_baseline(&bad, "X64", &current_revision, &script_hash).is_err());
        }
        let mut bad = good.clone();
        bad["process"]["primary_group_sid"] = user.into();
        assert!(check_identity_baseline(&bad, "X64", &current_revision, &script_hash).is_err());
        good["host"]["expected_arch"] = "ARM64".into();
        good["host"]["native_machine"] = "0xAA64".into();
        assert!(check_identity_baseline(&good, "ARM64", &current_revision, &script_hash).is_ok());
    }

    #[test]
    fn runtime_root_normalization_is_exact_and_keeps_bytes() {
        let out = normalize_runtime_root(
            b"write MCP auth token: open /x/root/data/mcp_token: denied\r\n",
            Path::new("/x/root"),
        );
        assert_eq!(
            out,
            b"write MCP auth token: open <RUNTIME_ROOT>/data/mcp_token: denied\r\n"
        );
        assert!(
            std::panic::catch_unwind(|| normalize_runtime_root(
                b"/x/rootish",
                Path::new("/x/root")
            ))
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| normalize_runtime_root(
                b"/x/root/a /x/root/b",
                Path::new("/x/root")
            ))
            .is_err()
        );
    }
}
