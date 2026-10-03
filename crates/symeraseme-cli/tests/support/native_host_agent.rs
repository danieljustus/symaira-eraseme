//! Build the actual checked-out Go helper; its observations are never simulated.
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn helper(root: &Path) -> PathBuf {
    let executable = root.join(if cfg!(windows) {
        "agent-helper.exe"
    } else {
        "agent-helper"
    });
    let log = root.join("agent-build.log");
    let output = std::fs::File::create(&log).unwrap();
    let mut child = Command::new("go")
        .args(["build", "-o"])
        .arg(&executable)
        .arg("./rust-tests/parity/oracle/native-host-agent")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .stdout(Stdio::null())
        .stderr(output)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait().unwrap();
            panic!("native agent Go helper build timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    child.wait().unwrap();
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
