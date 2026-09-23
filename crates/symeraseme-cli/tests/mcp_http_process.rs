//! Exercise token-authenticated MCP over the real local process/network path.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct TestDir(PathBuf);
type OracleCase<'a> = (&'a str, &'a [u8], Vec<(&'a str, String)>);

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
    static ALLOCATED: OnceLock<Mutex<HashSet<u16>>> = OnceLock::new();
    let allocated = ALLOCATED.get_or_init(|| Mutex::new(HashSet::new()));
    loop {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        if allocated.lock().unwrap().insert(port) {
            return port;
        }
    }
}

fn start(root: &Path, port: u16, host: &str, allow_remote: bool) -> Child {
    start_binary(
        Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
        root,
        port,
        host,
        allow_remote,
    )
}

fn start_binary(binary: &Path, root: &Path, port: u16, host: &str, allow_remote: bool) -> Child {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut command = Command::new(binary);
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

fn startup_error(
    binary: &Path,
    root: &Path,
    port: u16,
    host: &str,
) -> (std::process::ExitStatus, String) {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut child = Command::new(binary)
        .args(["mcp", "--host", host, "--port", &port.to_string()])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("SYMERASEME_DATA_DIR", root.join("data"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let status = child.wait().unwrap();
    let stderr = std::io::read_to_string(child.stderr.take().unwrap()).unwrap();
    (status, stderr)
}

fn build_go_oracle(root: &Path) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let oracle = root.join("symeraseme-go-oracle");
    let build = Command::new("go")
        .args(["build", "-o"])
        .arg(&oracle)
        .arg("./cmd/symeraseme")
        .current_dir(repo)
        .output()
        .expect("Go toolchain is required to reproduce the HTTP oracle transcript");
    assert!(
        build.status.success(),
        "Go oracle build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    oracle
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

fn exchange_obs_text_origin(port: u16, token: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut request = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nOrigin: http://local"
    )
    .into_bytes();
    request.extend_from_slice(&[0xff]);
    request.extend_from_slice(b"host\r\nContent-Length: 2\r\n\r\n{}");
    stream.write_all(&request).unwrap();
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
    let _ = sender.join();
    read_response(&mut stream)
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
    send_signal(child, name);
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

#[cfg(unix)]
fn send_signal(child: &mut Child, name: &str) {
    assert!(
        Command::new("kill")
            .args([format!("-{name}"), child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
#[cfg(unix)]
fn http_process_matches_core_contract_rotates_token_and_shuts_down_on_signals() {
    let root = TestDir::new();
    std::fs::create_dir_all(root.path().join("data")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.path().join("data"),
            std::fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        std::fs::write(root.path().join("data/mcp_token"), "old-token").unwrap();
        std::fs::set_permissions(
            root.path().join("data/mcp_token"),
            std::fs::Permissions::from_mode(0o666),
        )
        .unwrap();
    }
    let port = free_port();
    let mut child = start(root.path(), port, "127.0.0.1", false);
    wait_ready(&mut child, port);
    let first_token = token(root.path());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.path().join("data"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o777,
            "Go MkdirAll semantics preserve permissions on an existing directory"
        );
    }

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

#[test]
#[cfg(unix)]
fn signal_stops_accepting_before_in_flight_request_drains() {
    let root = TestDir::new();
    let port = free_port();
    let mut child = start(root.path(), port, "127.0.0.1", false);
    wait_ready(&mut child, port);
    let bearer = token(root.path());

    let mut in_flight = TcpStream::connect(("127.0.0.1", port)).unwrap();
    in_flight
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(in_flight, "POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {bearer}\r\nContent-Length: 10\r\nConnection: close\r\n\r\n{{}}").unwrap();
    thread::sleep(Duration::from_millis(100));
    send_signal(&mut child, "TERM");
    thread::sleep(Duration::from_millis(150));

    let mut later = match TcpStream::connect(("127.0.0.1", port)) {
        Ok(stream) => stream,
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            in_flight.write_all(b"12345678").unwrap();
            let (status, _, _) = read_response(&mut in_flight);
            assert_eq!(status, 200);
            let status = child.wait().unwrap();
            assert!(
                status.success(),
                "server failed while draining in-flight request"
            );
            return;
        }
        Err(error) => panic!("new connection during shutdown failed unexpectedly: {error}"),
    };
    later
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    later
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = [0; 1];
    match later.read(&mut response) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::ConnectionReset
            ) => {}
        Ok(count) => panic!(
            "server answered a connection after shutdown began: {:?}",
            &response[..count]
        ),
        Err(error) => panic!("unexpected read result after shutdown: {error}"),
    }

    in_flight.write_all(b"12345678").unwrap();
    let (status, _, _) = read_response(&mut in_flight);
    assert_eq!(status, 200);
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "server failed while draining in-flight request"
    );
}

#[test]
#[cfg(unix)]
fn incomplete_request_headers_expire_at_the_go_five_second_timeout() {
    let root = TestDir::new();
    let port = free_port();
    let mut child = start(root.path(), port, "127.0.0.1", false);
    wait_ready(&mut child, port);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(7)))
        .unwrap();
    stream
        .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer partial")
        .unwrap();
    let started = Instant::now();
    let mut response = Vec::new();
    let read_result = stream.read_to_end(&mut response);
    let elapsed = started.elapsed();
    assert!(
        read_result.is_ok()
            || read_result
                .as_ref()
                .is_err_and(|error| error.kind() == std::io::ErrorKind::ConnectionReset),
        "timed out header connection did not close: {read_result:?}"
    );
    assert!(
        (Duration::from_millis(4500)..=Duration::from_secs(7)).contains(&elapsed),
        "header connection closed after {elapsed:?}, expected Go's five second timeout"
    );
    send_signal(&mut child, "TERM");
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
#[cfg(unix)]
fn slow_header_connections_are_bounded_and_backpressured() {
    let root = TestDir::new();
    let port = free_port();
    let mut child = start(root.path(), port, "127.0.0.1", false);
    wait_ready(&mut child, port);

    let mut held = Vec::with_capacity(128);
    for _ in 0..128 {
        held.push(TcpStream::connect(("127.0.0.1", port)).unwrap());
    }
    thread::sleep(Duration::from_millis(150));

    let mut queued = match TcpStream::connect(("127.0.0.1", port)) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            assert!(
                child.try_wait().unwrap().is_none(),
                "server exited under connection load"
            );
            drop(held);
            signal(&mut child, "TERM");
            return;
        }
        Err(error) => panic!("over-capacity connection failed unexpectedly: {error}"),
    };
    queued
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    queued
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut first_byte = [0; 1];
    match queued.read(&mut first_byte) {
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        Ok(0) => panic!("over-capacity connection was closed instead of backpressured"),
        Ok(count) => panic!(
            "over-capacity connection was served before a slot opened: {:?}",
            &first_byte[..count]
        ),
        Err(error) => panic!("over-capacity connection failed unexpectedly: {error}"),
    }

    drop(held.pop());
    queued
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let (status, _, _) = read_response(&mut queued);
    assert_eq!(
        status, 405,
        "queued connection was not served after a slot opened"
    );
    drop(held);
    signal(&mut child, "TERM");
}

#[test]
#[cfg(unix)]
fn many_sequential_http_connections_complete_cleanly() {
    let root = TestDir::new();
    let port = free_port();
    let mut child = start(root.path(), port, "127.0.0.1", false);
    wait_ready(&mut child, port);
    for _ in 0..512 {
        let (status, content_type, _) = exchange(port, "GET", b"", &[]);
        assert_eq!(status, 405);
        assert_eq!(content_type, "application/json");
    }
    signal(&mut child, "TERM");
}

#[test]
#[cfg(unix)]
fn go_oracle_http_wire_transcripts_match() {
    // Reproduction: cargo test -p symeraseme-cli --test mcp_http_process go_oracle_http_wire_transcripts_match -- --exact
    // This compiles the checked-out Go CLI and compares real HTTP process transcripts.
    let root = TestDir::new();
    let oracle = build_go_oracle(root.path());

    let go_port = free_port();
    let rust_port = free_port();
    let go_root = root.path().join("go");
    let rust_root = root.path().join("rust");
    std::fs::create_dir_all(&go_root).unwrap();
    std::fs::create_dir_all(&rust_root).unwrap();
    let mut go = start_binary(&oracle, &go_root, go_port, "127.0.0.1", false);
    let mut rust = start(&rust_root, rust_port, "127.0.0.1", false);
    wait_ready(&mut go, go_port);
    wait_ready(&mut rust, rust_port);
    let go_token = std::fs::read_to_string(go_root.join("data/mcp_token")).unwrap();
    let rust_token = token(&rust_root);

    let cases: [OracleCase<'_>; 6] = [
        ("GET", b"", vec![]),
        ("POST", br#"{}"#, vec![]),
        (
            "POST",
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            vec![
                ("Authorization", format!("Bearer {go_token}")),
                ("Origin", "https://evil.example".into()),
            ],
        ),
        (
            "POST",
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            vec![
                ("Authorization", format!("Bearer {go_token}")),
                ("Origin", String::new()),
            ],
        ),
        (
            "POST",
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            vec![
                ("Authorization", format!("Bearer {go_token}")),
                ("Origin", "http://[::1]:8000".into()),
            ],
        ),
        (
            "POST",
            br#"{"jsonrpc":"2.0","method":"initialize"}"#,
            vec![("Authorization", format!("Bearer {go_token}"))],
        ),
    ];
    for (method, body, headers) in cases {
        let go_headers: Vec<_> = headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let go_reply = exchange(go_port, method, body, &go_headers);
        let rust_headers: Vec<_> = headers
            .iter()
            .map(|(name, value)| {
                (
                    *name,
                    if *name == "Authorization" {
                        format!("Bearer {rust_token}")
                    } else {
                        value.clone()
                    },
                )
            })
            .collect();
        let rust_headers: Vec<_> = rust_headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let rust_reply = exchange(rust_port, method, body, &rust_headers);
        assert_eq!(
            rust_reply, go_reply,
            "wire transcript differs for {method} {body:?}"
        );
    }
    signal(&mut go, "TERM");
    signal(&mut rust, "TERM");
}

#[test]
#[cfg(unix)]
fn occupied_bind_error_matches_go_for_ipv4_and_ipv6() {
    let root = TestDir::new();
    let oracle = build_go_oracle(root.path());
    let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));

    let mut hosts = vec!["127.0.0.1"];
    if TcpListener::bind("[::1]:0").is_ok() {
        hosts.push("::1");
    }
    for host in hosts {
        let bind_address = if host.contains(':') {
            format!("[{host}]:0")
        } else {
            format!("{host}:0")
        };
        let occupied = TcpListener::bind(&bind_address).unwrap();
        let port = occupied.local_addr().unwrap().port();
        let go_root = root.path().join(format!("go-{host}"));
        let rust_root = root.path().join(format!("rust-{host}"));
        std::fs::create_dir_all(&go_root).unwrap();
        std::fs::create_dir_all(&rust_root).unwrap();
        let (go_status, go_stderr) = startup_error(&oracle, &go_root, port, host);
        let (rust_status, rust_stderr) = startup_error(rust, &rust_root, port, host);
        assert!(!go_status.success());
        assert!(!rust_status.success());
        assert_eq!(
            rust_stderr, go_stderr,
            "bind startup error differs for {host}"
        );
        let address = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        assert_eq!(
            rust_stderr.trim(),
            format!("listen tcp {address}: bind: address already in use")
        );
        drop(occupied);
    }
}

#[test]
#[cfg(unix)]
fn obs_text_origin_rejection_matches_go_and_rust_processes() {
    let root = TestDir::new();
    let oracle = build_go_oracle(root.path());
    let go_port = free_port();
    let rust_port = free_port();
    let go_root = root.path().join("go-obs-text");
    let rust_root = root.path().join("rust-obs-text");
    std::fs::create_dir_all(&go_root).unwrap();
    std::fs::create_dir_all(&rust_root).unwrap();
    let mut go = start_binary(&oracle, &go_root, go_port, "127.0.0.1", false);
    let mut rust = start(&rust_root, rust_port, "127.0.0.1", false);
    wait_ready(&mut go, go_port);
    wait_ready(&mut rust, rust_port);
    let go_token = std::fs::read_to_string(go_root.join("data/mcp_token")).unwrap();
    let rust_token = token(&rust_root);
    let go_response = exchange_obs_text_origin(go_port, &go_token);
    let rust_response = exchange_obs_text_origin(rust_port, &rust_token);
    assert_eq!(go_response.0, 403, "Go rejected the raw obs-text Origin");
    assert_eq!(rust_response, go_response);
    signal(&mut go, "TERM");
    signal(&mut rust, "TERM");
}
