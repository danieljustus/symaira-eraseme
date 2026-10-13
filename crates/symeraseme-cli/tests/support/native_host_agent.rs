//! Builds the synthetic blocking agent from Rust source. It is not a Go oracle.
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn helper(root: &Path) -> PathBuf {
    let executable = root.join(if cfg!(windows) {
        "agent-helper.exe"
    } else {
        "agent-helper"
    });
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native_host_agent.rs");
    let log = root.join("agent-build.log");
    let output = std::fs::File::create(&log).unwrap();
    let mut child = Command::new("rustc")
        .args(["+1.98.0", "--edition=2024", "--crate-type=bin", "-o"])
        .arg(&executable)
        .arg(&source)
        .stdout(Stdio::null())
        .stderr(output)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait().unwrap();
            panic!("native agent helper build timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "native agent helper build: {}",
        std::fs::read_to_string(log).unwrap()
    );
    executable
}

pub fn observe(mut command: Command, root: &Path, label: &str) -> Output {
    let stdout = root.join(format!("{label}.stdout"));
    let stderr = root.join(format!("{label}.stderr"));
    let mut child = command
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&stdout).unwrap())
        .stderr(std::fs::File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait().unwrap();
            panic!("{label} native lookup exceeded its deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child.wait().unwrap();
    for path in [&stdout, &stderr] {
        assert!(
            std::fs::metadata(path).unwrap().len() <= 64 * 1024,
            "bounded lookup output"
        );
    }
    Output {
        status,
        stdout: std::fs::read(stdout).unwrap(),
        stderr: std::fs::read(stderr).unwrap(),
    }
}
