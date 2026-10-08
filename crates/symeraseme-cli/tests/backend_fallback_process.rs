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
            "symeraseme-rust-only-{}-{nonce}",
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

fn run(binary: &Path, cwd: &Path, args: &[&str], stdin: &[u8], backend: Option<&str>) -> Output {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(cwd)
        .env("HOME", cwd.join("home"))
        .env("USERPROFILE", cwd.join("home"))
        .env("XDG_CONFIG_HOME", cwd.join("config"))
        .env("XDG_DATA_HOME", cwd.join("data"))
        .env("SYMERASEME_DATA_DIR", cwd.join("data"))
        .env_remove("SYMERASEME_RESOURCES")
        .env_remove("SYMERASEME_BACKEND")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(value) = backend {
        command.env("SYMERASEME_BACKEND", value);
    }
    let mut child = command.spawn().expect("start Rust CLI process");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(stdin)
        .expect("write child stdin");
    child.wait_with_output().expect("wait for Rust CLI process")
}

#[test]
fn retired_backend_override_never_dispatches_a_go_sibling() {
    let scratch = Scratch::new();
    let project = scratch.0.join("project");
    fs::create_dir(&project).expect("create project working directory");
    fs::write(project.join(".symeraseme.toml"), "port = 8123\n").expect("write project config");
    let rust = scratch.0.join(if cfg!(windows) {
        "symeraseme.exe"
    } else {
        "symeraseme"
    });
    fs::copy(env!("CARGO_BIN_EXE_symeraseme-rust"), &rust).expect("stage Rust binary");
    let sibling = scratch.0.join(if cfg!(windows) {
        "symeraseme-go.exe"
    } else {
        "symeraseme-go"
    });
    let request = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"rust-only-test","version":"1"}}}"#;

    for present in [false, true] {
        if present {
            fs::write(&sibling, b"This retired sibling must never be executed")
                .expect("stage invalid legacy sibling");
        }
        for (args, stdin) in [
            (vec!["version", "--json"], b"".as_slice()),
            (vec!["mcp", "--help"], b"".as_slice()),
            (vec!["config", "show", "--output", "json"], b"".as_slice()),
            (vec!["no-such-command"], b"".as_slice()),
            (vec!["mcp", "--stdio"], request.as_slice()),
        ] {
            let expected = run(&rust, &project, &args, stdin, None);
            for backend in ["go", "rust", "unknown"] {
                let actual = run(&rust, &project, &args, stdin, Some(backend));
                assert_eq!(
                    actual.status, expected.status,
                    "status for {args:?}/{backend}"
                );
                assert_eq!(
                    actual.stdout, expected.stdout,
                    "stdout for {args:?}/{backend}"
                );
                assert_eq!(
                    actual.stderr, expected.stderr,
                    "stderr for {args:?}/{backend}"
                );
            }
        }
    }
}
