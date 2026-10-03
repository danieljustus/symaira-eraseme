//! Native Windows files, DACLs and atomic replacement compared with real Go.
#![cfg(windows)]

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use symeraseme_core::identity::{ConsentError, ConsentStore};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn capture(mut command: Command, root: &Path, label: &str, budget: Duration) -> Output {
    let stdout = root.join(format!("{label}.stdout"));
    let stderr = root.join(format!("{label}.stderr"));
    let mut child = command
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).unwrap())
        .stderr(fs::File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + budget;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait().unwrap();
            let diagnostic = if fs::metadata(&stderr).unwrap().len() <= 4096 {
                fs::read_to_string(&stderr).unwrap_or_default()
            } else {
                "diagnostic exceeds limit".into()
            };
            panic!("{label} exceeded its native deadline: {diagnostic}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    child.wait().unwrap();
    for path in [&stdout, &stderr] {
        assert!(fs::metadata(path).unwrap().len() <= 64 * 1024);
    }
    Output {
        status,
        stdout: fs::read(stdout).unwrap(),
        stderr: fs::read(stderr).unwrap(),
    }
}

fn private(command: &mut Command, root: &Path) {
    let home = root.join("home");
    let temp = root.join("tmp");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&temp).unwrap();
    command
        .env_clear()
        .current_dir(root)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("TEMP", &temp)
        .env("TMP", &temp);
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn acl(path: &Path, mode: &str, logs: &Path, label: &str) -> Value {
    let executable = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut command = Command::new(executable);
    private(&mut command, logs);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(repo().join("crates/symeraseme-core/tests/support/consent_acl.ps1"))
        .arg("-Path")
        .arg(path)
        .arg("-Mode")
        .arg(mode);
    let result = capture(command, logs, label, Duration::from_secs(30));
    assert!(
        result.status.success(),
        "ACL observation: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn native_consent_files_and_dacls_match_checked_out_go() {
    let root = tempfile::tempdir().unwrap();
    let helper = root.path().join("consent-go.exe");
    let mut build = Command::new("go");
    build
        .args(["build", "-o"])
        .arg(&helper)
        .arg("./rust-tests/parity/oracle/consent-windows-acl")
        .current_dir(repo())
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off");
    let result = capture(build, root.path(), "go-build", Duration::from_secs(180));
    assert!(
        result.status.success(),
        "Go consent build: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    for (case, protect, readonly_dir, readonly_token) in [
        ("inherited", false, false, false),
        ("owner-only-parent", true, false, false),
        ("readonly-directory", true, true, false),
        ("readonly-token", true, false, true),
    ] {
        let parent = root.path().join(case);
        fs::create_dir(&parent).unwrap();
        if protect {
            assert_eq!(
                acl(&parent, "protect", root.path(), case)["protected"],
                true
            );
        }
        let go_dir = parent.join("go");
        let rust_dir = parent.join("rust");
        for directory in [&go_dir, &rust_dir] {
            fs::create_dir(directory).unwrap();
            fs::write(directory.join("sentinel"), "unrelated sentinel").unwrap();
            if readonly_dir {
                let mut p = fs::metadata(directory).unwrap().permissions();
                p.set_readonly(true);
                fs::set_permissions(directory, p).unwrap();
            }
        }
        let mut command = Command::new(&helper);
        private(&mut command, root.path());
        command
            .arg(&go_dir)
            .arg(if readonly_token {
                "readonly-token"
            } else {
                "fresh"
            })
            .env("SYMERASEME_ORACLE_SOURCE_ROOT", repo());
        let result = capture(
            command,
            root.path(),
            &format!("go-{case}"),
            Duration::from_secs(30),
        );
        assert!(
            result.status.success(),
            "Go consent observation: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let observation: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(
            observation["schema"],
            "symeraseme.go-oracle.consent-windows-acl.v1"
        );
        assert_eq!(observation["go_version"], "go1.26.6");
        assert!(
            observation["platform"]
                .as_str()
                .unwrap()
                .starts_with("windows/")
        );
        let sources = observation["sources_sha256"].as_object().unwrap();
        assert_eq!(sources.len(), 4);
        for source in [
            "go.mod",
            "internal/identity/consent.go",
            "internal/identity/gate.go",
            "rust-tests/parity/oracle/consent-windows-acl/main.go",
        ] {
            assert_eq!(
                hex::encode(Sha256::digest(fs::read(repo().join(source)).unwrap())),
                sources[source].as_str().unwrap()
            );
        }
        let store = ConsentStore::new(&rust_dir)
            .with_clock(|| 1000)
            .with_random_source(|length| Ok(vec![7; length]));
        store.issue_token("before", 60).unwrap();
        let mut failed = false;
        if readonly_token {
            for entry in fs::read_dir(&rust_dir).unwrap() {
                let path = entry.unwrap().path();
                if path.file_name().unwrap() == "sentinel" {
                    continue;
                }
                let mut p = fs::metadata(&path).unwrap().permissions();
                p.set_readonly(true);
                fs::set_permissions(path, p).unwrap();
            }
            let error = store.issue_token("after", 60).unwrap_err();
            assert!(
                matches!(error, ConsentError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied)
            );
            failed = true;
        }
        assert_eq!(observation["failed"], failed);
        assert_eq!(
            observation["error_class"],
            if failed { "permission" } else { "ok" }
        );
        assert_eq!(
            acl(&rust_dir, "read", root.path(), &format!("rust-dir-{case}")),
            acl(&go_dir, "read", root.path(), &format!("go-dir-{case}"))
        );
        let mut names = fs::read_dir(&rust_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(serde_json::to_value(&names).unwrap(), observation["files"]);
        for name in names {
            let go = go_dir.join(&name);
            let rust = rust_dir.join(&name);
            assert!(
                fs::read(&go).unwrap() == fs::read(&rust).unwrap(),
                "{case}/{name}: native bytes differ"
            );
            assert_eq!(
                acl(
                    &rust,
                    "read",
                    root.path(),
                    &format!("rust-file-{case}-{name}")
                ),
                acl(&go, "read", root.path(), &format!("go-file-{case}-{name}"))
            );
            // Undo only our own read-only control before checked cleanup.
            for path in [go, rust] {
                #[allow(clippy::permissions_set_readonly_false)]
                {
                    let mut p = fs::metadata(&path).unwrap().permissions();
                    p.set_readonly(false);
                    fs::set_permissions(path, p).unwrap();
                }
            }
        }
        eprintln!(
            "native consent {case}: real Go bytes, owner/group/DACL, read-only state, full owned tree and sentinel matched"
        );
    }
    root.close().unwrap();
}
