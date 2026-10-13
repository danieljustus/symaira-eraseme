//! Strict reader for the committed native Windows HTTP observations at 735ae51e.
//!
//! The recorded real Go observations are the expectations. This reader never runs Go or a
//! Go executable. It authenticates selection.json whole-byte, authenticates every member
//! record whole-byte, validates its schema and measured streams, and binds the producer
//! source/generator closure to immutable Git blobs at PRODUCER_REVISION. Git is read only
//! at that revision, never from the working tree.
#![allow(dead_code)]

use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Component, Path};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

pub const PRODUCER_REVISION: &str = "735ae51ed9c49ca6df88f78c108df8b83f928877";
pub const RUN_ID: u64 = 38_414_186_336;
pub const SELECTION_PIN: &str = "ccb2b8ad8d261539aed6eb13df75a3c99b211688a5767b3afea19d1e6d26945d";
pub const SELECTION: &[u8] = include_bytes!(
    "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/selection.json"
);
const PROVIDER_FIXTURE: &[u8] =
    include_bytes!("../../../../tests/fixtures/provider-cancel/http.json");
const PROVIDER_PATH: &str = "tests/fixtures/provider-cancel/http.json";
const SCHEMA: &str = "symeraseme.actual-go.windows-http-process.v1";
const GO_VERSION: &str = "go1.26.6";
const RUST_VERSION_PREFIX: &str = "rustc 1.98.0 ";
const ENV: &str = "SYMERASEME_CAPTURE_WINDOWS_HTTP";
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const MAX_BYTES: usize = 4 * 1024 * 1024;
const SOURCE_COUNT: usize = 1478;
const GENERATOR_COUNT: usize = 10;
const CRLF_TOTAL: usize = 13;
const SOURCE_CRLF: usize = 3;
const GENERATOR_CRLF: usize = 9;
const ATTRIBUTES_CRLF: usize = 1;
const LOOKUP_SOURCES: [&str; 3] = [
    "go.mod",
    "internal/llm/agent.go",
    "rust-tests/parity/oracle/native-host-agent/main.go",
];

const HTTP: &str = "native_windows_http_matches_checked_out_go";
const HOST: &str = "native_windows_disconnect_reaps_host_agent_like_go";
const PATHEXT: &str = "native_windows_pathext_availability_matches_live_go";
const PROVIDER: &str = "native_windows_disconnect_cancels_provider_request_like_go";
const SLOW: &str = "native_windows_slow_header_timeout_matches_go";
const CTRL_BREAK: &str = "native_windows_ctrl_break_shutdown_matches_go";
const CTRL_C: &str = "native_windows_ctrl_c_shutdown_matches_go";

pub const CASES: [&str; 7] = [HTTP, HOST, PATHEXT, PROVIDER, SLOW, CTRL_BREAK, CTRL_C];

const SOURCE_ROOTS: [&str; 6] = [
    "cmd",
    "internal",
    "regdata.go",
    "registry",
    "rust-tests/parity/oracle/native-host-agent",
    "rust-tests/parity/oracle/provider-cancel",
];

const GENERATOR_INPUTS: [&str; 10] = [
    ".github/workflows/capture-windows-http-retirement.yml",
    "crates/symeraseme-cli/tests/mcp_http_windows_process.rs",
    "crates/symeraseme-cli/tests/support/capture_windows_http_process.rs",
    "crates/symeraseme-cli/tests/support/mcp_http_port.rs",
    "crates/symeraseme-cli/tests/support/native_host_agent.rs",
    "crates/symeraseme-cli/Cargo.toml",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "tests/fixtures/provider-cancel/http.json",
];

const TOP_KEYS: [&str; 20] = [
    ".gitattributes",
    "binaries",
    "case_inventory",
    "case_type",
    "case_version",
    "checkout_autocrlf",
    "console_child_propagation",
    "excluded_ephemeral",
    "generator_inputs",
    "go_env",
    "go_version",
    "native_target",
    "observations",
    "outcome",
    "rust_version",
    "schema",
    "scope",
    "source_files",
    "source_revision",
    "test",
];

/// (target, test, authenticated member bytes). Digests are pinned by selection.json.
const RECORDS: [(&str, &str, &[u8]); 14] = [
    (
        "windows/amd64",
        HTTP,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-amd64/native_windows_http_matches_checked_out_go.json"
        ),
    ),
    (
        "windows/amd64",
        HOST,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-amd64/native_windows_disconnect_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "windows/amd64",
        PATHEXT,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-amd64/native_windows_pathext_availability_matches_live_go.json"
        ),
    ),
    (
        "windows/amd64",
        PROVIDER,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-amd64/native_windows_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "windows/amd64",
        SLOW,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-amd64/native_windows_slow_header_timeout_matches_go.json"
        ),
    ),
    (
        "windows/amd64",
        CTRL_BREAK,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-amd64/native_windows_ctrl_break_shutdown_matches_go.json"
        ),
    ),
    (
        "windows/amd64",
        CTRL_C,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-amd64/native_windows_ctrl_c_shutdown_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        HTTP,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-arm64/native_windows_http_matches_checked_out_go.json"
        ),
    ),
    (
        "windows/arm64",
        HOST,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-arm64/native_windows_disconnect_reaps_host_agent_like_go.json"
        ),
    ),
    (
        "windows/arm64",
        PATHEXT,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-arm64/native_windows_pathext_availability_matches_live_go.json"
        ),
    ),
    (
        "windows/arm64",
        PROVIDER,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-arm64/native_windows_disconnect_cancels_provider_request_like_go.json"
        ),
    ),
    (
        "windows/arm64",
        SLOW,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-arm64/native_windows_slow_header_timeout_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        CTRL_BREAK,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-arm64/native_windows_ctrl_break_shutdown_matches_go.json"
        ),
    ),
    (
        "windows/arm64",
        CTRL_C,
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-735ae51e/windows-http/windows-arm64/native_windows_ctrl_c_shutdown_matches_go.json"
        ),
    ),
];

/// Rejects the retired live-capture switch, including an empty value, before any execution.
pub fn reject_retired_env() -> Result<(), String> {
    reject_retired(std::env::var_os(ENV).as_deref())
}

pub fn reject_retired(value: Option<&OsStr>) -> Result<(), String> {
    match value {
        None => Ok(()),
        Some(_) => Err(format!(
            "{ENV} is retired: live Go capture is not supported after freezing"
        )),
    }
}

pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn str_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be a string"))
}

fn bool_field(value: &Value, key: &str) -> Result<bool, String> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{key} must be a boolean"))
}

fn uint_field(value: &Value, key: &str) -> Result<u64, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{key} must be an unsigned integer"))
}

/// Explicit null and a missing key are different; neither is an accepted string.
fn nullable_string(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        None => Err(format!("{key} is missing")),
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("{key} must be null or a string")),
    }
}

/// The recorded Go classification is JSON null: a missing key or any string is rejected.
fn require_null(value: &Value, key: &str) -> Result<(), String> {
    match value.get(key) {
        Some(Value::Null) => Ok(()),
        Some(_) => Err(format!("{key} must be JSON null")),
        None => Err(format!("{key} is missing")),
    }
}

fn exact_keys(value: &Value, keys: &[&str]) -> Result<(), String> {
    let object = value.as_object().ok_or("observation must be an object")?;
    let got: BTreeSet<&str> = object.keys().map(String::as_str).collect();
    let want: BTreeSet<&str> = keys.iter().copied().collect();
    if got == want {
        Ok(())
    } else {
        Err("observation fields differ".into())
    }
}

/// A measured byte blob: base64 payload, byte count and SHA-256 must agree.
pub fn measured(value: &Value) -> Result<Vec<u8>, String> {
    let object = value
        .as_object()
        .ok_or("measured payload must be an object")?;
    if object.len() != 3
        || !["base64", "bytes", "sha256"]
            .iter()
            .all(|key| object.contains_key(*key))
    {
        return Err("measured payload field set differs".into());
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(str_field(value, "base64")?)
        .map_err(|_| "measured payload is not base64".to_owned())?;
    if raw.len() > MAX_BYTES || uint_field(value, "bytes")? != raw.len() as u64 {
        return Err("measured byte count differs".into());
    }
    if str_field(value, "sha256")? != digest(&raw) {
        return Err("measured digest differs".into());
    }
    Ok(raw)
}

/// Counts and authenticates every measured payload in a record.
fn walk_measured(value: &Value, count: &mut u64) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            if map.contains_key("base64") {
                measured(value)?;
                *count += 1;
                return Ok(());
            }
            for child in map.values() {
                walk_measured(child, count)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                walk_measured(child, count)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn verify_selection(raw: &[u8], pin: &str) -> Result<Value, String> {
    if raw.len() > MAX_BYTES {
        return Err("selection is too large".into());
    }
    if digest(raw) != pin {
        return Err("selection whole-byte pin differs".into());
    }
    let value: Value =
        serde_json::from_slice(raw).map_err(|_| "selection is not JSON".to_owned())?;
    if str_field(&value, "producer")? != PRODUCER_REVISION
        || uint_field(&value, "run")? != RUN_ID
        || uint_field(&value, "source_files_per_record")? != SOURCE_COUNT as u64
        || uint_field(&value, "generator_inputs_per_record")? != GENERATOR_COUNT as u64
        || uint_field(&value, "git_confirmed_crlf_paths")? != CRLF_TOTAL as u64
    {
        return Err("selection identity differs".into());
    }
    if value["records"].as_array().map(Vec::len) != Some(RECORDS.len()) {
        return Err("selection record count differs".into());
    }
    Ok(value)
}

pub struct Entry {
    pub target: String,
    pub test: String,
    pub bytes: u64,
    pub sha256: String,
    pub streams: u64,
}

pub fn selection_entries(selection: &Value) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for record in selection["records"]
        .as_array()
        .ok_or("selection records must be an array")?
    {
        if record.get("case_success") != Some(&Value::Bool(true)) {
            return Err("selection case_success must be true".into());
        }
        let test = str_field(record, "test")?.to_owned();
        if str_field(record, "member")? != format!("{test}.json") {
            return Err("selection member name differs".into());
        }
        str_field(record, "archive")?;
        str_field(record, "archive_sha256")?;
        out.push(Entry {
            target: str_field(record, "target")?.to_owned(),
            bytes: uint_field(record, "bytes")?,
            sha256: str_field(record, "sha256")?.to_owned(),
            streams: uint_field(record, "verified_encoded_streams")?,
            test,
        });
    }
    Ok(out)
}

pub fn member_bytes(target: &str, test: &str) -> Option<&'static [u8]> {
    RECORDS
        .iter()
        .find(|record| record.0 == target && record.1 == test)
        .map(|record| record.2)
}

pub fn authenticate_member(raw: &[u8], entry: &Entry) -> Result<Value, String> {
    if raw.len() > MAX_BYTES || raw.len() as u64 != entry.bytes || digest(raw) != entry.sha256 {
        return Err("member bytes differ from selection pin".into());
    }
    serde_json::from_slice(raw).map_err(|_| "member is not JSON".to_owned())
}

type Variants = Vec<(u64, String)>;

struct Closure {
    sources: BTreeMap<String, Variants>,
    generators: BTreeMap<String, Variants>,
    attributes: Variants,
}

static CLOSURE: OnceLock<Result<Closure, String>> = OnceLock::new();

fn source_closure() -> Result<&'static Closure, String> {
    CLOSURE
        .get_or_init(build_closure)
        .as_ref()
        .map_err(Clone::clone)
}

/// Runs git against the immutable producer object database only. Never Go.
fn git(args: &[&str], stdin: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command
        .arg("--no-replace-objects")
        .args(args)
        .current_dir(ROOT)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| "git is unavailable for the immutable producer pin".to_owned())?;
    // Feed stdin from a thread so a large batch cannot deadlock against stdout.
    let writer = match (stdin, child.stdin.take()) {
        (Some(data), Some(mut pipe)) => Some(std::thread::spawn(move || {
            let _ = pipe.write_all(&data);
        })),
        _ => None,
    };
    let output = child
        .wait_with_output()
        .map_err(|_| "git wait failed".to_owned())?;
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    if !output.status.success() {
        return Err("immutable producer revision is unavailable".into());
    }
    Ok(output.stdout)
}

fn build_closure() -> Result<Closure, String> {
    let mut args = vec![
        "ls-tree",
        "-r",
        "-z",
        "--name-only",
        PRODUCER_REVISION,
        "--",
    ];
    args.extend(SOURCE_ROOTS);
    let listed = git(&args, None)?;
    let mut sources = BTreeSet::new();
    for name in listed
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = std::str::from_utf8(name).map_err(|_| "non-UTF-8 producer path".to_owned())?;
        if name.ends_with(".go")
            || name.starts_with("registry/")
            || name.starts_with("internal/templating/templates/")
            || name == "internal/mcp/tools.json"
        {
            sources.insert(name.to_owned());
        }
    }
    sources.extend(["go.mod".to_owned(), "go.sum".to_owned()]);
    let generators: BTreeSet<String> = GENERATOR_INPUTS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    if sources.len() != SOURCE_COUNT || generators.len() != GENERATOR_COUNT {
        return Err("producer closure size differs".into());
    }
    let mut all: BTreeSet<String> = sources.union(&generators).cloned().collect();
    all.insert(".gitattributes".to_owned());
    for path in &all {
        let parsed = Path::new(path);
        if parsed.is_absolute()
            || !parsed
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || path.contains(['\n', '\r', '\\'])
        {
            return Err("unsafe producer path".into());
        }
    }

    let mut request = Vec::new();
    for path in &all {
        request.extend_from_slice(format!("{PRODUCER_REVISION}:{path}\n").as_bytes());
    }
    let batch = git(&["cat-file", "--batch"], Some(request))?;
    let mut cursor = 0usize;
    let mut blobs: BTreeMap<&str, &[u8]> = BTreeMap::new();
    for path in &all {
        let line_end = batch[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or("cat-file header is truncated")?
            + cursor;
        let header = std::str::from_utf8(&batch[cursor..line_end])
            .map_err(|_| "cat-file header is not UTF-8".to_owned())?;
        let parts: Vec<&str> = header.split(' ').collect();
        if parts.len() != 3 || parts[1] != "blob" {
            return Err("producer blob is missing".into());
        }
        let size: usize = parts[2]
            .parse()
            .map_err(|_| "cat-file size is malformed".to_owned())?;
        let start = line_end + 1;
        let end = start.checked_add(size).ok_or("cat-file size overflows")?;
        if batch.get(end) != Some(&b'\n') {
            return Err("cat-file payload is malformed".into());
        }
        blobs.insert(path.as_str(), &batch[start..end]);
        cursor = end + 1;
    }
    if cursor != batch.len() {
        return Err("unexpected trailing cat-file output".into());
    }

    let mut request = Vec::new();
    for path in &all {
        request.extend_from_slice(path.as_bytes());
        request.push(0);
    }
    let raw_attrs = git(
        &[
            "check-attr",
            "-z",
            "--source",
            PRODUCER_REVISION,
            "--stdin",
            "text",
            "eol",
        ],
        Some(request),
    )?;
    let body = raw_attrs
        .strip_suffix(&[0u8])
        .ok_or("check-attr output is malformed")?;
    let fields: Vec<&[u8]> = body.split(|byte| *byte == 0).collect();
    if fields.len() != all.len() * 6 {
        return Err("check-attr output is malformed".into());
    }
    let mut attrs: BTreeMap<String, (String, String)> = BTreeMap::new();
    for triple in fields.as_chunks::<3>().0 {
        let path = std::str::from_utf8(triple[0])
            .map_err(|_| "check-attr path is not UTF-8".to_owned())?;
        let value = std::str::from_utf8(triple[2])
            .map_err(|_| "check-attr value is not UTF-8".to_owned())?;
        let entry = attrs.entry(path.to_owned()).or_default();
        match std::str::from_utf8(triple[1])
            .map_err(|_| "check-attr name is not UTF-8".to_owned())?
        {
            "text" => entry.0 = value.to_owned(),
            "eol" => entry.1 = value.to_owned(),
            _ => return Err("check-attr attribute differs".into()),
        }
    }

    // Raw bytes first, then the LF-to-CRLF checkout variant when Git may convert the path.
    let mut variants: BTreeMap<String, Variants> = BTreeMap::new();
    for path in &all {
        let raw = blobs[path.as_str()];
        let (text, eol) = attrs.get(path).ok_or("check-attr entry is missing")?;
        let mut list = vec![(raw.len() as u64, digest(raw))];
        if text.as_str() != "unset"
            && eol.as_str() != "lf"
            && !raw.contains(&0)
            && !raw.contains(&b'\r')
        {
            let crlf: Vec<u8> = raw
                .iter()
                .flat_map(|byte| {
                    if *byte == b'\n' {
                        vec![b'\r', b'\n']
                    } else {
                        vec![*byte]
                    }
                })
                .collect();
            list.push((crlf.len() as u64, digest(&crlf)));
        }
        variants.insert(path.clone(), list);
    }

    let pick = |names: &BTreeSet<String>| -> BTreeMap<String, Variants> {
        names
            .iter()
            .map(|name| (name.clone(), variants[name].clone()))
            .collect()
    };
    Ok(Closure {
        sources: pick(&sources),
        generators: pick(&generators),
        attributes: variants[".gitattributes"].clone(),
    })
}

/// Validated closure facts for the portable controls.
pub struct ClosureSummary {
    pub sources: usize,
    pub generators: usize,
}

pub fn closure_summary() -> Result<ClosureSummary, String> {
    let closure = source_closure()?;
    Ok(ClosureSummary {
        sources: closure.sources.len(),
        generators: closure.generators.len(),
    })
}

/// Immutable producer pin of `.gitattributes` in one checkout form (0 = archived LF bytes).
pub fn attributes_pin(index: usize) -> Result<(u64, String), String> {
    source_closure()?
        .attributes
        .get(index)
        .cloned()
        .ok_or_else(|| "no such .gitattributes variant".to_owned())
}

fn pin_of(value: &Value) -> Result<(u64, String), String> {
    let object = value.as_object().ok_or("pin must be an object")?;
    if object.len() != 2 {
        return Err("pin field set differs".into());
    }
    Ok((
        uint_field(value, "bytes")?,
        str_field(value, "sha256")?.to_owned(),
    ))
}

/// Matches each recorded pin to the producer blob or its CRLF checkout variant.
/// Returns, per name, whether the CRLF variant was the one recorded.
fn check_pins(
    value: &Value,
    field: &str,
    expected: &BTreeMap<String, Variants>,
) -> Result<BTreeMap<String, bool>, String> {
    let files = value[field]
        .as_object()
        .ok_or_else(|| format!("{field} must be an object"))?;
    let got: BTreeSet<&str> = files.keys().map(String::as_str).collect();
    let want: BTreeSet<&str> = expected.keys().map(String::as_str).collect();
    if got != want {
        return Err(format!("{field} file set differs"));
    }
    let mut converted = BTreeMap::new();
    for (name, pin) in files {
        let index = pin_index(pin, &expected[name])?;
        converted.insert(name.clone(), index > 0);
    }
    Ok(converted)
}

fn pin_index(pin: &Value, variants: &Variants) -> Result<usize, String> {
    let (bytes, sha) = pin_of(pin)?;
    variants
        .iter()
        .position(|candidate| candidate.0 == bytes && candidate.1 == sha)
        .ok_or_else(|| "pin differs from producer blob".to_owned())
}

fn roles_for(test: &str) -> BTreeSet<&'static str> {
    let mut roles: BTreeSet<&'static str> = ["go_oracle", "rust_symeraseme"].into_iter().collect();
    if test.contains("pathext") {
        roles = ["go_native_host_agent", "rust_test_harness"]
            .into_iter()
            .collect();
    } else if test.contains("reaps_host_agent") {
        roles.insert("go_native_host_agent");
    } else if test.contains("ctrl_") {
        roles.insert("rust_test_harness");
    }
    roles
}

fn categories(test: &str) -> Result<&'static [&'static str], String> {
    Ok(match test {
        HTTP => &["http"],
        HOST => &["host_agent", "persisted_classification"],
        PATHEXT => &["pathext"],
        PROVIDER => &["persisted_classification", "provider", "provider_fixture"],
        SLOW => &[
            "health_after_slow_header",
            "servers_alive_after_slow_header",
            "slow_header",
        ],
        CTRL_BREAK | CTRL_C => &["console_children", "console_parent"],
        _ => return Err("unknown test case".into()),
    })
}

/// Validates one authenticated member record against the recorded producer closure.
/// `streams` is the selection's verified encoded-stream count for this record.
pub fn validate_record(
    value: &Value,
    target: &str,
    test: &str,
    streams: u64,
) -> Result<(), String> {
    let closure = source_closure()?;
    let object = value.as_object().ok_or("record must be a JSON object")?;
    let keys: BTreeSet<&str> = object.keys().map(String::as_str).collect();
    if keys != TOP_KEYS.iter().copied().collect::<BTreeSet<&str>>() {
        return Err("record field set differs".into());
    }
    let (os, arch) = target.split_once('/').ok_or("target is malformed")?;
    if str_field(value, "schema")? != SCHEMA {
        return Err("schema differs".into());
    }
    if str_field(value, "source_revision")? != PRODUCER_REVISION {
        return Err("source_revision differs".into());
    }
    if str_field(value, "native_target")? != target {
        return Err("native_target differs".into());
    }
    if str_field(value, "test")? != test || !CASES.contains(&test) {
        return Err("test differs".into());
    }
    if str_field(value, "outcome")? != "completed" {
        return Err("outcome differs".into());
    }
    if str_field(value, "case_type")? != "native_windows_process_comparator" {
        return Err("case_type differs".into());
    }
    if uint_field(value, "case_version")? != 1 {
        return Err("case_version differs".into());
    }
    if str_field(value, "go_version")? != GO_VERSION {
        return Err("go_version differs".into());
    }
    if !str_field(value, "rust_version")?.starts_with(RUST_VERSION_PREFIX) {
        return Err("rust_version differs".into());
    }
    if str_field(value, "checkout_autocrlf")? != "true" {
        return Err("checkout_autocrlf must be a string \"true\"".into());
    }
    if value["go_env"]
        != json!({"GOARCH": arch, "GOHOSTARCH": arch, "GOHOSTOS": os, "GOOS": os, "GOVERSION": GO_VERSION})
    {
        return Err("go_env differs".into());
    }
    let inventory = value["case_inventory"]
        .as_array()
        .ok_or("case_inventory must be an array")?
        .iter()
        .map(|case| {
            case.as_str()
                .ok_or("case_inventory entries must be strings")
        })
        .collect::<Result<Vec<&str>, &str>>()?;
    if inventory != CASES {
        return Err("case inventory differs".into());
    }

    let binaries = value["binaries"]
        .as_object()
        .ok_or("binaries must be an object")?;
    let got_roles: BTreeSet<&str> = binaries.keys().map(String::as_str).collect();
    if got_roles != roles_for(test) {
        return Err("binary roles differ".into());
    }
    for (role, binary) in binaries {
        if uint_field(binary, "bytes")? == 0 {
            return Err("binary byte count is empty".into());
        }
        let sha = str_field(binary, "sha256")?;
        if sha.len() != 64
            || !sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("binary digest is malformed".into());
        }
        if role.starts_with("go_") {
            let info = binary
                .get("embedded_build_info")
                .ok_or("embedded build info is missing")?;
            if str_field(info, "embedded_go_version")? != GO_VERSION {
                return Err("embedded Go version differs".into());
            }
            let build = str_field(info, "buildinfo_without_executable_heading")?;
            for setting in [
                format!("vcs.revision={PRODUCER_REVISION}"),
                "vcs.modified=false".to_owned(),
                format!("GOOS={os}"),
                format!("GOARCH={arch}"),
            ] {
                if !build.contains(format!("\tbuild\t{setting}\n").as_str()) {
                    return Err("embedded build settings differ".into());
                }
            }
        }
    }

    let source_pins = check_pins(value, "source_files", &closure.sources)?;
    let generator_pins = check_pins(value, "generator_inputs", &closure.generators)?;
    let attribute_index = pin_index(
        value
            .get(".gitattributes")
            .ok_or(".gitattributes is missing")?,
        &closure.attributes,
    )
    .map_err(|_| ".gitattributes pin differs from producer blob".to_owned())?;
    let sources_crlf = source_pins.values().filter(|converted| **converted).count();
    let generators_crlf = generator_pins
        .values()
        .filter(|converted| **converted)
        .count();
    let attributes_crlf = usize::from(attribute_index > 0);
    if sources_crlf != SOURCE_CRLF
        || generators_crlf != GENERATOR_CRLF
        || attributes_crlf != ATTRIBUTES_CRLF
        || sources_crlf + generators_crlf + attributes_crlf != CRLF_TOTAL
    {
        return Err("CRLF checkout pins differ".into());
    }
    if generator_pins.get(PROVIDER_PATH) == Some(&true) {
        return Err("provider fixture must remain LF".into());
    }

    let observations = value["observations"]
        .as_object()
        .ok_or("observations must be an object")?;
    let got: BTreeSet<&str> = observations.keys().map(String::as_str).collect();
    let want: BTreeSet<&str> = categories(test)?.iter().copied().collect();
    if got != want {
        return Err("observation categories differ".into());
    }
    let mut seen = 0u64;
    walk_measured(value, &mut seen)?;
    if seen != streams {
        return Err("measured stream count differs".into());
    }
    match test {
        HTTP => {
            http_cases(value)?;
        }
        HOST => {
            host_go(value)?;
        }
        PATHEXT => {
            pathext_go(value)?;
        }
        PROVIDER => {
            provider_go(value)?;
        }
        SLOW => {
            slow_go(value)?;
        }
        CTRL_BREAK => {
            console_go(value, "Ctrl+Break")?;
        }
        CTRL_C => {
            console_go(value, "Ctrl+C")?;
        }
        _ => return Err("unknown test case".into()),
    }
    Ok(())
}

pub struct Record {
    pub target: String,
    pub test: String,
    pub streams: u64,
    pub value: Value,
}

/// Authenticates selection, member and producer closure, then validates the record.
pub fn load(target: &str, test: &str) -> Result<Record, String> {
    reject_retired_env()?;
    let selection = verify_selection(SELECTION, SELECTION_PIN)?;
    let entries = selection_entries(&selection)?;
    let entry = entries
        .iter()
        .find(|entry| entry.target == target && entry.test == test)
        .ok_or("no authenticated record for this target and test")?;
    let raw =
        member_bytes(target, test).ok_or("no committed record bytes for this target and test")?;
    let value = authenticate_member(raw, entry)?;
    validate_record(&value, target, test, entry.streams)?;
    Ok(Record {
        target: target.to_owned(),
        test: test.to_owned(),
        streams: entry.streams,
        value,
    })
}

fn items<'a>(value: &'a Value, category: &str) -> Result<&'a [Value], String> {
    value["observations"][category]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("observation {category} must be an array"))
}

fn implementation_items<'a>(items: &'a [Value], name: &str) -> Vec<&'a Value> {
    items
        .iter()
        .filter(|item| item.get("implementation").and_then(Value::as_str) == Some(name))
        .collect()
}

pub struct HttpCase {
    pub authorized: bool,
    pub origin: Option<String>,
    pub status: u16,
    pub body: Vec<u8>,
}

/// Recorded request matrix in its exact order: (authorized, origin, status).
const HTTP_MATRIX: [(bool, Option<&str>, u64); 4] = [
    (false, None, 401),
    (true, None, 200),
    (true, Some("https://evil.example"), 403),
    (true, Some(""), 200),
];

/// Recorded PATHEXT inventory in its exact order: (case, pathext, path mode).
pub const PATHEXT_MATRIX: [(&str, &str, &str); 9] = [
    ("exe", ".EXE", "absolute"),
    ("com", ".COM;.EXE", "absolute"),
    ("cmd", ".CMD", "absolute"),
    ("normalized", "EXE", "absolute"),
    ("default", "", "absolute"),
    ("excluded", ".COM", "absolute"),
    ("no-extension", ".EXE", "absolute"),
    ("empty-extension-list", ";", "absolute"),
    ("relative", ".EXE", "relative"),
];

pub fn http_cases(value: &Value) -> Result<Vec<HttpCase>, String> {
    let cases = http_cases_raw(value)?;
    if cases.len() != HTTP_MATRIX.len() {
        return Err("HTTP request matrix count differs".into());
    }
    for (case, (authorized, origin, status)) in cases.iter().zip(HTTP_MATRIX) {
        if case.authorized != authorized
            || case.origin.as_deref() != origin
            || u64::from(case.status) != status
        {
            return Err("HTTP request matrix differs".into());
        }
    }
    Ok(cases)
}

fn http_cases_raw(value: &Value) -> Result<Vec<HttpCase>, String> {
    let items = items(value, "http")?;
    if items.len() != 4 {
        return Err("HTTP observation count differs".into());
    }
    items
        .iter()
        .map(|item| {
            exact_keys(item, &["authorized", "go", "origin"])?;
            let authorized = bool_field(item, "authorized")?;
            let origin = nullable_string(item, "origin")?;
            let go = item.get("go").ok_or("HTTP go observation is missing")?;
            exact_keys(go, &["body", "status"])?;
            let status = u16::try_from(uint_field(go, "status")?)
                .map_err(|_| "HTTP status is out of range".to_owned())?;
            let body = measured(go.get("body").ok_or("HTTP body is missing")?)?;
            Ok(HttpCase {
                authorized,
                origin,
                status,
                body,
            })
        })
        .collect()
}

pub struct HostGo {
    pub before_wait: u32,
    pub after_wait: u32,
    pub alive: bool,
    pub health_status: u16,
    pub health_body: Vec<u8>,
}

pub fn host_go(value: &Value) -> Result<HostGo, String> {
    let host = items(value, "host_agent")?;
    let go = implementation_items(host, "go");
    if go.len() != 4 {
        return Err("host-agent go observations differ".into());
    }
    let phase = |name: &str| -> Result<&Value, String> {
        go.iter()
            .copied()
            .find(|item| item.get("phase").and_then(Value::as_str) == Some(name))
            .ok_or_else(|| format!("host-agent {name} is missing"))
    };
    let before = phase("before_disconnect")?;
    let after = phase("after_disconnect")?;
    exact_keys(before, &["implementation", "phase", "wait_result"])?;
    exact_keys(after, &["implementation", "phase", "wait_result"])?;
    let alive_item = go
        .iter()
        .copied()
        .find(|item| item.get("server_alive_after_disconnect").is_some())
        .ok_or("host-agent liveness is missing")?;
    let alive = bool_field(alive_item, "server_alive_after_disconnect")?;
    let health_item = go
        .iter()
        .copied()
        .find(|item| item.get("health_after_disconnect").is_some())
        .ok_or("host-agent health is missing")?;
    let health = health_item
        .get("health_after_disconnect")
        .ok_or("health is missing")?;
    exact_keys(health, &["body", "status"])?;
    let persisted = items(value, "persisted_classification")?;
    let persisted_go = implementation_items(persisted, "go");
    if persisted_go.len() != 1 {
        return Err("persisted classification go observation differs".into());
    }
    exact_keys(persisted_go[0], &["classified_as", "implementation"])?;
    require_null(persisted_go[0], "classified_as")?;
    Ok(HostGo {
        before_wait: u32::try_from(uint_field(before, "wait_result")?)
            .map_err(|_| "wait result is out of range".to_owned())?,
        after_wait: u32::try_from(uint_field(after, "wait_result")?)
            .map_err(|_| "wait result is out of range".to_owned())?,
        alive,
        health_status: u16::try_from(uint_field(health, "status")?)
            .map_err(|_| "health status is out of range".to_owned())?,
        health_body: measured(health.get("body").ok_or("health body is missing")?)?,
    })
}

pub struct PathextGo {
    pub case: String,
    pub pathext: String,
    pub path_mode: String,
    pub available: bool,
}

pub fn pathext_go(value: &Value) -> Result<Vec<PathextGo>, String> {
    let items = items(value, "pathext")?;
    let go: Vec<&Value> = items
        .iter()
        .filter(|item| item.get("go_found").is_some())
        .collect();
    let rust: Vec<&Value> = items
        .iter()
        .filter(|item| item.get("rust_lookup").is_some())
        .collect();
    if go.len() != 9 || rust.len() != 9 || go.len() + rust.len() != items.len() {
        return Err("PATHEXT observation count differs".into());
    }
    let mut out = Vec::new();
    for (item, (case, pathext, mode)) in go.into_iter().zip(PATHEXT_MATRIX) {
        exact_keys(
            item,
            &[
                "case",
                "go_available",
                "go_found",
                "go_path_nonempty",
                "go_sources_sha256",
                "path_mode",
                "pathext",
            ],
        )?;
        if str_field(item, "case")? != case
            || str_field(item, "pathext")? != pathext
            || str_field(item, "path_mode")? != mode
        {
            return Err("PATHEXT case inventory differs".into());
        }
        bool_field(item, "go_path_nonempty")?;
        let found = bool_field(item, "go_found")?;
        let available = bool_field(item, "go_available")?;
        if found != available {
            return Err("recorded PATHEXT found differs from available".into());
        }
        let path_mode = str_field(item, "path_mode")?;
        if path_mode != "absolute" && path_mode != "relative" {
            return Err("PATHEXT path mode differs".into());
        }
        let sources = item["go_sources_sha256"]
            .as_object()
            .ok_or("PATHEXT source digests must be an object")?;
        let names: BTreeSet<&str> = sources.keys().map(String::as_str).collect();
        if names != LOOKUP_SOURCES.iter().copied().collect::<BTreeSet<&str>>() {
            return Err("PATHEXT source set differs".into());
        }
        for (name, digest_value) in sources {
            if digest_value.as_str() != value["source_files"][name]["sha256"].as_str() {
                return Err("PATHEXT source digest differs from the producer pin".into());
            }
        }
        out.push(PathextGo {
            case: str_field(item, "case")?.to_owned(),
            pathext: str_field(item, "pathext")?.to_owned(),
            path_mode: path_mode.to_owned(),
            available,
        });
    }
    for (item, expected) in rust.into_iter().zip(&out) {
        exact_keys(item, &["case", "rust_lookup"])?;
        if str_field(item, "case")? != expected.case
            || str_field(item, "rust_lookup")? != expected.available.to_string()
        {
            return Err("recorded rust lookup differs".into());
        }
    }
    Ok(out)
}

pub struct ProviderGo {
    pub request_line: String,
    pub upstream: String,
    pub alive: bool,
}

pub fn provider_go(value: &Value) -> Result<ProviderGo, String> {
    let fixture = items(value, "provider_fixture")?;
    if fixture.len() != 1 {
        return Err("provider fixture observation count differs".into());
    }
    exact_keys(&fixture[0], &["generated"])?;
    let generated = measured(
        fixture[0]
            .get("generated")
            .ok_or("provider fixture is missing")?,
    )?;
    if generated != PROVIDER_FIXTURE {
        return Err("recorded provider fixture differs from committed bytes".into());
    }
    let provider = items(value, "provider")?;
    if provider.len() != 6 || implementation_items(provider, "rust").len() != 3 {
        return Err("provider observation count differs".into());
    }
    let go = implementation_items(provider, "go");
    if go.len() != 3 {
        return Err("provider go observations differ".into());
    }
    let mut request_line = None;
    let mut upstream = None;
    let mut alive = None;
    for item in go {
        if item.get("request_line").is_some() {
            exact_keys(item, &["implementation", "request_line"])?;
            request_line = Some(str_field(item, "request_line")?.to_owned());
        } else if item.get("upstream_after_disconnect").is_some() {
            exact_keys(item, &["implementation", "upstream_after_disconnect"])?;
            upstream = Some(str_field(item, "upstream_after_disconnect")?.to_owned());
        } else {
            exact_keys(item, &["implementation", "server_alive_after_disconnect"])?;
            alive = Some(bool_field(item, "server_alive_after_disconnect")?);
        }
    }
    let persisted = items(value, "persisted_classification")?;
    let persisted_go = implementation_items(persisted, "go");
    if persisted_go.len() != 1 {
        return Err("persisted classification go observation differs".into());
    }
    exact_keys(persisted_go[0], &["classified_as", "implementation"])?;
    require_null(persisted_go[0], "classified_as")?;
    Ok(ProviderGo {
        request_line: request_line.ok_or("provider request line is missing")?,
        upstream: upstream.ok_or("provider upstream outcome is missing")?,
        alive: alive.ok_or("provider liveness is missing")?,
    })
}

pub struct SlowGo {
    pub result: String,
    pub elapsed_ms: u64,
    pub health_status: u16,
    pub health_body: Vec<u8>,
}

pub fn slow_go(value: &Value) -> Result<SlowGo, String> {
    let closures = items(value, "slow_header")?;
    let go = implementation_items(closures, "go");
    if go.len() != 1 || implementation_items(closures, "rust").len() != 1 {
        return Err("slow-header observations differ".into());
    }
    exact_keys(go[0], &["closure", "implementation"])?;
    let closure = go[0]
        .get("closure")
        .ok_or("slow-header closure is missing")?;
    let result = str_field(closure, "result")?.to_owned();
    match result.as_str() {
        "eof" => exact_keys(closure, &["elapsed_ms", "response_bytes", "result"])?,
        "error" => exact_keys(closure, &["elapsed_ms", "error_kind", "result"])?,
        _ => return Err("slow-header result differs".into()),
    }
    let elapsed_ms = uint_field(closure, "elapsed_ms")?;
    if !(4500..=8000).contains(&elapsed_ms) {
        return Err("slow-header closure elapsed outside bounds".into());
    }
    let alive = items(value, "servers_alive_after_slow_header")?;
    if alive.len() != 1 {
        return Err("slow-header liveness differs".into());
    }
    exact_keys(&alive[0], &["go", "rust"])?;
    if !bool_field(&alive[0], "go")? || !bool_field(&alive[0], "rust")? {
        return Err("servers were not alive after slow-header closure".into());
    }
    let health = items(value, "health_after_slow_header")?;
    if health.len() != 1 {
        return Err("slow-header health observation differs".into());
    }
    exact_keys(&health[0], &["go"])?;
    let go_health = health[0].get("go").ok_or("slow-header health is missing")?;
    exact_keys(go_health, &["body", "status"])?;
    Ok(SlowGo {
        result,
        elapsed_ms,
        health_status: u16::try_from(uint_field(go_health, "status")?)
            .map_err(|_| "health status is out of range".to_owned())?,
        health_body: measured(go_health.get("body").ok_or("health body is missing")?)?,
    })
}

pub struct ConsoleGo {
    pub event_nonzero: bool,
    pub exit_success: bool,
    pub exit_code: u64,
    pub port_closed: bool,
}

pub fn console_go(value: &Value, signal: &str) -> Result<ConsoleGo, String> {
    let children = items(value, "console_children")?;
    if children
        .iter()
        .filter(|item| item.get("signal").and_then(Value::as_str) == Some(signal))
        .count()
        != 4
    {
        return Err("console observation count differs".into());
    }
    let go = implementation_items(children, "go");
    if go.len() != 2 || implementation_items(children, "rust").len() != 2 {
        return Err("console go observations differ".into());
    }
    let event = go
        .iter()
        .copied()
        .find(|item| item.get("event_result").is_some())
        .ok_or("console event is missing")?;
    exact_keys(event, &["event_result", "implementation", "signal"])?;
    if str_field(event, "signal")? != signal {
        return Err("console signal differs".into());
    }
    let exit = go
        .iter()
        .copied()
        .find(|item| item.get("exit_code").is_some())
        .ok_or("console exit is missing")?;
    exact_keys(
        exit,
        &[
            "exit_code",
            "exit_success",
            "implementation",
            "port_closed",
            "signal",
        ],
    )?;
    if str_field(exit, "signal")? != signal {
        return Err("console signal differs".into());
    }
    let parent = items(value, "console_parent")?;
    if parent.len() != 1 {
        return Err("console parent observation differs".into());
    }
    exact_keys(&parent[0], &["child_exit_code", "child_exit_success"])?;
    if !bool_field(&parent[0], "child_exit_success")? {
        return Err("isolated console child did not succeed".into());
    }
    Ok(ConsoleGo {
        event_nonzero: uint_field(event, "event_result")? != 0,
        exit_success: bool_field(exit, "exit_success")?,
        exit_code: uint_field(exit, "exit_code")?,
        port_closed: bool_field(exit, "port_closed")?,
    })
}
