//! Native Windows Go/Rust MCP HTTP process contract.
#![cfg(windows)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start(binary: &Path, root: &Path, port: u16) -> Server {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    Server(
        Command::new(binary)
            .args(["mcp", "--host", "127.0.0.1", "--port", &port.to_string()])
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("SYMERASEME_DATA_DIR", root.join("data"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

fn ready(server: &mut Server, port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "MCP server exited early"
        );
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "MCP server did not listen");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn exchange(port: u16, token: Option<&str>, origin: Option<&str>) -> (u16, Vec<u8>) {
    let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "POST / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )
    .unwrap();
    if let Some(token) = token {
        write!(stream, "Authorization: Bearer {token}\r\n").unwrap();
    }
    if let Some(origin) = origin {
        write!(stream, "Origin: {origin}\r\n").unwrap();
    }
    stream.write_all(b"\r\n").unwrap();
    stream.write_all(body).unwrap();
    let mut response = Vec::new();
    let read_error = stream.read_to_end(&mut response).err();
    let split = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap();
    let head = std::str::from_utf8(&response[..split]).unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = response[split + 4..].to_vec();
    if let Some(error) = read_error {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        // Windows can report an RST after an early auth/origin rejection.
        // Accept it only when the complete HTTP response arrived first.
        let length = head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .expect("reset response must declare Content-Length");
        assert_eq!(body.len(), length, "reset truncated the HTTP response");
    }
    (status, body)
}

#[test]
fn native_windows_http_matches_checked_out_go() {
    let root = tempfile::tempdir().unwrap();
    let oracle = root.path().join("symeraseme-go-oracle.exe");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let build = Command::new("go")
        .args(["build", "-o"])
        .arg(&oracle)
        .arg("./cmd/symeraseme")
        .current_dir(repo)
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "Go oracle build: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let go_root = root.path().join("go");
    let rust_root = root.path().join("rust");
    let go_port = port();
    let mut rust_port = port();
    while rust_port == go_port {
        rust_port = port();
    }
    let mut go = start(&oracle, &go_root, go_port);
    let mut rust = start(
        Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
        &rust_root,
        rust_port,
    );
    ready(&mut go, go_port);
    ready(&mut rust, rust_port);
    let go_token = std::fs::read_to_string(go_root.join("data/mcp_token")).unwrap();
    let rust_token = std::fs::read_to_string(rust_root.join("data/mcp_token")).unwrap();
    assert_eq!(go_token.len(), 43);
    assert_eq!(rust_token.len(), 43);

    for (authorized, origin) in [
        (false, None),
        (true, None),
        (true, Some("https://evil.example")),
        (true, Some("")),
    ] {
        let go_reply = exchange(go_port, authorized.then_some(go_token.as_str()), origin);
        let rust_reply = exchange(rust_port, authorized.then_some(rust_token.as_str()), origin);
        assert_eq!(
            rust_reply, go_reply,
            "HTTP parity: authorized={authorized}, origin={origin:?}"
        );
    }

    drop(rust);
    let mut restarted = start(
        Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
        &rust_root,
        rust_port,
    );
    ready(&mut restarted, rust_port);
    assert_ne!(
        std::fs::read_to_string(rust_root.join("data/mcp_token")).unwrap(),
        rust_token
    );

    let mut refused = Server(
        Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
            .args(["mcp", "--host", "0.0.0.0", "--port", &port().to_string()])
            .env("HOME", rust_root.join("home"))
            .env("USERPROFILE", rust_root.join("home"))
            .env("SYMERASEME_DATA_DIR", rust_root.join("data"))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = refused.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "non-loopback bind did not fail closed"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success());
    let mut stderr = String::new();
    refused
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(stderr.contains("refusing non-loopback MCP bind"));
}
