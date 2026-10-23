//! Opt-in measured native Go observations from the auxiliary HTTP process comparators:
//! disconnect cancellation, startup handoff, bind failures and obs-text Origin.
use base64::Engine;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const SOURCE_ROOTS: [&str; 4] = [
    "cmd",
    "internal",
    "rust-tests/parity/oracle/agent-cancel",
    "rust-tests/parity/oracle/provider-cancel",
];

fn digest(bytes: &[u8]) -> Value {
    let sha256 = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    json!({"bytes": bytes.len(), "sha256": sha256})
}

fn checked(command: &mut Command) -> Vec<u8> {
    let output = command.output().expect("capture provenance command");
    assert!(output.status.success(), "capture provenance failed");
    assert!(output.stdout.len() <= 1024 * 1024 && output.stderr.len() <= 65536);
    output.stdout
}

/// Bounded measured bytes with their size and SHA-256.
pub fn bytes(raw: &[u8]) -> Value {
    assert!(raw.len() <= 1024 * 1024);
    let mut value = digest(raw);
    value["base64"] = base64::engine::general_purpose::STANDARD.encode(raw).into();
    value
}

/// A failed `mcp` startup with its single listening port replaced by `<port>`.
pub fn startup_failure(case: &str, port: u16, exit_code: Option<i32>, stderr: &[u8]) -> Value {
    let stderr = std::str::from_utf8(stderr).expect("UTF-8 startup diagnostic");
    let marker = format!(":{port}:");
    assert_eq!(
        stderr.matches(&marker).count(),
        1,
        "port substitution must be unambiguous: {stderr}"
    );
    json!({"case": case, "exit_code": exit_code, "stderr": stderr.replace(&marker, ":<port>:")})
}

/// Current checkout newline policy; Windows generator bytes depend on it.
pub fn checkout_autocrlf() -> &'static str {
    let autocrlf = Command::new("git")
        .args(["config", "--get", "core.autocrlf"])
        .current_dir(ROOT)
        .output()
        .unwrap();
    assert!(matches!(autocrlf.status.code(), Some(0 | 1)));
    assert!(autocrlf.stdout.len() <= 32 && autocrlf.stderr.is_empty());
    match String::from_utf8(autocrlf.stdout).unwrap().trim() {
        "" | "false" => "false",
        "true" => "true",
        "input" => "input",
        _ => panic!("unrecorded checkout newline policy"),
    }
}

/// Current Go inputs and Rust input generators for one comparator file.
pub fn current_inputs(test_file: &str) -> (Map<String, Value>, Map<String, Value>) {
    let read = |name: &str| digest(&std::fs::read(Path::new(ROOT).join(name)).unwrap());
    let mut sources = Map::new();
    let tracked = checked(
        Command::new("git")
            .args(["ls-files", "-z", "--"])
            .args(SOURCE_ROOTS)
            .current_dir(ROOT),
    );
    for name in tracked
        .split(|byte| *byte == 0)
        .filter(|name| name.ends_with(b".go"))
    {
        let name = std::str::from_utf8(name).unwrap();
        sources.insert(name.to_owned(), read(name));
    }
    for name in ["go.mod", "go.sum"] {
        sources.insert(name.to_owned(), read(name));
    }
    let generators = [
        test_file,
        "crates/symeraseme-cli/tests/support/mcp_http_port.rs",
        "crates/symeraseme-cli/tests/support/capture_http_aux.rs",
        "Cargo.lock",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), read(name)))
    .collect();
    (sources, generators)
}

pub fn record(binary: &Path, test_file: &str, test: &str, observation: Value) {
    let Some(directory) =
        std::env::var_os("SYMERASEME_CAPTURE_HTTP_AUX").filter(|value| !value.is_empty())
    else {
        return;
    };
    assert!(observation.is_object());
    let directory = std::path::PathBuf::from(directory);
    assert!(
        directory.is_absolute(),
        "capture directory must be absolute"
    );
    std::fs::create_dir_all(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    assert!(!directory.starts_with(Path::new(ROOT).canonicalize().unwrap()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert!(
        checked(
            Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(ROOT)
        )
        .is_empty()
    );
    let revision = String::from_utf8(checked(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(ROOT),
    ))
    .unwrap();
    let revision = revision.trim();
    let info: Value = serde_json::from_slice(&checked(
        Command::new("go")
            .args([
                "env",
                "-json",
                "GOVERSION",
                "GOHOSTOS",
                "GOHOSTARCH",
                "GOOS",
                "GOARCH",
            ])
            .env("GOTOOLCHAIN", "go1.26.6")
            .env("GOENV", "off")
            .env("GOWORK", "off"),
    ))
    .unwrap();
    assert_eq!(info["GOVERSION"], "go1.26.6");
    assert_eq!(info["GOHOSTOS"], info["GOOS"]);
    assert_eq!(info["GOHOSTARCH"], info["GOARCH"]);
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => panic!("unrecorded native target"),
    };
    assert_eq!(info["GOHOSTOS"], os);
    assert_eq!(info["GOHOSTARCH"], arch);
    let build_info = String::from_utf8(checked(
        Command::new("go")
            .args(["version", "-m"])
            .arg(binary)
            .env("GOTOOLCHAIN", "go1.26.6"),
    ))
    .unwrap();
    for expected in [
        format!("vcs.revision={revision}"),
        "vcs.modified=false".to_owned(),
        format!("GOOS={os}"),
        format!("GOARCH={arch}"),
    ] {
        assert!(
            build_info.contains(&expected),
            "actual native Go binary provenance"
        );
    }
    let (sources, generators) = current_inputs(test_file);
    let encoded = serde_json::to_vec_pretty(&json!({
        "schema": "symeraseme.actual-go.http-aux.v1",
        "source_revision": revision, "source_files": sources,
        "archived_generators": generators, "checkout_autocrlf": checkout_autocrlf(),
        "native_target": format!("{os}/{arch}"), "go_version": "go1.26.6",
        "embedded_build_info": build_info, "test_file": test_file, "test": test,
        "observation": observation,
        "scope": "The Go side of one original auxiliary HTTP process comparator: exactly the statuses, port-normalized startup diagnostics, HTTP replies, process effects and regenerated cancellation oracle bytes that the original test compared against Rust. Timing, pids and ephemeral ports/tokens are not retained."
    }))
    .unwrap();
    assert!(encoded.len() <= 1024 * 1024);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(format!("{test}.json")))
        .unwrap();
    file.write_all(&encoded).unwrap();
    file.write_all(b"\n").unwrap();
}
