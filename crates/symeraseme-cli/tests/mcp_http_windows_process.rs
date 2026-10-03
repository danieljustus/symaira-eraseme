//! Native Windows Go/Rust MCP HTTP process contract.
#![cfg(windows)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use symeraseme_core::storage::Store;
use symeraseme_core::storage::repository::Repository;

#[path = "support/mcp_http_port.rs"]
mod mcp_http_port;
#[path = "support/native_host_agent.rs"]
mod native_host_agent;
use mcp_http_port::{
    StartedChild, accepts_token, free_port, read_bounded_response, spawn_with_handoff,
};

const GO_PROVIDER_CANCEL_FIXTURE: &str =
    include_str!("../../../tests/fixtures/provider-cancel/http.json");

struct Server(StartedChild);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn port() -> u16 {
    free_port()
}

fn start(binary: &Path, root: &Path, port: &mut u16) -> Server {
    start_with(binary, root, port, &[])
}

fn start_with(binary: &Path, root: &Path, port: &mut u16, envs: &[(&str, &str)]) -> Server {
    start_with_flags(binary, root, port, envs, 0x0000_0200)
}

fn start_with_flags(
    binary: &Path,
    root: &Path,
    port: &mut u16,
    envs: &[(&str, &str)],
    flags: u32,
) -> Server {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let process = spawn_with_handoff(
        port,
        &root.join("data/mcp_token"),
        Duration::from_secs(10),
        |candidate, stderr| {
            let mut command = Command::new(binary);
            if envs.iter().any(|(key, _)| *key == "AGENT_STARTED") {
                command.env_clear();
                for key in ["SystemRoot", "WINDIR"] {
                    if let Some(value) = std::env::var_os(key) {
                        command.env(key, value);
                    }
                }
                for (key, suffix) in [
                    ("XDG_CONFIG_HOME", "config"),
                    ("XDG_DATA_HOME", "data"),
                    ("XDG_STATE_HOME", "state"),
                    ("XDG_CACHE_HOME", "cache"),
                    ("TEMP", "tmp"),
                    ("TMP", "tmp"),
                ] {
                    let directory = home.join(suffix);
                    std::fs::create_dir_all(&directory).unwrap();
                    command.env(key, directory);
                }
            }
            command
                .creation_flags(flags)
                .args([
                    "mcp",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &candidate.to_string(),
                ])
                .env_clear()
                .current_dir(root)
                .env("HOME", &home)
                .env("USERPROFILE", &home)
                .env("SYMERASEME_DATA_DIR", root.join("data"))
                .env("TMP", &home)
                .env("TEMP", &home)
                .envs(envs.iter().copied())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr);
            for key in ["CONFIG", "DATA", "STATE", "CACHE"] {
                command.env(format!("XDG_{key}_HOME"), home.join(key.to_lowercase()));
            }
            for key in ["SystemRoot", "WINDIR"] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
            command.spawn()
        },
    );
    Server(process)
}

fn ready(server: &mut Server, port: u16, root: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            let stderr = server.0.stderr_text();
            panic!("MCP server exited early ({status}): {stderr}");
        }
        if let Ok(token) = std::fs::read_to_string(root.join("data/mcp_token"))
            && accepts_token(port, &token, deadline)
        {
            return;
        }
        assert!(Instant::now() < deadline, "MCP server did not authenticate");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn exchange(port: u16, token: Option<&str>, origin: Option<&str>) -> (u16, Vec<u8>) {
    let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    // Stage the entire request before sending: an early auth/origin rejection
    // can reset while a separate body write is still in flight on Windows.
    let mut request = Vec::new();
    write!(
        request,
        "POST / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )
    .unwrap();
    if let Some(token) = token {
        write!(request, "Authorization: Bearer {token}\r\n").unwrap();
    }
    if let Some(origin) = origin {
        write!(request, "Origin: {origin}\r\n").unwrap();
    }
    request.extend_from_slice(b"\r\n");
    request.extend_from_slice(body);
    stream.write_all(&request).unwrap();
    let response = read_bounded_response(&mut stream, Instant::now() + Duration::from_secs(5))
        .expect("bounded native response capture");
    let split = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("native response must contain complete headers");
    let head = std::str::from_utf8(&response[..split]).unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = response[split + 4..].to_vec();
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .expect("native response must declare Content-Length");
    assert_eq!(body.len(), length, "native response body was truncated");
    (status, body)
}

fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `go <pre...> <path> <post...>` with the pinned offline toolchain.
fn go(pre: &[&str], path: &Path, post: &[&str]) -> std::process::Output {
    Command::new("go")
        .args(pre)
        .arg(path)
        .args(post)
        .current_dir(repo())
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .output()
        .unwrap()
}

fn build_oracle(root: &Path) -> std::path::PathBuf {
    let oracle = root.join("symeraseme-go-oracle.exe");
    let build = go(&["build", "-o"], &oracle, &["./cmd/symeraseme"]);
    assert!(
        build.status.success(),
        "Go oracle build: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    oracle
}

#[test]
fn native_windows_http_matches_checked_out_go() {
    let root = tempfile::tempdir().unwrap();
    let oracle = build_oracle(root.path());

    let go_root = root.path().join("go");
    let rust_root = root.path().join("rust");
    let (mut go_port, mut rust_port) = distinct_ports();
    let mut go = start(&oracle, &go_root, &mut go_port);
    let mut rust = start(
        Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
        &rust_root,
        &mut rust_port,
    );
    ready(&mut go, go_port, &go_root);
    ready(&mut rust, rust_port, &rust_root);
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
        &mut rust_port,
    );
    ready(&mut restarted, rust_port, &rust_root);
    assert_ne!(
        std::fs::read_to_string(rust_root.join("data/mcp_token")).unwrap(),
        rust_token
    );

    let mut refused = Server(StartedChild::from_child(
        Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
            .args(["mcp", "--host", "0.0.0.0", "--port", &port().to_string()])
            .env("HOME", rust_root.join("home"))
            .env("USERPROFILE", rust_root.join("home"))
            .env("SYMERASEME_DATA_DIR", rust_root.join("data"))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
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
    let stderr = refused.0.stderr_text();
    assert!(stderr.contains("refusing non-loopback MCP bind"));
}

fn distinct_ports() -> (u16, u16) {
    let first = port();
    let mut second = port();
    while second == first {
        second = port();
    }
    (first, second)
}

fn seed_reply(root: &Path) {
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

/// A handle to the exact running synthetic agent, opened before disconnect.
/// Waiting on it proves termination without PID reuse or tasklist parsing.
struct AgentProcess(*mut std::ffi::c_void);

#[link(name = "Kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn TerminateProcess(handle: *mut std::ffi::c_void, code: u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

impl AgentProcess {
    fn open(pid: u32) -> Self {
        // SAFETY: opens only the PID published by our private fake executable.
        let handle = unsafe { OpenProcess(0x0010_0001, 0, pid) }; // SYNCHRONIZE | TERMINATE
        assert!(
            !handle.is_null(),
            "open synthetic agent: {}",
            std::io::Error::last_os_error()
        );
        Self(handle)
    }
    fn wait(&self, milliseconds: u32) -> u32 {
        // SAFETY: the owned handle remains valid until Drop.
        unsafe { WaitForSingleObject(self.0, milliseconds) }
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        // SAFETY: terminate only our owned still-running fake agent on failure,
        // then close the handle in both success and failure paths.
        unsafe {
            if self.wait(0) == 258 {
                TerminateProcess(self.0, 99);
                WaitForSingleObject(self.0, 5000);
            }
            CloseHandle(self.0);
        }
    }
}

#[test]
fn native_windows_disconnect_reaps_host_agent_like_go() {
    let root = tempfile::tempdir().unwrap();
    let helper = native_host_agent::helper(root.path());
    let oracle = build_oracle(root.path());
    let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"classify_reply","arguments":{"request_id":1,"provider":"agent","save":true}}}"#;
    for (name, binary) in [
        ("go", oracle.as_path()),
        ("rust", Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"))),
    ] {
        let case = root.path().join(name);
        seed_reply(&case);
        let bin = case.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::copy(&helper, bin.join("claude.exe")).unwrap();
        let started = case.join("agent.pid");
        let path = bin.to_str().unwrap();
        let started_path = started.to_str().unwrap();
        let mut port = port();
        let mut server = start_with(
            binary,
            &case,
            &mut port,
            &[
                ("PATH", path),
                ("PATHEXT", ".EXE"),
                ("SYMERASEME_LLM_PROVIDER", "agent"),
                ("SYMERASEME_AGENT_BACKEND", "claude"),
                ("AGENT_STARTED", started_path),
            ],
        );
        ready(&mut server, port, &case);
        let token = std::fs::read_to_string(case.join("data/mcp_token")).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(client, "POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
        client.write_all(body).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let pid = loop {
            if let Ok(contents) = std::fs::read_to_string(&started) {
                break contents.trim().parse().unwrap();
            }
            assert!(server.0.try_wait().unwrap().is_none());
            assert!(
                Instant::now() < deadline,
                "{name} did not reach the fake host agent"
            );
            thread::sleep(Duration::from_millis(10));
        };
        let agent = AgentProcess::open(pid);
        assert_eq!(
            agent.wait(0),
            258,
            "agent must be running before disconnect"
        );
        drop(client);
        assert_eq!(
            agent.wait(5000),
            0,
            "{name} did not reap the agent after disconnect"
        );
        assert!(server.0.try_wait().unwrap().is_none());
        assert_eq!(exchange(port, Some(&token), None).0, 200);
        let store = Store::open(case.join("data/symeraseme.db")).unwrap();
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
            "{name} persisted a cancelled classification"
        );
        eprintln!(
            "native host-agent {name}: live before disconnect, terminated afterwards, server healthy, no classification"
        );
    }
}

#[test]
fn native_windows_pathext_availability_matches_live_go() {
    const CHILD: &str = "SYMERASEME_LOOKUP_CHILD";
    if std::env::var_os(CHILD).is_some() {
        println!("lookup={}", symeraseme_core::llm::cli_on_path("claude"));
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let helper = native_host_agent::helper(root.path());
    for (id, filename, pathext, relative) in [
        ("exe", "claude.EXE", ".EXE", false),
        ("com", "claude.com", ".COM;.EXE", false),
        ("cmd", "claude.cmd", ".CMD", false),
        ("normalized", "claude.exe", "EXE", false),
        ("default", "claude.exe", "", false),
        ("excluded", "claude.exe", ".COM", false),
        ("no-extension", "claude", ".EXE", false),
        ("empty-extension-list", "claude", ";", false),
        ("relative", "claude.exe", ".EXE", true),
    ] {
        let case = root.path().join(id);
        let bin = case.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::copy(&helper, bin.join(filename)).unwrap();
        let mut go = Command::new(&helper);
        let mut rust = Command::new(std::env::current_exe().unwrap());
        go.env_clear()
            .arg("--lookup")
            .env("SYMERASEME_ORACLE_SOURCE_ROOT", repo());
        rust.env_clear()
            .args([
                "--exact",
                "native_windows_pathext_availability_matches_live_go",
                "--nocapture",
            ])
            .env(CHILD, "1");
        for command in [&mut go, &mut rust] {
            // These lookup children need no credentials or operator profile.
            // Reconstruct the Windows loader environment and private roots.
            if let Some(value) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", value);
            }
            command
                .current_dir(&case)
                .env("HOME", &case)
                .env("USERPROFILE", &case)
                .env("PATH", if relative { Path::new("bin") } else { &bin })
                .env("PATHEXT", pathext)
                .env("GODEBUG", "")
                .env("NoDefaultCurrentDirectoryInExePath", "1");
        }
        let observation = native_host_agent::observe(go, &case, "go-lookup");
        assert!(observation.status.success());
        let observation: serde_json::Value = serde_json::from_slice(&observation.stdout).unwrap();
        use sha2::{Digest, Sha256};
        for (source, digest) in observation["sources_sha256"].as_object().unwrap() {
            let actual = Sha256::digest(std::fs::read(repo().join(source)).unwrap());
            let actual: String = actual.iter().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(actual, digest.as_str().unwrap(), "source-bound {source}");
        }
        assert_eq!(observation["found"], observation["available"]);
        let result = native_host_agent::observe(rust, &case, "rust-lookup");
        assert!(result.status.success());
        let expected = format!("lookup={}\n", observation["available"].as_bool().unwrap());
        assert!(
            String::from_utf8_lossy(&result.stdout).contains(&expected),
            "{id}: Rust disagrees with live Go {observation}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        eprintln!("native PATHEXT {id}: {observation}");
    }
}

/// Synthetic loopback OpenAI endpoint: captures one request, never answers,
/// and reports how the MCP server's upstream connection ended.
fn blocking_provider() -> (String, mpsc::Receiver<String>, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let mut length = 0usize;
        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        tx.send(request_line).unwrap();
        let mut byte = [0; 1];
        match reader.read(&mut byte) {
            Ok(0) => "eof".to_owned(),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::UnexpectedEof
                ) =>
            {
                "closed".to_owned()
            }
            other => format!("{other:?}"),
        }
    });
    (url, rx, handle)
}

/// DOM-008A on native Windows: dropping the MCP client socket mid tools/call
/// closes the upstream provider request in Go and Rust alike, the server keeps
/// running, and nothing is persisted.
#[test]
fn native_windows_disconnect_cancels_provider_request_like_go() {
    let root = tempfile::tempdir().unwrap();
    let fixture = root.path().join("provider-cancel.json");
    let oracle_run = go(
        &[
            "run",
            "./rust-tests/parity/oracle/provider-cancel",
            "-fixture",
        ],
        &fixture,
        &[],
    );
    assert!(
        oracle_run.status.success(),
        "Go provider cancellation oracle: {}",
        String::from_utf8_lossy(&oracle_run.stderr)
    );
    assert_eq!(
        std::fs::read(&fixture).unwrap(),
        GO_PROVIDER_CANCEL_FIXTURE.as_bytes(),
        "Go provider cancellation oracle drifted"
    );
    let oracle = build_oracle(root.path());
    let rust = Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
    let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"classify_reply","arguments":{"request_id":1,"provider":"openai","model":"oracle-model"}}}"#;

    for (name, binary) in [("go", oracle.as_path()), ("rust", rust)] {
        let case = root.path().join(name);
        seed_reply(&case);
        let (url, request_rx, provider) = blocking_provider();
        let mut port = port();
        let mut server = start_with(
            binary,
            &case,
            &mut port,
            &[
                ("SYMERASEME_LLM_BASE_URL", url.as_str()),
                ("OPENAI_API_KEY", "synthetic-cancel-key"),
            ],
        );
        ready(&mut server, port, &case);
        let token = std::fs::read_to_string(case.join("data/mcp_token")).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            client,
            "POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .unwrap();
        client.write_all(body).unwrap();
        let request_line = request_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|error| panic!("{name} did not reach the provider: {error}"));
        assert_eq!(request_line.trim(), "POST /chat/completions HTTP/1.1");

        drop(client);
        let outcome = provider.join().unwrap();
        assert!(
            outcome == "eof" || outcome == "closed",
            "{name} upstream after disconnect: {outcome}"
        );
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "{name} MCP server stopped after a client disconnect"
        );
        let store = Store::open(case.join("data/symeraseme.db")).unwrap();
        let classification: Option<String> = store
            .connection()
            .query_row(
                "SELECT classified_as FROM inbox_replies WHERE message_id = 'cancel-message'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(classification, None, "{name} persisted after cancellation");
    }
}

/// MCP-013 on native Windows: Go and Rust both close a connection whose
/// headers never complete at the five-second read-header timeout and keep
/// serving afterwards.
#[test]
fn native_windows_slow_header_timeout_matches_go() {
    let root = tempfile::tempdir().unwrap();
    let oracle = build_oracle(root.path());
    let (mut go_port, mut rust_port) = distinct_ports();
    let go_root = root.path().join("go");
    let rust_root = root.path().join("rust");
    let mut go_server = start(&oracle, &go_root, &mut go_port);
    let mut rust_server = start(
        Path::new(env!("CARGO_BIN_EXE_symeraseme-rust")),
        &rust_root,
        &mut rust_port,
    );
    ready(&mut go_server, go_port, &go_root);
    ready(&mut rust_server, rust_port, &rust_root);
    let pending: Vec<_> = [("go", go_port), ("rust", rust_port)]
        .into_iter()
        .map(|(name, port)| {
            thread::spawn(move || {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(8)))
                    .unwrap();
                stream
                    .write_all(
                        b"POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer partial",
                    )
                    .unwrap();
                let started = Instant::now();
                let mut response = Vec::new();
                let result = stream.read_to_end(&mut response).map_err(|e| e.kind());
                (name, result, started.elapsed())
            })
        })
        .collect();
    for handle in pending {
        let (name, result, elapsed) = handle.join().unwrap();
        assert!(
            matches!(
                result,
                Ok(_)
                    | Err(
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    )
            ),
            "{name} slow-header connection did not close: {result:?}"
        );
        assert!(
            (Duration::from_millis(4500)..=Duration::from_secs(8)).contains(&elapsed),
            "{name} closed the slow-header connection after {elapsed:?}"
        );
    }
    assert!(go_server.0.try_wait().unwrap().is_none());
    assert!(rust_server.0.try_wait().unwrap().is_none());
    assert_eq!(
        exchange(rust_port, None, None),
        exchange(go_port, None, None)
    );
}

/// Go maps CTRL_BREAK_EVENT to os.Interrupt. Use a private console and target
/// each child's process group, never the test runner's console/group zero.
/// https://learn.microsoft.com/en-us/windows/console/generateconsolectrlevent
#[test]
fn native_windows_ctrl_break_shutdown_matches_go() {
    console_shutdown_matches_go(false);
}

/// CTRL_C_EVENT cannot target a process group. Broadcast only inside the
/// separately created test console, with its controller ignoring the event.
#[test]
fn native_windows_ctrl_c_shutdown_matches_go() {
    console_shutdown_matches_go(true);
}

fn console_shutdown_matches_go(ctrl_c: bool) {
    let signal = if ctrl_c { "Ctrl+C" } else { "Ctrl+Break" };
    let test = if ctrl_c {
        "native_windows_ctrl_c_shutdown_matches_go"
    } else {
        "native_windows_ctrl_break_shutdown_matches_go"
    };
    const CHILD_ROOT: &str = "SYMERASEME_CTRL_BREAK_TEST_ROOT";
    const GO_ORACLE: &str = "SYMERASEME_CTRL_BREAK_GO_ORACLE";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = std::path::PathBuf::from(root);
        let oracle = std::path::PathBuf::from(std::env::var_os(GO_ORACLE).unwrap());
        for (name, binary) in [
            ("go", oracle.as_path()),
            ("rust", Path::new(env!("CARGO_BIN_EXE_symeraseme-rust"))),
        ] {
            let mut port = port();
            let server_root = root.join(name);
            #[link(name = "Kernel32")]
            unsafe extern "system" {
                fn GenerateConsoleCtrlEvent(event: u32, process_group: u32) -> i32;
                fn SetConsoleCtrlHandler(
                    handler: Option<unsafe extern "system" fn(u32) -> i32>,
                    add: i32,
                ) -> i32;
            }
            if ctrl_c {
                // SAFETY: NULL resets this private controller's inheritable
                // ignore flag. The new server must inherit CTRL_C enabled.
                assert_ne!(unsafe { SetConsoleCtrlHandler(None, 0) }, 0);
            }
            let mut server = start_with_flags(
                binary,
                &server_root,
                &mut port,
                &[],
                if ctrl_c { 0 } else { 0x0000_0200 },
            );
            if ctrl_c {
                // SAFETY: ignore only in this already isolated controller,
                // after the child inherited the enabled flag. It owns the
                // only console descendants and starts them sequentially.
                assert_ne!(unsafe { SetConsoleCtrlHandler(None, 1) }, 0);
            }
            ready(&mut server, port, &server_root);
            // SAFETY: Ctrl+Break targets our owned live group. Ctrl+C group
            // zero broadcasts solely in this CREATE_NEW_CONSOLE helper;
            // it cannot reach the test runner or an operator's console.
            let sent = unsafe {
                GenerateConsoleCtrlEvent(
                    if ctrl_c { 0 } else { 1 },
                    if ctrl_c { 0 } else { server.0.id() },
                )
            };
            assert_ne!(
                sent,
                0,
                "send {signal}: {}",
                std::io::Error::last_os_error()
            );
            let deadline = Instant::now() + Duration::from_secs(8);
            let status = loop {
                if let Some(status) = server.0.try_wait().unwrap() {
                    break status;
                }
                assert!(Instant::now() < deadline, "{name} did not handle {signal}");
                thread::sleep(Duration::from_millis(20));
            };
            assert!(status.success(), "{name} {signal} exit: {status}");
            assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
        }
        println!("{signal} Go/Rust comparison executed both processes");
        return;
    }

    let root = tempfile::tempdir().unwrap();
    let oracle = build_oracle(root.path());
    let log_path = root.path().join("console.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut helper = Server(StartedChild::from_child(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(CHILD_ROOT, root.path())
            .env(GO_ORACLE, oracle)
            .creation_flags(0x0000_0010) // CREATE_NEW_CONSOLE
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(40);
    let status = loop {
        if let Some(status) = helper.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "isolated console test timed out");
        thread::sleep(Duration::from_millis(20));
    };
    let log = std::fs::read_to_string(log_path).unwrap();
    assert!(status.success(), "isolated console test failed: {log}");
    assert!(log.contains(&format!(
        "{signal} Go/Rust comparison executed both processes"
    )));
}
