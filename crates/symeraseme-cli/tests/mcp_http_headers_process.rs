//! Native Go/Rust HTTP response parity, including every header and raw body.
//! Header names/order are HTTP-insensitive; only the validated Date value varies.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[path = "support/mcp_http_port.rs"]
mod mcp_http_port;
use mcp_http_port::{StartedChild, accepts_token, free_port, spawn_with_handoff};

struct Server(StartedChild);

impl Drop for Server {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn start(binary: &Path, root: &Path) -> (Server, u16, String) {
    std::fs::create_dir_all(root).unwrap();
    let mut port = free_port();
    let token_path = root.join("data/mcp_token");
    let server = spawn_with_handoff(
        &mut port,
        &token_path,
        Duration::from_secs(10),
        |candidate, stderr| {
            let mut command = Command::new(binary);
            command
                .args([
                    "mcp",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &candidate.to_string(),
                ])
                .current_dir(root)
                .env_clear()
                .env("HOME", root)
                .env("USERPROFILE", root)
                .env("SYMERASEME_DATA_DIR", root.join("data"))
                .env("TMPDIR", root)
                .env("TMP", root)
                .env("TEMP", root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr);
            for name in ["CONFIG", "DATA", "STATE", "CACHE"] {
                command.env(format!("XDG_{name}_HOME"), root.join(name.to_lowercase()));
            }
            // Windows system APIs need the OS directory, not operator credentials/config.
            if let Some(system_root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", system_root);
            }
            command.spawn()
        },
    );
    let token = std::fs::read_to_string(token_path).unwrap();
    (Server(server), port, token)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Response {
    status_line: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn parse_response(raw: &[u8]) -> Response {
    let split = raw.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    let head = std::str::from_utf8(&raw[..split]).unwrap();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap().to_owned();
    let mut headers = Vec::new();
    let mut dates = 0;
    for line in lines {
        let (name, value) = line.split_once(':').expect("well-formed header");
        let name = name.to_ascii_lowercase();
        let mut value = value.to_owned();
        if name == "date" {
            let date = value.strip_prefix(' ').expect("HTTP Date separator");
            let parsed = chrono::DateTime::parse_from_rfc2822(date).expect("valid HTTP Date");
            assert_eq!(
                date,
                parsed
                    .with_timezone(&chrono::Utc)
                    .format("%a, %d %b %Y %H:%M:%S GMT")
                    .to_string(),
                "canonical HTTP Date"
            );
            dates += 1;
            value = "<DATE>".to_owned();
        }
        headers.push((name, value));
    }
    assert_eq!(dates, 1, "exactly one Date header, not silently omitted");
    headers.sort();
    let body = raw[split + 4..].to_vec();
    if let Some((_, length)) = headers.iter().find(|(name, _)| name == "content-length") {
        assert_eq!(
            body.len(),
            length.trim().parse::<usize>().unwrap(),
            "complete body"
        );
    } else {
        assert!(status_line.starts_with("HTTP/1.1 204 ") && body.is_empty());
    }
    Response {
        status_line,
        headers,
        body,
    }
}

fn exchange(port: u16, method: &str, body: &[u8], headers: &[(&str, String)]) -> Response {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "{method} / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )
    .unwrap();
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").unwrap();
    }
    stream.write_all(b"\r\n").unwrap();
    stream.write_all(body).unwrap();
    let mut raw = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut buffer = [0; 4096];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "total response deadline");
        stream.set_read_timeout(Some(remaining)).unwrap();
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                assert!(raw.len() + count <= 64 * 1024, "bounded response capture");
                raw.extend_from_slice(&buffer[..count]);
            }
            Err(error) => {
                // Windows may reset after rejection; parsing still requires a
                // complete response, so a reset cannot hide truncated output.
                assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
                break;
            }
        }
    }
    parse_response(&raw)
}

fn assert_matches(candidate: &Response, oracle: &Response) {
    assert_eq!(candidate, oracle, "complete HTTP response parity");
}

#[test]
fn readiness_probe_is_bounded_when_peer_dribbles_bytes() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let (started, observed) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "probe never connected");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept probe: {error}"),
            }
        };
        // Windows inherits the listener's nonblocking mode.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).unwrap() > 0);
        started.send(()).unwrap();
        for _ in 0..32 {
            if stream.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let start = Instant::now();
    let authenticated = accepts_token(port, "probe-token", start + Duration::from_millis(200));
    let elapsed = start.elapsed();
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    peer.join().unwrap();
    assert!(!authenticated, "an incomplete response is not readiness");
    eprintln!("readiness probe elapsed: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(1),
        "readiness ignored its total bound: {elapsed:?}"
    );
}

#[test]
fn header_comparison_rejects_missing_extra_changed_and_duplicate_headers() {
    let raw = b"HTTP/1.1 200 OK\r\nDate: Tue, 29 Sep 2026 10:00:00 GMT\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}";
    let baseline = parse_response(raw);
    assert_matches(&baseline, &baseline);
    let mut variants = Vec::new();
    let mut missing = baseline.clone();
    missing.headers.retain(|(name, _)| name != "content-type");
    variants.push(missing);
    let mut extra = baseline.clone();
    extra.headers.push(("x-unexpected".into(), "value".into()));
    variants.push(extra);
    let mut changed = baseline.clone();
    changed.headers[0].1 = "changed".into();
    variants.push(changed);
    let mut duplicate = baseline.clone();
    duplicate.headers.push(duplicate.headers[0].clone());
    variants.push(duplicate);
    let mut body = baseline.clone();
    body.body = b"[]".to_vec();
    variants.push(body);
    for variant in variants {
        assert!(std::panic::catch_unwind(|| assert_matches(&variant, &baseline)).is_err());
    }
    assert!(std::panic::catch_unwind(|| parse_response(&raw[..raw.len() - 1])).is_err());
    let padded = String::from_utf8(raw.to_vec())
        .unwrap()
        .replace("application/json", "application/json ");
    assert!(
        std::panic::catch_unwind(|| assert_matches(&parse_response(padded.as_bytes()), &baseline))
            .is_err()
    );
    let non_http_date = String::from_utf8(raw.to_vec())
        .unwrap()
        .replace(" GMT", " +0000");
    assert!(std::panic::catch_unwind(|| parse_response(non_http_date.as_bytes())).is_err());
}

#[test]
fn native_http_complete_headers_and_bodies_match_go() {
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("oracle.exe");
    let build = Command::new("go")
        .args(["build", "-o"])
        .arg(&binary)
        .arg("./cmd/symeraseme")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let (go_server, go_port, go_token) = start(&binary, &root.path().join("go"));
    let (rust_server, rust_port, rust_token) = start(
        Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
        &root.path().join("rust"),
    );
    let cases: [(&str, &[u8], bool, Option<&str>); 10] = [
        ("GET", b"", false, None),
        ("POST", b"{}", false, None),
        ("POST", br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#, true, Some("https://evil.example")),
        ("POST", br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#, true, Some("")),
        ("POST", br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#, true, Some("http://[::1]:8000")),
        ("POST", br#"{"jsonrpc":"2.0","method":"initialize"}"#, true, None),
        ("POST", br#"[{"jsonrpc":"2.0","id":1,"method":"initialize"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#, true, None),
        ("POST", br#"[{"jsonrpc":"2.0","method":"notifications/initialized"}]"#, true, None),
        ("POST", b"[]", true, None),
        ("POST", br#"[{"jsonrpc":"2.0","id":1,"method":"initialize"},7]"#, true, None),
    ];
    for (index, (method, body, authorized, origin)) in cases.into_iter().enumerate() {
        let headers = |token: &str| {
            let mut headers = Vec::new();
            if authorized {
                headers.push(("Authorization", format!("Bearer {token}")));
            }
            if let Some(origin) = origin {
                headers.push(("Origin", origin.to_owned()));
            }
            headers
        };
        let rust = exchange(rust_port, method, body, &headers(&rust_token));
        let go = exchange(go_port, method, body, &headers(&go_token));
        let expected_status = [405, 401, 403, 200, 200, 204, 200, 204, 200, 200][index];
        for reply in [&rust, &go] {
            assert_eq!(
                reply.status_line.split_whitespace().nth(1).unwrap(),
                expected_status.to_string(),
                "case {index}"
            );
        }
        assert_matches(&rust, &go);
    }
    drop(go_server);
    drop(rust_server);
}
