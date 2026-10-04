//! Opt-in measured native Go HTTP observations from the original ten-case comparator.
use base64::Engine;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn digest(bytes: &[u8]) -> serde_json::Value {
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

pub fn record(binary: &Path, exit_status: i32, cases: &[serde_json::Value]) {
    let Some(directory) =
        std::env::var_os("SYMERASEME_CAPTURE_HTTP_WIRE").filter(|value| !value.is_empty())
    else {
        return;
    };
    assert_eq!(exit_status, 0);
    assert_eq!(cases.len(), 10);
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
    let info: serde_json::Value = serde_json::from_slice(&checked(
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
    let mut sources = serde_json::Map::new();
    let tracked = checked(
        Command::new("git")
            .args(["ls-files", "-z", "cmd", "internal"])
            .current_dir(ROOT),
    );
    for name in tracked
        .split(|byte| *byte == 0)
        .filter(|name| name.ends_with(b".go"))
    {
        let name = std::str::from_utf8(name).unwrap();
        sources.insert(
            name.to_owned(),
            digest(&std::fs::read(Path::new(ROOT).join(name)).unwrap()),
        );
    }
    for name in ["go.mod", "go.sum"] {
        sources.insert(
            name.to_owned(),
            digest(&std::fs::read(Path::new(ROOT).join(name)).unwrap()),
        );
    }
    assert_eq!(sources.len(), 171);
    let generators = [
        "crates/symeraseme-cli/tests/mcp_http_process.rs",
        "crates/symeraseme-cli/tests/support/capture_http_wire.rs",
        "Cargo.lock",
    ]
    .into_iter()
    .map(|name| {
        (
            name.to_owned(),
            digest(&std::fs::read(Path::new(ROOT).join(name)).unwrap()),
        )
    })
    .collect::<serde_json::Map<_, _>>();
    let encoded = serde_json::to_vec_pretty(&json!({
        "schema": "symeraseme.actual-go.http-wire.v1",
        "source_revision": revision, "source_files": sources,
        "archived_generators": generators, "native_target": format!("{os}/{arch}"),
        "go_version": "go1.26.6", "embedded_build_info": build_info,
        "server_exit_status": exit_status, "cases": cases,
        "scope": "All ten original HTTP process comparisons: actual status, Content-Type and complete body bytes. Ephemeral native authorization tokens are represented as input roles, exactly as the original Go/Rust request-token substitution. Other response headers and process stdout/stderr are not part of that original comparator."
    }))
    .unwrap();
    assert!(encoded.len() <= 1024 * 1024);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("wire.json"))
        .unwrap();
    file.write_all(&encoded).unwrap();
    file.write_all(b"\n").unwrap();
}

pub fn request(method: &str, body: &[u8], headers: &[(&str, String)]) -> serde_json::Value {
    json!({
        "method": method,
        "body_base64": base64::engine::general_purpose::STANDARD.encode(body),
        "body": digest(body),
        "headers": headers.iter().map(|(name, value)| {
            if name.eq_ignore_ascii_case("authorization") {
                assert!(value.starts_with("Bearer "));
                json!({"name": name, "role": "private-native-bearer-token"})
            } else { json!({"name": name, "value": value}) }
        }).collect::<Vec<_>>()
    })
}

pub fn case(
    index: usize,
    request: serde_json::Value,
    reply: &(u16, String, Vec<u8>),
) -> serde_json::Value {
    assert!(reply.2.len() <= 1024 * 1024);
    json!({"index": index, "request": request, "response": {
        "status": reply.0, "content_type": reply.1,
        "body_base64": base64::engine::general_purpose::STANDARD.encode(&reply.2),
        "body": digest(&reply.2)
    }})
}
