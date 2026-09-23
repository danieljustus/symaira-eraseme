//! Exercise token-authenticated MCP over the real local process/network path.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "symeraseme-mcp-http-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn start(root: &Path, port: u16, host: &str, allow_remote: bool) -> Child {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
    command.args(["mcp", "--host", host, "--port", &port.to_string()]);
    if allow_remote {
        command.arg("--allow-remote");
    }
    command
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("SYMERASEME_DATA_DIR", root.join("data"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn wait_ready(child: &mut Child, port: u16) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let stderr = std::io::read_to_string(child.stderr.take().unwrap()).unwrap();
            panic!("MCP server exited early ({status}): {stderr}");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "MCP HTTP server did not start"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn token(root: &Path) -> String {
    let path = root.join("data/mcp_token");
    let value = std::fs::read_to_string(&path).unwrap();
    assert_eq!(value.len(), 43);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(root.join("data"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    value
}

fn exchange(
    port: u16,
    method: &str,
    body: &[u8],
    headers: &[(&str, &str)],
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(stream, "{method} / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len()).unwrap();
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").unwrap();
    }
    stream.write_all(b"\r\n").unwrap();
    stream.write_all(body).unwrap();
    read_response(&mut stream)
}

fn oversized_declared_request(port: u16, token: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(stream, "POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n", 5 * 1024 * 1024 + 1).unwrap();
    read_response(&mut stream)
}

fn chunked_oversized_request(port: u16, token: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    let token = token.to_owned();
    let sender = thread::spawn(move || {
        let body = vec![b'x'; 5 * 1024 * 1024 + 1];
        let _ = write!(
            writer,
            "POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
            body.len()
        );
        let _ = writer.write_all(&body);
        let _ = writer.write_all(b"\r\n0\r\n\r\n");
    });
    let reply = read_response(&mut stream);
    let _ = sender.join();
    reply
}

fn read_response(stream: &mut TcpStream) -> (u16, String, Vec<u8>) {
    let mut response = Vec::new();
    let mut chunk = [0; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).unwrap();
        assert_ne!(count, 0, "HTTP response ended before headers");
        response.extend_from_slice(&chunk[..count]);
        if let Some(index) = response.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header = std::str::from_utf8(&response[..header_end]).unwrap();
    let status = header
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let content_type = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-type")
                .then(|| value.trim().to_owned())
        })
        .unwrap_or_default();
    let content_length = header.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().unwrap())
    });
    let mut body = response[header_end..].to_vec();
    while content_length.is_some_and(|length| body.len() < length) {
        let count = stream.read(&mut chunk).unwrap();
        assert_ne!(count, 0, "HTTP response body ended early");
        body.extend_from_slice(&chunk[..count]);
    }
    if let Some(length) = content_length {
        body.truncate(length);
    } else if status != 204 {
        stream.read_to_end(&mut body).unwrap();
    } else {
        body.clear();
    }
    (status, content_type, body)
}

fn stop_with_args(args: &[&str], root: &Path) -> (std::process::ExitStatus, String) {
    let home = root.join("failed-home");
    std::fs::create_dir_all(&home).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(args)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("SYMERASEME_DATA_DIR", root.join("failed-data"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let status = child.wait().unwrap();
    let stderr = std::io::read_to_string(child.stderr.take().unwrap()).unwrap();
    (status, stderr)
}

#[cfg(unix)]
fn signal(child: &mut Child, name: &str) {
    assert!(
        Command::new("kill")
            .args([format!("-{name}"), child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(6);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "server exited with {status}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "server did not shut down after {name}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[cfg(unix)]
fn http_process_matches_core_contract_rotates_token_and_shuts_down_on_signals() {
    let root = TestDir::new();
    let port = free_port();
    let mut child = start(root.path(), port, "127.0.0.1", false);
    wait_ready(&mut child, port);
    let first_token = token(root.path());

    let (status, content_type, body) = exchange(port, "GET", b"", &[]);
    assert_eq!(status, 405);
    assert_eq!(content_type, "application/json");
    assert_eq!(
        body,
        br#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"POST required"},"id":null}
"#
    );

    let (status, _, body) = exchange(port, "POST", br#"{}"#, &[]);
    assert_eq!(status, 401);
    assert_eq!(
        body,
        br#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Unauthorized"},"id":null}
"#
    );

    let (status, _, body) = exchange(
        port,
        "POST",
        br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        &[
            ("Authorization", &format!("Bearer {first_token}")),
            ("Origin", "https://evil.example"),
        ],
    );
    assert_eq!(status, 403);
    assert_eq!(body, br#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Forbidden: disallowed Origin"},"id":null}
"#);

    let (status, _, _body) = exchange(
        port,
        "POST",
        br#"{}"#,
        &[
            ("Authorization", &format!("Bearer {first_token}")),
            ("Authorization", &format!("Bearer {first_token}")),
        ],
    );
    assert_eq!(status, 401);

    let (status, content_type, body) = exchange(
        port,
        "POST",
        br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        &[
            ("Authorization", &format!("Bearer {first_token}")),
            ("Origin", "http://[::1]:8000"),
        ],
    );
    assert_eq!(status, 200);
    assert_eq!(content_type, "application/json");
    assert_eq!(body, br#"{"jsonrpc":"2.0","result":{"capabilities":{"tools":{}},"protocolVersion":"2025-06-18","serverInfo":{"name":"symeraseme","version":"dev"}},"id":1}
"#);

    let (status, content_type, body) = exchange(
        port,
        "POST",
        br#"{"jsonrpc":"2.0","method":"initialize"}"#,
        &[("Authorization", &format!("Bearer {first_token}"))],
    );
    assert_eq!(status, 204);
    assert_eq!(content_type, "");
    assert!(body.is_empty());

    let (status, content_type, body) = oversized_declared_request(port, &first_token);
    assert_eq!(status, 413);
    assert_eq!(content_type, "application/json");
    assert_eq!(
        body,
        br#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid Request"},"id":null}
"#
    );

    let (status, content_type, body) = chunked_oversized_request(port, &first_token);
    assert_eq!(status, 200);
    assert_eq!(content_type, "application/json");
    assert_eq!(
        body,
        br#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"parse error"},"id":null}
"#
    );

    for signal_name in ["INT", "TERM"] {
        signal(&mut child, signal_name);
        if signal_name == "INT" {
            child = start(root.path(), port, "127.0.0.1", false);
            wait_ready(&mut child, port);
            let second_token = token(root.path());
            assert_ne!(
                first_token, second_token,
                "MCP token was not rotated on restart"
            );
        }
    }

    let (status, stderr) = stop_with_args(&["mcp", "--host", "0.0.0.0"], root.path());
    assert!(!status.success());
    assert!(stderr.contains("refusing non-loopback MCP bind \"0.0.0.0\" without --allow-remote"));

    let remote_port = free_port();
    let mut remote = start(root.path(), remote_port, "0.0.0.0", true);
    wait_ready(&mut remote, remote_port);
    signal(&mut remote, "TERM");
}
