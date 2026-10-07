//! Exercise token-authenticated MCP over the real local process/network path.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use symeraseme_core::storage::Store;
use symeraseme_core::storage::repository::Repository;

#[path = "support/mcp_http_port.rs"]
mod mcp_http_port;
use mcp_http_port::{StartedChild, accepts_token, free_port, spawn_with_handoff};

#[path = "support/capture_http_aux.rs"]
mod capture_http_aux;
#[path = "support/capture_http_wire.rs"]
mod capture_http_wire;
#[path = "support/frozen_http_aux.rs"]
mod frozen_http_aux;
#[path = "support/frozen_http_wire.rs"]
mod frozen_http_wire;
#[path = "support/interrupted_read.rs"]
mod interrupted_read;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const THIS_FILE: &str = "crates/symeraseme-cli/tests/mcp_http_process.rs";
const GO_AGENT_CANCEL_FIXTURE: &str =
    include_str!("../../../tests/fixtures/agent-cancel/http.json");
const GO_PROVIDER_CANCEL_FIXTURE: &str =
    include_str!("../../../tests/fixtures/provider-cancel/http.json");

struct TestDir(PathBuf);
type OracleCase<'a> = (&'a str, &'a [u8], Vec<(&'a str, String)>);

impl TestDir {
    fn new() -> Self {
        // macOS clocks tick in microseconds, so parallel tests can read the same
        // nanosecond value; the counter keeps each test's data dir (and token) private.
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "symeraseme-mcp-http-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn start(root: &Path, port: &mut u16, host: &str, allow_remote: bool) -> StartedChild {
    start_binary(
        Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
        root,
        port,
        host,
        allow_remote,
    )
}

fn start_binary(
    binary: &Path,
    root: &Path,
    port: &mut u16,
    host: &str,
    allow_remote: bool,
) -> StartedChild {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    spawn_with_handoff(
        port,
        &root.join("data/mcp_token"),
        Duration::from_secs(5),
        |candidate, stderr| {
            let mut command = Command::new(binary);
            command.args(["mcp", "--host", host, "--port", &candidate.to_string()]);
            if allow_remote {
                command.arg("--allow-remote");
            }
            command
                .env("HOME", &home)
                .env("USERPROFILE", &home)
                .env("SYMERASEME_DATA_DIR", root.join("data"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr)
                .spawn()
        },
    )
}

fn start_agent_server(binary: &Path, root: &Path, port: &mut u16, started: &Path) -> StartedChild {
    let home = root.join("home");
    let bin = root.join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let agent = bin.join("claude");
    std::fs::write(
        &agent,
        "#!/bin/sh\nprintf '%s' \"$$\" > \"$AGENT_STARTED\"\nexec /bin/sleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:/bin:/usr/bin", bin.display());
    spawn_with_handoff(
        port,
        &root.join("data/mcp_token"),
        Duration::from_secs(5),
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
                .env("HOME", &home)
                .env("USERPROFILE", &home)
                .env("SYMERASEME_DATA_DIR", root.join("data"))
                .env("SYMERASEME_LLM_PROVIDER", "agent")
                .env("SYMERASEME_AGENT_BACKEND", "claude")
                .env("AGENT_STARTED", started)
                .env("PATH", &path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr)
                .spawn()
        },
    )
}

fn seed_agent_reply(root: &Path) {
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let store = Store::open(data.join("symeraseme.db")).unwrap();
    let request_id = Repository::new(&store)
        .create_removal_request(
            "oracle-broker",
            "email",
            "oracle-campaign",
            "DE",
            "gdpr-art17.de.md.j2",
            "",
        )
        .unwrap();
    store
        .connection()
        .execute(
            "INSERT INTO inbox_replies (request_id, message_id, thread_id, from_addr, subject, snippet) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (request_id, "cancel-message", "cancel-thread", "privacy@example.invalid", "Please verify your address", "We need your current address."),
        )
        .unwrap();
}

fn post_then_disconnect(port: u16, bearer: &str) -> TcpStream {
    post_body_then_disconnect(
        port,
        bearer,
        br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"classify_reply","arguments":{"request_id":1}}}"#,
    )
}

fn post_body_then_disconnect(port: u16, bearer: &str, body: &[u8]) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(stream, "POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {bearer}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
    stream.write_all(body).unwrap();
    stream
}

fn wait_agent_started(path: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // The shell redirect truncates before printf writes, so an empty or
        // partial read means "not yet", not a malformed pid.
        if let Some(pid) = std::fs::read_to_string(path)
            .ok()
            .and_then(|pid| pid.parse().ok())
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "host agent did not start");
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_process_exit(pid: i32, server: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let alive = Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        if !alive {
            return;
        }
        if Instant::now() >= deadline {
            let _ = server.kill();
            let _ = server.wait();
            let _ = Command::new("/bin/kill")
                .args(["-KILL", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            panic!("cancelled host agent {pid} survived");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn startup_error(
    binary: &Path,
    root: &Path,
    port: u16,
    host: &str,
    allow_remote: bool,
) -> (std::process::ExitStatus, String) {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut command = Command::new(binary);
    command.args(["mcp", "--host", host, "--port", &port.to_string()]);
    if allow_remote {
        command.arg("--allow-remote");
    }
    let mut child = command
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
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .output()
        .expect("Go toolchain is required to reproduce the HTTP oracle transcript");
    assert!(
        build.status.success(),
        "Go oracle build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    oracle
}

fn assert_agent_cancel_oracle_matches(root: &Path) -> Vec<u8> {
    let generated_fixture = root.join("agent-cancel.json");
    let oracle = Command::new("go")
        .args(["run", "./rust-tests/parity/oracle/agent-cancel", "-fixture"])
        .arg(&generated_fixture)
        .current_dir(ROOT)
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .output()
        .expect("run source-pinned Go cancellation oracle");
    assert!(
        oracle.status.success(),
        "Go cancellation oracle failed: {}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let generated = std::fs::read(generated_fixture).unwrap();
    assert_eq!(
        generated,
        GO_AGENT_CANCEL_FIXTURE.as_bytes(),
        "Go 1.26.6 cancellation oracle or pinned source hashes drifted"
    );
    generated
}

fn assert_provider_cancel_oracle_matches(root: &Path) -> Vec<u8> {
    let generated_fixture = root.join("provider-cancel.json");
    let oracle = Command::new("go")
        .args([
            "run",
            "./rust-tests/parity/oracle/provider-cancel",
            "-fixture",
        ])
        .arg(&generated_fixture)
        .current_dir(ROOT)
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .output()
        .expect("run source-pinned Go provider cancellation oracle");
    assert!(
        oracle.status.success(),
        "Go provider cancellation oracle failed: {}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let generated = std::fs::read(generated_fixture).unwrap();
    assert_eq!(
        generated,
        GO_PROVIDER_CANCEL_FIXTURE.as_bytes(),
        "Go 1.26.6 provider cancellation oracle or pinned source hashes drifted"
    );
    generated
}

fn start_provider_server(
    binary: &Path,
    root: &Path,
    port: &mut u16,
    provider_url: &str,
) -> StartedChild {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    spawn_with_handoff(
        port,
        &root.join("data/mcp_token"),
        Duration::from_secs(5),
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
                .env("HOME", &home)
                .env("USERPROFILE", &home)
                .env("SYMERASEME_DATA_DIR", root.join("data"))
                .env("SYMERASEME_LLM_BASE_URL", provider_url)
                .env("OPENAI_API_KEY", "synthetic-cancel-key")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr)
                .spawn()
        },
    )
}

struct CapturedProviderRequest {
    request_line: String,
    authorization: String,
    body: Vec<u8>,
}

fn start_blocking_provider() -> (
    String,
    mpsc::Receiver<CapturedProviderRequest>,
    thread::JoinHandle<String>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let provider_url = format!("http://{}", listener.local_addr().unwrap());
    let (request_tx, request_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return format!("accept failed: {}", error.kind()),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let mut content_length = 0usize;
        let mut authorization = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap();
                } else if name.eq_ignore_ascii_case("authorization") {
                    authorization = value.trim().to_owned();
                }
            }
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).unwrap();
        request_tx
            .send(CapturedProviderRequest {
                request_line,
                authorization,
                body,
            })
            .unwrap();

        let mut byte = [0; 1];
        match reader.read(&mut byte) {
            Ok(0) => "eof".to_owned(),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::UnexpectedEof
                ) =>
            {
                format!("closed: {}", error.kind())
            }
            Err(error) => format!("read error: {}", error.kind()),
            Ok(count) => format!("unexpected data: {} bytes", count),
        }
    });
    (provider_url, request_rx, server)
}

fn wait_ready(child: &mut StartedChild, port: u16, root: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let stderr = child.stderr_text();
            panic!("MCP server exited early ({status}): {stderr}");
        }
        // A bare connect can reach a foreign listener on a port released by
        // free_port, and the token file can still hold a previous server's
        // token; only this server's current token authenticates.
        if accepts_current_token(port, root) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "MCP HTTP server did not start"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn accepts_current_token(port: u16, root: &Path) -> bool {
    let Ok(token) = std::fs::read_to_string(root.join("data/mcp_token")) else {
        return false;
    };
    accepts_token(port, &token, Instant::now() + Duration::from_millis(500))
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

fn read_response_chunk(stream: &mut TcpStream, chunk: &mut [u8]) -> std::io::Result<usize> {
    let budget = stream
        .read_timeout()?
        .expect("HTTP read requires its original finite timeout");
    let result = interrupted_read::retry_until(Instant::now() + budget, |remaining| {
        interrupted_read::rearm(stream, remaining)?;
        stream.read(chunk)
    });
    interrupted_read::rearm(stream, budget)?;
    result
}

fn read_response(stream: &mut TcpStream) -> (u16, String, Vec<u8>) {
    let mut response = Vec::new();
    let mut chunk = [0; 4096];
    let header_end = loop {
        let count = read_response_chunk(stream, &mut chunk).unwrap();
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
        let count = read_response_chunk(stream, &mut chunk).unwrap();
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
    let mut port = free_port();
    let mut child = start(root.path(), &mut port, "127.0.0.1", false);
    wait_ready(&mut child, port, root.path());
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
            child = start(root.path(), &mut port, "127.0.0.1", false);
            wait_ready(&mut child, port, root.path());
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

    let mut remote_port = free_port();
    let mut remote = start(root.path(), &mut remote_port, "0.0.0.0", true);
    wait_ready(&mut remote, remote_port, root.path());
    signal(&mut remote, "TERM");
}

#[test]
#[cfg(unix)]
fn signal_stops_accepting_before_in_flight_request_drains() {
    let root = TestDir::new();
    let mut port = free_port();
    let mut child = start(root.path(), &mut port, "127.0.0.1", false);
    wait_ready(&mut child, port, root.path());
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
fn live_http_disconnect_cancels_and_reaps_host_agent_like_go() {
    const TEST: &str = "live_http_disconnect_cancels_and_reaps_host_agent_like_go";
    let fixture: serde_json::Value = serde_json::from_str(GO_AGENT_CANCEL_FIXTURE).unwrap();
    assert_eq!(fixture["schema"], "symeraseme.go-oracle.agent-cancel.v1");
    assert_eq!(fixture["go_version"], "go1.26.6");
    assert_eq!(fixture["handler_error"], "context canceled");
    assert_eq!(fixture["child_exited"], true);

    let root = TestDir::new();
    let frozen = frozen_http_aux::observation(THIS_FILE, TEST);
    let live = frozen.is_none().then(|| {
        let generated = assert_agent_cancel_oracle_matches(root.path());
        let go = root.path().join("symeraseme-go-oracle");
        let build = Command::new("go")
            .args(["build", "-o"])
            .arg(&go)
            .arg("./cmd/symeraseme")
            .current_dir(ROOT)
            .env("GOTOOLCHAIN", "go1.26.6")
            .env("GOPROXY", "off")
            .env("GOSUMDB", "off")
            .output()
            .expect("build Go 1.26.6 MCP process oracle");
        assert!(
            build.status.success(),
            "Go MCP process build failed: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        (generated, go)
    });
    let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
    let mut observed = Vec::new();
    for (name, binary) in live
        .iter()
        .map(|(_, go)| ("go", go.as_path()))
        .chain([("rust", rust)])
    {
        let case = TestDir::new();
        seed_agent_reply(case.path());
        let mut port = free_port();
        let started = case.path().join("agent.started");
        let mut server = start_agent_server(binary, case.path(), &mut port, &started);
        wait_ready(&mut server, port, case.path());
        let bearer = token(case.path());
        let request = post_then_disconnect(port, &bearer);
        let agent_pid = wait_agent_started(&started);
        drop(request);
        wait_process_exit(agent_pid, &mut server);
        let running = server.try_wait().unwrap().is_none();
        assert!(
            running,
            "{name} MCP server stopped after a client disconnect"
        );

        let store = Store::open(case.path().join("data/symeraseme.db")).unwrap();
        let classification: Option<String> = store
            .connection()
            .query_row(
                "SELECT classified_as FROM inbox_replies WHERE message_id = 'cancel-message'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            classification, None,
            "{name} persisted a classification after cancellation"
        );
        observed.push(serde_json::json!({
            "host_agent_reaped": true,
            "server_running_after_disconnect": running,
            "classification": classification,
        }));

        send_signal(&mut server, "TERM");
    }
    let rust_observation = observed.pop().unwrap();
    let go_observation = match (&frozen, &live) {
        (Some(record), _) => {
            assert_eq!(
                frozen_http_aux::bytes(&record["oracle_fixture"]),
                GO_AGENT_CANCEL_FIXTURE.as_bytes(),
                "Go 1.26.6 cancellation oracle or pinned source hashes drifted"
            );
            record["process"].clone()
        }
        (None, Some((generated, go))) => {
            let process = observed.pop().unwrap();
            capture_http_aux::record(
                go,
                THIS_FILE,
                TEST,
                serde_json::json!({
                    "oracle_fixture": capture_http_aux::bytes(generated),
                    "process": process,
                }),
            );
            process
        }
        (None, None) => unreachable!(),
    };
    assert_eq!(
        rust_observation, go_observation,
        "Rust differs from the actual Go host-agent cancellation"
    );
}

#[test]
#[cfg(unix)]
fn live_http_disconnect_cancels_provider_request_like_go() {
    const TEST: &str = "live_http_disconnect_cancels_provider_request_like_go";
    let root = TestDir::new();
    let frozen = frozen_http_aux::observation(THIS_FILE, TEST);
    let generated = match &frozen {
        Some(record) => {
            let generated = frozen_http_aux::bytes(&record["oracle_fixture"]);
            assert_eq!(
                generated,
                GO_PROVIDER_CANCEL_FIXTURE.as_bytes(),
                "Go 1.26.6 provider cancellation oracle or pinned source hashes drifted"
            );
            generated
        }
        None => assert_provider_cancel_oracle_matches(root.path()),
    };
    let provider_fixture: serde_json::Value =
        serde_json::from_str(GO_PROVIDER_CANCEL_FIXTURE).unwrap();
    assert_eq!(
        provider_fixture["schema"],
        "symeraseme.go-oracle.provider-cancel.v1"
    );
    assert_eq!(provider_fixture["provider_canceled"], true);
    assert_eq!(provider_fixture["client_disconnected"], true);
    let go = frozen.is_none().then(|| build_go_oracle(root.path()));
    let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
    let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"classify_reply","arguments":{"request_id":1,"provider":"openai","model":"oracle-model"}}}"#;

    let mut observed = Vec::new();
    for (name, binary) in go
        .iter()
        .map(|go| ("go", go.as_path()))
        .chain([("rust", rust)])
    {
        let case = TestDir::new();
        seed_agent_reply(case.path());
        let (provider_url, request_rx, provider) = start_blocking_provider();
        let mut port = free_port();
        let mut server = start_provider_server(binary, case.path(), &mut port, &provider_url);
        wait_ready(&mut server, port, case.path());
        let bearer = token(case.path());
        let request = post_body_then_disconnect(port, &bearer, body);
        let provider_request = request_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|error| panic!("{name} did not reach the local provider: {error}"));
        assert_eq!(
            provider_request.request_line.trim(),
            "POST /chat/completions HTTP/1.1",
            "{name} provider request path"
        );
        assert_eq!(
            provider_request.authorization, "Bearer synthetic-cancel-key",
            "{name} synthetic provider credential"
        );
        let provider_body: serde_json::Value =
            serde_json::from_slice(&provider_request.body).unwrap();
        assert_eq!(provider_body["model"], "oracle-model");
        assert_eq!(provider_body["messages"][0]["role"], "system");
        assert_eq!(provider_body["messages"][1]["role"], "user");

        drop(request);
        let provider_result = provider.join().unwrap();
        let provider_saw_disconnect =
            provider_result == "eof" || provider_result.starts_with("closed:");
        assert!(
            provider_saw_disconnect,
            "{name} provider read after client disconnect: {provider_result}"
        );
        let running = server.try_wait().unwrap().is_none();
        assert!(
            running,
            "{name} MCP server stopped after a client disconnect"
        );
        let store = Store::open(case.path().join("data/symeraseme.db")).unwrap();
        let classification: Option<String> = store
            .connection()
            .query_row(
                "SELECT classified_as FROM inbox_replies WHERE message_id = 'cancel-message'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(classification, None, "{name} persisted after cancellation");
        drop(store);
        observed.push(serde_json::json!({
            "provider_request_line": provider_request.request_line.trim(),
            "provider_authorization": provider_request.authorization,
            "provider_model": provider_body["model"],
            "provider_roles": [provider_body["messages"][0]["role"], provider_body["messages"][1]["role"]],
            "provider_saw_disconnect": provider_saw_disconnect,
            "server_running_after_disconnect": running,
            "classification": classification,
        }));
        send_signal(&mut server, "TERM");
    }
    let rust_observation = observed.pop().unwrap();
    let go_observation = match (&frozen, &go) {
        (Some(record), _) => record["process"].clone(),
        (None, Some(go)) => {
            let process = observed.pop().unwrap();
            capture_http_aux::record(
                go,
                THIS_FILE,
                TEST,
                serde_json::json!({
                    "oracle_fixture": capture_http_aux::bytes(&generated),
                    "process": process,
                }),
            );
            process
        }
        (None, None) => unreachable!(),
    };
    assert_eq!(
        rust_observation, go_observation,
        "Rust differs from the actual Go provider cancellation"
    );
}

#[test]
#[cfg(unix)]
fn incomplete_request_headers_expire_at_the_go_five_second_timeout() {
    let root = TestDir::new();
    let mut port = free_port();
    let mut child = start(root.path(), &mut port, "127.0.0.1", false);
    wait_ready(&mut child, port, root.path());
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
    let mut port = free_port();
    let mut child = start(root.path(), &mut port, "127.0.0.1", false);
    wait_ready(&mut child, port, root.path());
    assert_eq!(exchange(port, "GET", b"", &[]).0, 405);

    let mut held = Vec::with_capacity(128);
    let fill_deadline = Instant::now() + Duration::from_secs(4);
    for _ in 0..128 {
        loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => {
                    held.push(stream);
                    break;
                }
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
                    assert!(
                        Instant::now() < fill_deadline,
                        "could not establish 128 slow-header connections: {error}"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("held connection failed unexpectedly: {error}"),
            }
        }
    }

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
            assert_eq!(
                exchange(port, "GET", b"", &[]).0,
                405,
                "server did not accept requests after overload"
            );
            eprintln!(
                "OS rejected the over-capacity socket; queued-client branch was not exercised: {error}"
            );
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
    let mut port = free_port();
    let mut child = start(root.path(), &mut port, "127.0.0.1", false);
    wait_ready(&mut child, port, root.path());
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
    // Unrecorded hosts and explicit capture/live modes execute the checked-out Go CLI.
    let root = TestDir::new();
    let frozen = frozen_http_wire::observations();
    let oracle = frozen.is_none().then(|| build_go_oracle(root.path()));

    let mut go_port = free_port();
    let mut rust_port = free_port();
    let go_root = root.path().join("go");
    let rust_root = root.path().join("rust");
    std::fs::create_dir_all(&go_root).unwrap();
    std::fs::create_dir_all(&rust_root).unwrap();
    let mut go = oracle
        .as_ref()
        .map(|binary| start_binary(binary, &go_root, &mut go_port, "127.0.0.1", false));
    let mut rust = start(&rust_root, &mut rust_port, "127.0.0.1", false);
    if let Some(child) = go.as_mut() {
        wait_ready(child, go_port, &go_root);
    }
    wait_ready(&mut rust, rust_port, &rust_root);
    let go_token = if go.is_some() {
        std::fs::read_to_string(go_root.join("data/mcp_token")).unwrap()
    } else {
        "recorded-private-native-bearer-token".to_owned()
    };
    let rust_token = token(&rust_root);

    let cases: [OracleCase<'_>; 10] = [
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
        (
            "POST",
            br#"[{"jsonrpc":"2.0","id":1,"method":"initialize"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#,
            vec![("Authorization", format!("Bearer {go_token}"))],
        ),
        (
            "POST",
            br#"[{"jsonrpc":"2.0","method":"notifications/initialized"}]"#,
            vec![("Authorization", format!("Bearer {go_token}"))],
        ),
        (
            "POST",
            b"[]",
            vec![("Authorization", format!("Bearer {go_token}"))],
        ),
        (
            "POST",
            br#"[{"jsonrpc":"2.0","id":1,"method":"initialize"},7]"#,
            vec![("Authorization", format!("Bearer {go_token}"))],
        ),
    ];
    let mut measured = Vec::new();
    for (index, (method, body, headers)) in cases.into_iter().enumerate() {
        let request = capture_http_wire::request(method, body, &headers);
        let go_headers: Vec<_> = headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let go_reply = if let Some(corpus) = &frozen {
            corpus.reply(index, &request)
        } else {
            let reply = exchange(go_port, method, body, &go_headers);
            measured.push(capture_http_wire::case(index, request, &reply));
            reply
        };
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
    if let Some(child) = go.as_mut() {
        signal(child, "TERM");
        let status = child.try_wait().unwrap().unwrap().code().unwrap();
        capture_http_wire::record(oracle.as_ref().unwrap(), status, &measured);
    }
    signal(&mut rust, "TERM");
}

struct StaleTokenListener {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for StaleTokenListener {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

fn start_stale_token_listener(listener: TcpListener) -> StaleTokenListener {
    listener.set_nonblocking(true).unwrap();
    let (stop, stop_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        loop {
            if stop_rx.try_recv().is_ok() {
                return;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut reader = BufReader::new(stream);
                    let mut authorization = String::new();
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if reader.read_line(&mut line).is_err() || line.is_empty() || line == "\r\n"
                        {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':')
                            && name.eq_ignore_ascii_case("authorization")
                        {
                            authorization = value.trim().to_owned();
                        }
                    }
                    let stale = authorization == "Bearer stale-token";
                    let response = if stale {
                        let body = r#"{"result":{"serverInfo":{"name":"symeraseme"}}}"#;
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    } else {
                        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
                    };
                    let _ = reader.into_inner().write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("foreign listener accept failed: {error}"),
            }
        }
    });
    StaleTokenListener {
        stop,
        worker: Some(worker),
    }
}

#[test]
#[cfg(unix)]
fn startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness() {
    const TEST: &str = "startup_handoff_reselects_a_stolen_candidate_and_rejects_stale_readiness";
    let root = TestDir::new();
    let frozen = frozen_http_aux::observation(THIS_FILE, TEST);
    let oracle = frozen.is_none().then(|| build_go_oracle(root.path()));
    let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));

    let mut observed = Vec::new();
    for (name, binary) in oracle
        .iter()
        .map(|go| ("go", go.as_path()))
        .chain([("rust", rust)])
    {
        let case = TestDir::new();
        let data = case.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("mcp_token"), "stale-token").unwrap();

        let mut port = free_port();
        let occupied = TcpListener::bind(("127.0.0.1", port)).unwrap();
        let occupied_port = occupied.local_addr().unwrap().port();
        let foreign = start_stale_token_listener(occupied);
        let stale_probe_deadline = Instant::now() + Duration::from_secs(5);
        while !accepts_current_token(port, case.path()) {
            assert!(
                Instant::now() < stale_probe_deadline,
                "the foreign listener did not respond with its stale-token success"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let mut server = start_binary(binary, case.path(), &mut port, "127.0.0.1", false);
        assert_ne!(
            port, occupied_port,
            "{name} did not reselect the stolen port"
        );
        wait_ready(&mut server, port, case.path());
        let rotated = token(case.path()) != "stale-token";
        assert!(rotated);
        let running = server.try_wait().unwrap().is_none();
        assert!(running);
        observed.push(serde_json::json!({
            "reselected_stolen_candidate": port != occupied_port,
            "rotated_stale_token": rotated,
            "running_after_ready": running,
        }));
        eprintln!("{name} startup reselected {occupied_port} -> {port}");
        signal(&mut server, "TERM");
        drop(foreign);
    }
    let rust_observation = observed.pop().unwrap();
    let go_observation = match (frozen, &oracle) {
        (Some(record), _) => record,
        (None, Some(go)) => {
            let go_observation = observed.pop().unwrap();
            capture_http_aux::record(go, THIS_FILE, TEST, go_observation.clone());
            go_observation
        }
        (None, None) => unreachable!(),
    };
    assert_eq!(
        rust_observation, go_observation,
        "Rust differs from the actual Go startup handoff"
    );
}

#[test]
#[cfg(unix)]
fn occupied_bind_error_matches_go_for_ipv4_and_ipv6() {
    const TEST: &str = "occupied_bind_error_matches_go_for_ipv4_and_ipv6";
    let root = TestDir::new();
    let frozen = frozen_http_aux::observation(THIS_FILE, TEST);
    let oracle = frozen.is_none().then(|| build_go_oracle(root.path()));
    let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));

    let mut hosts = vec!["127.0.0.1"];
    if TcpListener::bind("[::1]:0").is_ok() {
        hosts.push("::1");
    }
    if let Some(record) = &frozen {
        assert_eq!(
            record["cases"].as_array().unwrap().len(),
            hosts.len(),
            "recorded Go loopback families differ from this host"
        );
    }
    let mut measured = Vec::new();
    for (index, host) in hosts.into_iter().enumerate() {
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
        let (go_success, go_stderr) = go_startup_error(
            frozen.as_ref().map(|record| &record["cases"][index]),
            oracle.as_deref(),
            &mut measured,
            &go_root,
            port,
            host,
        );
        let (rust_status, rust_stderr) = startup_error(rust, &rust_root, port, host, true);
        assert!(!go_success);
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
    if let Some(go) = &oracle {
        capture_http_aux::record(go, THIS_FILE, TEST, serde_json::json!({"cases": measured}));
    }
}

/// Actual Go startup failure, or the recorded one with this run's port.
fn go_startup_error(
    recorded: Option<&serde_json::Value>,
    oracle: Option<&Path>,
    measured: &mut Vec<serde_json::Value>,
    root: &Path,
    port: u16,
    host: &str,
) -> (bool, String) {
    if let Some(case) = recorded {
        let (code, stderr) = frozen_http_aux::startup_failure(case, host, port);
        return (code == Some(0), String::from_utf8(stderr).unwrap());
    }
    let (status, stderr) = startup_error(oracle.unwrap(), root, port, host, true);
    measured.push(capture_http_aux::startup_failure(
        host,
        port,
        status.code(),
        stderr.as_bytes(),
    ));
    (status.success(), stderr)
}

#[test]
#[cfg(unix)]
fn unavailable_local_address_error_matches_go() {
    const TEST: &str = "unavailable_local_address_error_matches_go";
    let host = "192.0.2.1";
    let probe = TcpListener::bind(format!("{host}:0")).unwrap_err();
    assert_eq!(
        probe.kind(),
        std::io::ErrorKind::AddrNotAvailable,
        "the reserved TEST-NET address must be unavailable on this host"
    );

    let root = TestDir::new();
    let frozen = frozen_http_aux::observation(THIS_FILE, TEST);
    let oracle = frozen.is_none().then(|| build_go_oracle(root.path()));
    let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
    let port = free_port();
    let go_root = root.path().join("go-unavailable");
    let rust_root = root.path().join("rust-unavailable");
    std::fs::create_dir_all(&go_root).unwrap();
    std::fs::create_dir_all(&rust_root).unwrap();
    let mut measured = Vec::new();
    let (go_success, go_stderr) = go_startup_error(
        frozen.as_ref().map(|record| &record["cases"][0]),
        oracle.as_deref(),
        &mut measured,
        &go_root,
        port,
        host,
    );
    let (rust_status, rust_stderr) = startup_error(rust, &rust_root, port, host, true);
    assert!(!go_success);
    assert!(!rust_status.success());
    assert_eq!(rust_stderr, go_stderr);
    let native_message = if cfg!(target_os = "linux") {
        "cannot assign requested address"
    } else {
        "can't assign requested address"
    };
    assert_eq!(
        rust_stderr.trim(),
        format!("listen tcp {host}:{port}: bind: {native_message}")
    );
    if let Some(record) = &frozen {
        assert_eq!(record["cases"].as_array().unwrap().len(), 1);
    }
    if let Some(go) = &oracle {
        capture_http_aux::record(go, THIS_FILE, TEST, serde_json::json!({"cases": measured}));
    }
}

#[test]
#[cfg(unix)]
fn obs_text_origin_rejection_matches_go_and_rust_processes() {
    const TEST: &str = "obs_text_origin_rejection_matches_go_and_rust_processes";
    let root = TestDir::new();
    let frozen = frozen_http_aux::observation(THIS_FILE, TEST);
    let oracle = frozen.is_none().then(|| build_go_oracle(root.path()));
    let mut go_port = free_port();
    let mut rust_port = free_port();
    let go_root = root.path().join("go-obs-text");
    let rust_root = root.path().join("rust-obs-text");
    std::fs::create_dir_all(&go_root).unwrap();
    std::fs::create_dir_all(&rust_root).unwrap();
    let mut go = oracle
        .as_ref()
        .map(|binary| start_binary(binary, &go_root, &mut go_port, "127.0.0.1", false));
    let mut rust = start(&rust_root, &mut rust_port, "127.0.0.1", false);
    if let Some(child) = go.as_mut() {
        wait_ready(child, go_port, &go_root);
    }
    wait_ready(&mut rust, rust_port, &rust_root);
    let rust_token = token(&rust_root);
    let go_response = match &frozen {
        Some(record) => (
            u16::try_from(record["status"].as_u64().unwrap()).unwrap(),
            record["content_type"].as_str().unwrap().to_owned(),
            frozen_http_aux::bytes(&record["body"]),
        ),
        None => {
            let go_token = std::fs::read_to_string(go_root.join("data/mcp_token")).unwrap();
            exchange_obs_text_origin(go_port, &go_token)
        }
    };
    let rust_response = exchange_obs_text_origin(rust_port, &rust_token);
    assert_eq!(go_response.0, 403, "Go rejected the raw obs-text Origin");
    assert_eq!(rust_response, go_response);
    if let Some(child) = go.as_mut() {
        signal(child, "TERM");
        capture_http_aux::record(
            oracle.as_ref().unwrap(),
            THIS_FILE,
            TEST,
            serde_json::json!({
                "status": go_response.0,
                "content_type": go_response.1,
                "body": capture_http_aux::bytes(&go_response.2),
            }),
        );
    }
    signal(&mut rust, "TERM");
}
