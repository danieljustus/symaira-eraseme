use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "symeraseme-backend-fallback-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated test directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn build_go_cli(directory: &Path) -> PathBuf {
    let binary = directory.join(if cfg!(windows) {
        "symeraseme-go.exe"
    } else {
        "symeraseme-go"
    });
    let output = Command::new("go")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .env("GOWORK", "off")
        .env("GOENV", "off")
        .env("GOPROXY", "off")
        .env("GOTOOLCHAIN", "go1.26.6")
        .args(["build", "-o"])
        .arg(&binary)
        .arg("./cmd/symeraseme")
        .output()
        .expect("build source-bound Go CLI");
    assert!(
        output.status.success(),
        "Go CLI build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn run(binary: &Path, cwd: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .env("HOME", cwd.join("home"))
        .env("SYMERASEME_DATA_DIR", cwd.join("data"))
        .env_remove("SYMERASEME_BACKEND")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start CLI process");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(stdin)
        .expect("write child stdin");
    child.wait_with_output().expect("wait for CLI process")
}

fn run_go_backend(binary: &Path, cwd: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .env("HOME", cwd.join("home"))
        .env("SYMERASEME_DATA_DIR", cwd.join("data"))
        .env("SYMERASEME_BACKEND", "go")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Rust fallback");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(stdin)
        .expect("write fallback stdin");
    child.wait_with_output().expect("wait for fallback process")
}

#[test]
fn explicit_go_backend_matches_live_go_process_and_requires_sibling() {
    let scratch = Scratch::new();
    let go = build_go_cli(&scratch.0);
    let project = scratch.0.join("project");
    fs::create_dir(&project).expect("create project working directory");
    fs::write(project.join(".symeraseme.toml"), "port = 8123\n").expect("write project config");
    let rust = scratch.0.join(if cfg!(windows) {
        "symeraseme-rust.exe"
    } else {
        "symeraseme-rust"
    });
    fs::copy(env!("CARGO_BIN_EXE_symeraseme-rust"), &rust).expect("stage Rust shadow binary");

    for (args, stdin) in [
        (vec!["version", "--json"], b"".as_slice()),
        (vec!["mcp", "--help"], b"".as_slice()),
        (vec!["config", "show", "--output", "json"], b"".as_slice()),
        (vec!["no-such-command"], b"".as_slice()),
    ] {
        let expected = run(&go, &project, &args, stdin);
        let actual = run_go_backend(&rust, &project, &args, stdin);
        assert_eq!(actual.status, expected.status, "status for {args:?}");
        assert_eq!(actual.stdout, expected.stdout, "stdout for {args:?}");
        assert_eq!(actual.stderr, expected.stderr, "stderr for {args:?}");
    }

    let missing_directory = scratch.0.join("without-fallback");
    fs::create_dir(&missing_directory).expect("create no-fallback directory");
    let missing_rust = missing_directory.join(rust.file_name().expect("Rust binary name"));
    fs::copy(&rust, &missing_rust).expect("stage Rust binary without Go sibling");
    let missing = Command::new(missing_rust)
        .env("SYMERASEME_BACKEND", "go")
        .arg("version")
        .output()
        .expect("run missing-fallback case");
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Go fallback is missing"));

    let invalid = Command::new(&rust)
        .env("SYMERASEME_BACKEND", "unknown")
        .arg("version")
        .output()
        .expect("run invalid-backend case");
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("unsupported SYMERASEME_BACKEND"));
}
