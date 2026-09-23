//! CLI-019: replay Go's scheduler, report, and dashboard filesystem fixtures.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

type Manifest = BTreeMap<String, Value>;

fn unique_root() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "symeraseme-cli019-{}-{nonce}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_symeraseme-rust")
        .or_else(|| std::env::var_os("CARGO_BIN_EXE_symeraseme_rust"))
        .map(PathBuf::from)
        .expect("Cargo provides the CLI binary path")
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fold_root(bytes: &[u8], root: &Path) -> Vec<u8> {
    let needle = root.to_string_lossy();
    if needle.is_empty() {
        return bytes.to_vec();
    }
    String::from_utf8_lossy(bytes)
        .replace(needle.as_ref(), "<ORACLE_ROOT>")
        .into_bytes()
}

fn run(case: &Value, root: &Path, id: &str) -> Output {
    let case_root = root.join("filesystem").join(id);
    let home = case_root.join("home");
    let cwd = case_root.join("cwd");
    let empty_path = case_root.join("empty-path");
    for path in [
        &home,
        &cwd,
        &empty_path,
        &case_root.join("xdg-config"),
        &case_root.join("xdg-data"),
        &case_root.join("xdg-cache"),
        &case_root.join("tmp"),
        &case_root.join("data"),
        &case_root.join("db"),
        &case_root.join("config"),
    ] {
        fs::create_dir_all(path).expect("isolated case directory");
    }
    if let Some(preconditions) = case.get("preconditions").and_then(Value::as_array) {
        for precondition in preconditions {
            let relative = precondition["path"].as_str().expect("precondition path");
            let path = case_root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("precondition parent directory");
            }
            fs::write(
                &path,
                precondition["content"]
                    .as_str()
                    .expect("precondition content"),
            )
            .expect("precondition file");
            #[cfg(unix)]
            if let Some(mode) = precondition["mode"].as_str() {
                use std::os::unix::fs::PermissionsExt;
                let mode =
                    u32::from_str_radix(mode.strip_prefix("0o").expect("octal file mode"), 8)
                        .expect("valid octal file mode");
                fs::set_permissions(&path, fs::Permissions::from_mode(mode))
                    .expect("precondition file mode");
            }
        }
    }
    let argv = case["process"]["argv"]
        .as_array()
        .expect("process argv")
        .iter()
        .map(|arg| {
            arg.as_str()
                .expect("string argv")
                .replace("<ORACLE_ROOT>", &root.to_string_lossy())
        })
        .collect::<Vec<_>>();
    let mut command = Command::new(binary());
    command
        .args(argv)
        .current_dir(cwd)
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", case_root.join("xdg-config"))
        .env("XDG_DATA_HOME", case_root.join("xdg-data"))
        .env("XDG_CACHE_HOME", case_root.join("xdg-cache"))
        .env("TMPDIR", case_root.join("tmp"))
        .env("SYMERASEME_DATA_DIR", case_root.join("data"))
        .env("SYMERASEME_DB_DIR", case_root.join("db"))
        .env("SYMERASEME_CONFIG_DIR", case_root.join("config"))
        .env("SYMERASEME_ENCRYPT_DB", "0")
        .env("SYMERASEME_RESOURCES", "")
        .env("ANTHROPIC_API_KEY", "")
        .env("CAPSOLVER_API_KEY", "")
        .env("SYMERASEME_IDENTITY_MASTER_KEY", "")
        .env("PATH", empty_path)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::null());
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command.output().expect("isolated CLI process runs")
}

fn manifest(directory: &Path, root: &Path) -> Manifest {
    let mut entries = BTreeMap::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(parent) = pending.pop() {
        for entry in fs::read_dir(parent).expect("read generated output") {
            let path = entry.expect("generated entry").path();
            let metadata = fs::symlink_metadata(&path).expect("generated metadata");
            let relative = path
                .strip_prefix(directory)
                .expect("entry beneath output root")
                .to_string_lossy()
                .replace('\\', "/");
            let mut record = json!({"path": relative});
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                record["mode"] = format!("0o{:o}", metadata.permissions().mode() & 0o777).into();
            }
            if metadata.is_dir() {
                record["type"] = "directory".into();
                pending.push(path);
            } else {
                assert!(metadata.is_file(), "unexpected generated entry: {path:?}");
                let bytes = fold_root(&fs::read(path).expect("generated file bytes"), root);
                record["type"] = "file".into();
                record["size_bytes"] = bytes.len().into();
                record["sha256"] = digest(&bytes).into();
            }
            entries.insert(relative, record);
        }
    }
    entries
}

fn decode_base64(input: &str) -> Vec<u8> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::new();
    let mut bits = 0u32;
    let mut count = 0u8;
    for byte in input.bytes().filter(|byte| *byte != b'=') {
        let value = alphabet
            .iter()
            .position(|candidate| *candidate == byte)
            .expect("valid base64 artifact evidence") as u32;
        bits = (bits << 6) | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            output.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    output
}

fn normalize_timestamps(bytes: &[u8]) -> (Vec<u8>, usize) {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut count = 0;
    let mut index = 0;
    while index < bytes.len() {
        let candidate = bytes.get(index..index + 20);
        let is_timestamp = candidate.is_some_and(|value| {
            value[0..4].iter().all(u8::is_ascii_digit)
                && value[4] == b'-'
                && value[5..7].iter().all(u8::is_ascii_digit)
                && value[7] == b'-'
                && value[8..10].iter().all(u8::is_ascii_digit)
                && value[10] == b' '
                && value[11..13].iter().all(u8::is_ascii_digit)
                && value[13] == b':'
                && value[14..16].iter().all(u8::is_ascii_digit)
                && value[16..20] == *b" UTC"
        });
        if is_timestamp {
            normalized.extend_from_slice(b"<TIMESTAMP>");
            count += 1;
            index += 20;
        } else {
            normalized.push(bytes[index]);
            index += 1;
        }
    }
    (normalized, count)
}

fn replay(case: &Value) {
    let id = case["id"].as_str().expect("case id");
    let root = unique_root();
    fs::create_dir_all(&root).expect("isolated runtime root");
    let _cleanup = Cleanup(root.clone());
    let output = run(case, &root, id);
    let process = &case["process"];
    assert_eq!(
        output.status.code().map(i64::from),
        process["exit_code"].as_i64(),
        "{id}: process exit"
    );
    for (stream, bytes) in [
        ("stdout", output.stdout.as_slice()),
        ("stderr", output.stderr.as_slice()),
    ] {
        let folded = fold_root(bytes, &root);
        assert_eq!(
            folded.len() as u64,
            process[format!("{stream}_bytes")]
                .as_u64()
                .expect("stream length"),
            "{id}: {stream} length"
        );
        assert_eq!(
            digest(&folded),
            process[format!("{stream}_sha256")]
                .as_str()
                .expect("stream digest"),
            "{id}: {stream} digest: {}",
            String::from_utf8_lossy(&folded)
        );
    }
    for (name, recorded_root) in case["manifest_roots"].as_object().expect("manifest roots") {
        let relative = recorded_root
            .as_str()
            .expect("recorded output root")
            .strip_prefix("<ORACLE_ROOT>/")
            .expect("root is isolated");
        let directory = root.join(relative);
        let expected: Manifest = case["manifests"][name]
            .as_array()
            .expect("expected entries")
            .iter()
            .map(|entry| {
                let mut normalized = entry.clone();
                normalized
                    .as_object_mut()
                    .expect("entry object")
                    .remove("content");
                #[cfg(not(unix))]
                normalized
                    .as_object_mut()
                    .expect("entry object")
                    .remove("mode");
                (
                    entry["path"].as_str().expect("entry path").to_owned(),
                    normalized,
                )
            })
            .collect();
        let mut actual = manifest(&directory, &root);
        for (path, record) in &expected {
            if record.get("sha256").is_none() {
                actual
                    .get_mut(path)
                    .expect("actual tree entry")
                    .as_object_mut()
                    .expect("actual file record")
                    .remove("sha256");
            }
        }
        assert_eq!(actual, expected, "{id}: {name} tree");
        if let Some(evidence) = case.get("artifact_evidence") {
            let artifact_path = directory.join(
                evidence["path"]
                    .as_str()
                    .expect("artifact path")
                    .strip_prefix("reports/")
                    .expect("artifact beneath report root"),
            );
            let raw = fold_root(&fs::read(artifact_path).expect("artifact bytes"), &root);
            let (normalized, timestamp_count) = normalize_timestamps(&raw);
            assert_eq!(
                timestamp_count,
                evidence["normalizations"][0]["occurrences"]
                    .as_u64()
                    .expect("normalization count") as usize,
                "{id}: rendered timestamp count"
            );
            if id == "report-generate" {
                assert_eq!(
                    timestamp_count, 2,
                    "report timestamps are the only normalized fields"
                );
            }
            let expected_bytes = decode_base64(
                evidence["content_base64"]
                    .as_str()
                    .expect("artifact content evidence"),
            );
            assert_eq!(
                normalized, expected_bytes,
                "{id}: normalized artifact bytes"
            );
            assert_eq!(
                normalized.len() as u64,
                evidence["size_bytes"].as_u64().expect("artifact size"),
                "{id}: normalized artifact size"
            );
            assert_eq!(
                digest(&normalized),
                evidence["sha256"].as_str().expect("artifact digest"),
                "{id}: normalized artifact digest"
            );
        }
    }
}

#[test]
fn filesystem_generators_match_go_observations() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../rust-tests/parity/cases/filesystem/manifests.json"
    )))
    .expect("filesystem oracle JSON");
    assert_eq!(fixture["commit"], "4e582f28");
    for id in [
        "schedule-generate",
        "schedule-generate-overwrite",
        "report-generate",
        "dashboard-generate",
    ] {
        let case = fixture["cases"]
            .as_array()
            .expect("filesystem cases")
            .iter()
            .find(|case| case["id"] == id)
            .unwrap_or_else(|| panic!("missing filesystem case {id}"));
        replay(case);
    }
}
