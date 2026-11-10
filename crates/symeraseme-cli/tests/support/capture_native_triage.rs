//! Actual native Go CLI/MCP triage streams, saved effects and invocation controls.
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

pub fn record(binary: &Path, family: &str, cases: &[serde_json::Value]) {
    let Some(directory) =
        std::env::var_os("SYMERASEME_CAPTURE_NATIVE_TRIAGE").filter(|value| !value.is_empty())
    else {
        return;
    };
    let count = match family {
        "cli" => 16,
        "mcp" => 8,
        _ => panic!("unknown triage family"),
    };
    assert_eq!(cases.len(), count);
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
    let helper = "rust-tests/parity/oracle/native-triage-agent/main.go";
    sources.insert(
        helper.to_owned(),
        digest(&std::fs::read(Path::new(ROOT).join(helper)).unwrap()),
    );
    assert_eq!(sources.len(), 172);
    let generators = [
        "crates/symeraseme-cli/tests/triage_native_process.rs",
        "crates/symeraseme-cli/tests/fixtures/native_triage_agent.rs",
        "crates/symeraseme-cli/tests/support/capture_native_triage.rs",
        "Cargo.lock",
        ".gitattributes",
        "tests/fixtures/cli-triage/cases.json",
    ]
    .into_iter()
    .map(|name| {
        (
            name.to_owned(),
            digest(&std::fs::read(Path::new(ROOT).join(name)).unwrap()),
        )
    })
    .collect::<serde_json::Map<_, _>>();
    let autocrlf = Command::new("git")
        .args(["config", "--get", "core.autocrlf"])
        .current_dir(ROOT)
        .output()
        .unwrap();
    assert!(matches!(autocrlf.status.code(), Some(0 | 1)));
    assert!(autocrlf.stdout.len() <= 32 && autocrlf.stderr.is_empty());
    let autocrlf = String::from_utf8(autocrlf.stdout).unwrap();
    let autocrlf = match autocrlf.trim() {
        "" | "false" => "false",
        "true" => "true",
        "input" => "input",
        _ => panic!("unrecorded checkout newline policy"),
    };
    let encoded = serde_json::to_vec_pretty(&json!({
        "schema": "symeraseme.actual-go.native-triage.v1",
        "source_revision": revision, "source_files": sources,
        "archived_generators": generators, "native_target": format!("{os}/{arch}"),
        "go_version": "go1.26.6", "embedded_build_info": build_info,
        "checkout_autocrlf": autocrlf,
        "family": family, "cases": cases,
        "scope": "Original actual native Go CLI/MCP triage: complete raw stdout/stderr/status, exact original input argv/environment or request frames, whole saved reply and ordered events, and actual invocation counts. The original provider-list-only ordering fold remains in the comparator; raw streams are retained without normalization. The Rust child is a native synthetic fixture, never a Go CLI or MCP oracle."
    }))
    .unwrap();
    assert!(encoded.len() <= 1024 * 1024);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(format!("{family}.json")))
        .unwrap();
    file.write_all(&encoded).unwrap();
    file.write_all(b"\n").unwrap();
}
