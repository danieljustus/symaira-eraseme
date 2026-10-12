//! Native negative controls for the owned Windows AppContainer CLI boundary.
//!
//! Test-only port of rust-tests/parity/oracle/store-sandbox-control (Go). It is
//! never shipped with the product. Connection refusal or timeouts do not count
//! as denial; only Windows access-denied (5), WSAEACCES (10013) and blocked
//! child creation (367) do.
#![forbid(unsafe_code)]

use serde_json::json;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::process::{Command, Stdio, exit};
use std::thread;
use std::time::Duration;

const ERROR_ACCESS_DENIED: i32 = 5;
const WSAEACCES: i32 = 10013;
const ERROR_CHILD_PROCESS_BLOCKED: i32 = 367;

fn error_code(error: Option<&io::Error>) -> i64 {
    error.and_then(io::Error::raw_os_error).map_or(0, i64::from)
}

fn is_denied(error: Option<&io::Error>) -> bool {
    error.is_some_and(|e| matches!(e.raw_os_error(), Some(ERROR_ACCESS_DENIED | WSAEACCES)))
}

#[test]
fn only_kernel_access_denials_count_as_network_confinement() {
    for code in [ERROR_ACCESS_DENIED, WSAEACCES] {
        assert!(is_denied(Some(&io::Error::from_raw_os_error(code))));
    }
    for code in [0, ERROR_CHILD_PROCESS_BLOCKED, 10060, 10061] {
        assert!(!is_denied(Some(&io::Error::from_raw_os_error(code))));
    }
    assert!(!is_denied(None));
    assert!(!is_denied(Some(&io::Error::new(
        ErrorKind::PermissionDenied,
        "not a native error code",
    ))));
}

fn denied_permission<T>(result: io::Result<T>) -> bool {
    matches!(result, Err(error) if error.kind() == ErrorKind::PermissionDenied)
}

fn tcp_error() -> Option<io::Error> {
    let address = SocketAddr::from(([127, 0, 0, 1], 9));
    TcpStream::connect_timeout(&address, Duration::from_secs(1)).err()
}

fn udp_error() -> Option<io::Error> {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => socket,
        Err(error) => return Some(error),
    };
    if let Err(error) = socket.connect("198.51.100.1:9") {
        return Some(error);
    }
    socket.send(b"synthetic-network-negative-control").err()
}

fn child_error() -> Option<io::Error> {
    let executable = match env::current_exe() {
        Ok(path) => path,
        Err(error) => return Some(error),
    };
    Command::new(executable)
        .arg("failure")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .err()
}

fn go_os() -> &'static str {
    match env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

fn go_arch() -> &'static str {
    match env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn run_controls(outside_read: &str, outside_write: &str) -> bool {
    let mut checks: BTreeMap<&str, bool> = BTreeMap::new();
    let mut codes: BTreeMap<&str, i64> = BTreeMap::new();

    checks.insert(
        "outside_read_denied",
        denied_permission(fs::read(outside_read)),
    );
    checks.insert(
        "outside_write_denied",
        denied_permission(fs::write(outside_write, b"must not escape")),
    );

    let home = env::var("HOME").unwrap_or_default();
    let inside = format!("{home}{}inside-marker", std::path::MAIN_SEPARATOR);
    let owned_write = fs::write(&inside, b"owned write");
    checks.insert("owned_write_allowed", owned_write.is_ok());
    if owned_write.is_ok() {
        checks.insert("owned_remove_allowed", fs::remove_file(&inside).is_ok());
    }

    let tcp = tcp_error();
    checks.insert("tcp_denied", is_denied(tcp.as_ref()));
    codes.insert("tcp", error_code(tcp.as_ref()));

    let udp = udp_error();
    checks.insert("udp_denied", is_denied(udp.as_ref()));
    codes.insert("udp", error_code(udp.as_ref()));

    let child = child_error();
    checks.insert(
        "child_creation_denied",
        is_denied(child.as_ref())
            || error_code(child.as_ref()) == i64::from(ERROR_CHILD_PROCESS_BLOCKED),
    );
    codes.insert("child_creation", error_code(child.as_ref()));

    let passed = checks.values().all(|value| *value);
    let report = json!({
        "checks": checks,
        "native_error_codes": codes,
        "passed": passed,
        "os": go_os(),
        "arch": go_arch(),
    });
    println!("{report}");
    passed
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [_, mode] if mode == "stall" => loop {
            thread::sleep(Duration::from_secs(1));
        },
        [_, mode] if mode == "failure" => exit(23),
        [_, outside_read, outside_write] => {
            if !run_controls(outside_read, outside_write) {
                eprintln!("native sandbox negative control failed");
                exit(1);
            }
        }
        _ => {
            eprintln!("expected outside read and write markers");
            exit(2);
        }
    }
}
