//! MCP HTTP transport and process lifecycle.

use super::handler::ToolHandler;
use super::protocol::{
    InitializeOutcome, initialize_cancellable, skip_json_value, skip_whitespace,
};
use base64::Engine;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rand::Rng;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use subtle::ConstantTimeEq;
use symeraseme_core::llm::CancellationToken;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::sync::watch;
use tokio::task::JoinSet;

const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;
const MAX_HTTP_CONNECTIONS: usize = 128;

#[derive(Serialize)]
struct RpcError<'a> {
    code: i32,
    message: &'a str,
}

#[derive(Serialize)]
struct ErrorResponse<'a> {
    jsonrpc: &'static str,
    error: RpcError<'a>,
    id: (),
}

struct HttpReply {
    status: u16,
    body: Vec<u8>,
}

/// Run the MCP HTTP endpoint, preserving the Go CLI's bind, token, and shutdown contract.
pub(crate) fn serve(
    mut host: String,
    port: i64,
    allow_remote: bool,
    build_handler: impl FnOnce() -> Result<Arc<dyn ToolHandler>, String>,
) -> Result<(), String> {
    host = host.trim_matches(['[', ']']).to_owned();
    if host.is_empty() {
        host = "127.0.0.1".to_owned();
    }
    if !(1..=65535).contains(&port) {
        return Err(format!(
            "invalid MCP port {port}: must be between 1 and 65535"
        ));
    }
    if !allow_remote && !is_loopback_host(&host) {
        return Err(format!(
            "refusing non-loopback MCP bind {host:?} without --allow-remote"
        ));
    }

    let token = write_token().map_err(|error| format!("write MCP auth token: {error}"))?;
    let handler = build_handler()?;
    let address = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let stopping = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stopping))
        .map_err(|error| error.to_string())?;
    #[cfg(windows)]
    signal_hook::flag::register(signal_hook::consts::SIGBREAK, Arc::clone(&stopping))
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stopping))
        .map_err(|error| error.to_string())?;

    runtime.block_on(serve_async(address, token, handler, stopping))
}

async fn serve_async(
    address: String,
    token: Arc<String>,
    handler: Arc<dyn ToolHandler>,
    stopping: Arc<AtomicBool>,
) -> Result<(), String> {
    let listener = TcpListener::bind(&address)
        .await
        .map_err(|error| listen_error(&address, error))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let connection_slots = Arc::new(Semaphore::new(MAX_HTTP_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
        while connections.try_join_next().is_some() {}
        if stopping.load(Ordering::Relaxed) {
            break;
        }
        let permit = tokio::select! {
            permit = Arc::clone(&connection_slots).acquire_owned() => permit.map_err(|error| error.to_string())?,
            _ = tokio::time::sleep(Duration::from_millis(50)) => continue,
        };
        if stopping.load(Ordering::Relaxed) {
            drop(permit);
            break;
        }
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|error| error.to_string())?;
                let token = Arc::clone(&token);
                let handler = Arc::clone(&handler);
                let shutdown_rx = shutdown_rx.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let mut builder = http1::Builder::new();
                    builder.timer(hyper_util::rt::TokioTimer::new());
                    builder.header_read_timeout(Duration::from_secs(5));
                    let service = service_fn(move |request| {
                        let token = Arc::clone(&token);
                        let handler = Arc::clone(&handler);
                        async move { Ok::<_, std::convert::Infallible>(handle_request(request, &token, handler).await) }
                    });
                    let mut connection = Box::pin(builder.serve_connection(TokioIo::new(stream), service));
                    tokio::select! {
                        result = &mut connection => result.map_err(|error| error.to_string()),
                        _ = wait_for_shutdown(shutdown_rx) => {
                            connection.as_mut().graceful_shutdown();
                            connection.await.map_err(|error| error.to_string())
                        }
                    }
                });
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }

    // Dropping the listener closes the accept path before active requests drain.
    drop(listener);
    let _ = shutdown_tx.send(true);
    match tokio::time::timeout(Duration::from_secs(5), async {
        while connections.join_next().await.is_some() {}
    })
    .await
    {
        Ok(()) => Ok(()),
        Err(_) => {
            connections.abort_all();
            Err("context deadline exceeded".to_owned())
        }
    }
}

fn listen_error(address: &str, error: std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => {
            format!("listen tcp {address}: bind: address already in use")
        }
        std::io::ErrorKind::AddrNotAvailable => {
            format!(
                "listen tcp {address}: bind: {}",
                addr_not_available_message()
            )
        }
        _ => format!("listen tcp {address}: {error}"),
    }
}

#[cfg(target_os = "linux")]
fn addr_not_available_message() -> &'static str {
    "cannot assign requested address"
}

#[cfg(not(target_os = "linux"))]
fn addr_not_available_message() -> &'static str {
    "can't assign requested address"
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    let _ = shutdown.changed().await;
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|address| match address {
            IpAddr::V4(address) => address.is_loopback(),
            IpAddr::V6(address) => {
                address.is_loopback()
                    || address
                        .to_ipv4_mapped()
                        .is_some_and(|mapped| mapped.is_loopback())
            }
        })
}

fn write_token() -> std::io::Result<Arc<String>> {
    let dir = symeraseme_core::identity::default_consent_directory()?;
    create_token_directory(&dir)?;
    let mut bytes = [0; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let path = dir.join("mcp_token");
    write_token_file(&path, &token)?;
    Ok(Arc::new(token))
}

#[cfg(unix)]
fn write_token_file(path: &Path, token: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(token.as_bytes())?;
    set_mode(path)
}

#[cfg(not(unix))]
fn write_token_file(path: &Path, token: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(token.as_bytes())
}

#[cfg(unix)]
fn create_token_directory(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn create_token_directory(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

#[cfg(unix)]
fn set_mode(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

async fn handle_request(
    request: Request<Incoming>,
    token: &str,
    handler: Arc<dyn ToolHandler>,
) -> Response<Full<Bytes>> {
    let declared_too_large = request
        .headers()
        .get(http::header::CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok())
        .and_then(|length| length.parse::<usize>().ok())
        .is_some_and(|length| length > MAX_BODY_BYTES);
    let reply = if request.method() != Method::POST {
        rpc_reply(405, -32600, "POST required")
    } else if !origin_header_allowed(request.headers().get(http::header::ORIGIN)) {
        rpc_reply(403, -32000, "Forbidden: disallowed Origin")
    } else if !authorized(request.headers(), token) {
        rpc_reply(401, -32000, "Unauthorized")
    } else if declared_too_large {
        rpc_reply(413, -32600, "Invalid Request")
    } else {
        let body = Limited::new(request.into_body(), MAX_BODY_BYTES + 1)
            .collect()
            .await;
        if let Ok(body) = body {
            let body = body.to_bytes();
            if body.len() <= MAX_BODY_BYTES {
                let cancellation = CancellationToken::default();
                let mut cancel_on_drop = CancelOnDrop::new(cancellation.clone());
                let result = tokio::task::spawn_blocking(move || {
                    protocol_reply_cancellable(&body, handler.as_ref(), &cancellation)
                })
                .await;
                cancel_on_drop.disarm();
                match result {
                    Ok(reply) => reply,
                    Err(_) => rpc_reply(200, -32603, "Internal server error"),
                }
            } else {
                rpc_reply(200, -32700, "parse error")
            }
        } else {
            rpc_reply(200, -32700, "parse error")
        }
    };

    let mut response = Response::builder()
        .status(StatusCode::from_u16(reply.status).expect("valid static status"));
    if reply.status != 204 {
        response = response.header(http::header::CONTENT_TYPE, "application/json");
    }
    response
        .body(Full::new(Bytes::from(reply.body)))
        .expect("static response headers")
}

fn authorized(headers: &http::HeaderMap, token: &str) -> bool {
    let mut values = headers.get_all(http::header::AUTHORIZATION).iter();
    let Some(header) = values.next().and_then(|header| header.to_str().ok()) else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let Some(supplied) = header.strip_prefix("Bearer ") else {
        return false;
    };
    if supplied.is_empty() {
        return false;
    }
    let length_match = supplied.len().ct_eq(&token.len());
    let comparison = if supplied.len() == token.len() {
        supplied.as_bytes()
    } else {
        token.as_bytes()
    };
    bool::from(length_match & comparison.ct_eq(token.as_bytes()))
}

fn allowed_origin(raw: &str) -> bool {
    raw.parse::<http::Uri>()
        .ok()
        .and_then(|url| {
            url.host()
                .map(|host| host.trim_matches(['[', ']']).to_ascii_lowercase())
        })
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
}

fn origin_header_allowed(origin: Option<&http::HeaderValue>) -> bool {
    origin.is_none_or(|origin| origin.is_empty() || origin.to_str().is_ok_and(allowed_origin))
}

fn protocol_reply(body: &[u8], handler: &dyn ToolHandler) -> HttpReply {
    protocol_reply_cancellable(body, handler, &CancellationToken::default())
}

fn protocol_reply_cancellable(
    body: &[u8],
    handler: &dyn ToolHandler,
    cancellation: &CancellationToken,
) -> HttpReply {
    let (start, end) = match one_json_value(body) {
        Some(span) => span,
        None => return rpc_reply(200, -32700, "parse error"),
    };
    if body[start] != b'[' {
        return protocol_outcome(initialize_cancellable(
            &body[start..end],
            handler,
            cancellation,
        ));
    }

    let mut index = start + 1;
    skip_whitespace(body, &mut index);
    if body.get(index) == Some(&b']') {
        return rpc_reply(200, -32600, "invalid request");
    }
    let mut responses = Vec::new();
    while index < end - 1 {
        let Some(item_end) = skip_json_value(body, index) else {
            return rpc_reply(200, -32700, "parse error");
        };
        if let InitializeOutcome::Response(mut bytes) =
            initialize_cancellable(&body[index..item_end], handler, cancellation)
        {
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
            }
            responses.push(bytes);
        }
        index = item_end;
        skip_whitespace(body, &mut index);
        if body.get(index) == Some(&b',') {
            index += 1;
            skip_whitespace(body, &mut index);
        } else {
            break;
        }
    }
    if responses.is_empty() {
        return HttpReply {
            status: 204,
            body: Vec::new(),
        };
    }
    let mut result = Vec::from(b"[" as &[u8]);
    for (index, response) in responses.iter().enumerate() {
        if index > 0 {
            result.push(b',');
        }
        result.extend_from_slice(response);
    }
    result.extend_from_slice(b"]\n");
    HttpReply {
        status: 200,
        body: result,
    }
}

struct CancelOnDrop {
    cancellation: CancellationToken,
    armed: bool,
}

impl CancelOnDrop {
    fn new(cancellation: CancellationToken) -> Self {
        Self {
            cancellation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel();
        }
    }
}

fn one_json_value(body: &[u8]) -> Option<(usize, usize)> {
    let mut start = 0;
    skip_whitespace(body, &mut start);
    let end = skip_json_value(body, start)?;
    let mut trailing = end;
    skip_whitespace(body, &mut trailing);
    (trailing == body.len()).then_some((start, end))
}

fn protocol_outcome(outcome: InitializeOutcome) -> HttpReply {
    match outcome {
        InitializeOutcome::Response(body) => HttpReply { status: 200, body },
        InitializeOutcome::Notification => HttpReply {
            status: 204,
            body: Vec::new(),
        },
        InitializeOutcome::ParseError => rpc_reply(200, -32700, "parse error"),
    }
}

fn rpc_reply(status: u16, code: i32, message: &'static str) -> HttpReply {
    let body = serde_json::to_vec(&ErrorResponse {
        jsonrpc: "2.0",
        error: RpcError { code, message },
        id: (),
    })
    .expect("RPC error serializes");
    let mut body = body;
    body.push(b'\n');
    HttpReply { status, body }
}

#[cfg(test)]
mod origin_tests {
    use super::origin_header_allowed;
    use http::{HeaderMap, HeaderValue, header};

    #[test]
    fn origin_header_validation_distinguishes_absent_empty_and_invalid_bytes() {
        let absent = HeaderMap::new();
        assert!(origin_header_allowed(absent.get(header::ORIGIN)));

        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, HeaderValue::from_static(""));
        assert!(origin_header_allowed(headers.get(header::ORIGIN)));

        for remote in [
            "https://example.invalid",
            "http://localhost.example.invalid",
            "http://127.0.0.2",
            "http://[::2]",
            "null",
            "not a URL",
        ] {
            headers.insert(header::ORIGIN, HeaderValue::from_str(remote).unwrap());
            assert!(
                !origin_header_allowed(headers.get(header::ORIGIN)),
                "untrusted origin {remote}"
            );
        }

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_bytes(b"http://localhost\xff").unwrap(),
        );
        assert!(!origin_header_allowed(headers.get(header::ORIGIN)));

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://localhost:8000"),
        );
        assert!(origin_header_allowed(headers.get(header::ORIGIN)));
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use crate::mcp::handler::test_support::no_backend_handler;
    use std::sync::Arc;

    const GO_AGENT_CANCEL_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/agent-cancel/http.json"
    ));

    /// An invalid port is rejected before the auth token is written and before
    /// the handler is built — the build callback must never run.
    #[test]
    fn serve_rejects_an_invalid_port_before_building_the_handler() {
        let result = serve(
            String::new(),
            0,
            false,
            || -> Result<Arc<dyn ToolHandler>, String> {
                panic!("the handler must not be built when the port is invalid")
            },
        );
        let error = result.expect_err("port 0 must be rejected");
        assert_eq!(error, "invalid MCP port 0: must be between 1 and 65535");
    }

    /// Go's `listen` error wording for each bind failure class.
    #[test]
    fn listen_error_words_each_bind_failure_like_go() {
        let in_use = std::io::Error::from(std::io::ErrorKind::AddrInUse);
        assert_eq!(
            listen_error("127.0.0.1:8080", in_use),
            "listen tcp 127.0.0.1:8080: bind: address already in use"
        );

        let unavailable = std::io::Error::from(std::io::ErrorKind::AddrNotAvailable);
        let expected_unavailable = if cfg!(target_os = "linux") {
            "cannot assign requested address"
        } else {
            "can't assign requested address"
        };
        assert_eq!(addr_not_available_message(), expected_unavailable);
        assert_eq!(
            listen_error("[::1]:8080", unavailable),
            format!("listen tcp [::1]:8080: bind: {}", expected_unavailable)
        );

        let other = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        assert_eq!(
            listen_error("127.0.0.1:8080", other),
            "listen tcp 127.0.0.1:8080: permission denied"
        );
    }

    /// A shutdown that was already signalled returns without awaiting a change.
    #[test]
    fn wait_for_shutdown_returns_when_the_signal_already_fired() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        let (_tx, rx) = tokio::sync::watch::channel(true);
        runtime.block_on(wait_for_shutdown(rx));
    }

    #[test]
    fn wait_for_shutdown_blocks_until_a_later_signal() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (tx, rx) = watch::channel(false);
            let mut task = tokio::spawn(wait_for_shutdown(rx));
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut task)
                    .await
                    .is_err()
            );
            tx.send(true).unwrap();
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
        });
    }

    #[test]
    fn actual_bind_failure_is_not_reported_as_a_successful_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let error = runtime
            .block_on(serve_async(
                address.clone(),
                Arc::new("synthetic-test-token".to_owned()),
                Arc::new(no_backend_handler()),
                Arc::new(AtomicBool::new(true)),
            ))
            .unwrap_err();
        assert_eq!(
            error,
            format!("listen tcp {address}: bind: address already in use")
        );
    }

    #[test]
    fn token_files_are_written_and_tightened_inside_a_private_directory() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        create_token_directory(&data).unwrap();
        assert!(data.is_dir());
        let path = data.join("mcp_token");
        write_token_file(&path, "synthetic-private-token").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"synthetic-private-token");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&data).unwrap().permissions().mode() & 0o777,
                0o700
            );
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            write_token_file(&path, "replacement-synthetic-token").unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn bind_policy_and_token_generation_are_exercised_in_private_child_processes() {
        for case in ["localhost", "ipv4", "remote-denied", "remote-allowed"] {
            let root = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "mcp::http::transport_tests::private_bind_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("HTTP_BIND_CASE", case)
                .env("HOME", root.path())
                .env("USERPROFILE", root.path())
                .env("SYMERASEME_DATA_DIR", root.path().join("data"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
    }

    #[test]
    #[ignore = "only launched by its parent with a disposable HOME and data directory"]
    fn private_bind_child() {
        let case = std::env::var("HTTP_BIND_CASE").unwrap();
        let (host, allowed) = match case.as_str() {
            "localhost" => ("localhost", false),
            "ipv4" => ("127.0.0.1", false),
            "remote-denied" => ("203.0.113.7", false),
            "remote-allowed" => ("203.0.113.7", true),
            _ => panic!("unknown private case"),
        };
        // The callback stops before bind; no remote network listener is opened.
        let error = serve(host.to_owned(), 8080, allowed, || {
            Err("synthetic-handler-stop".to_owned())
        })
        .unwrap_err();
        let path = std::path::PathBuf::from(std::env::var_os("SYMERASEME_DATA_DIR").unwrap())
            .join("mcp_token");
        if case == "remote-denied" {
            assert_eq!(
                error,
                "refusing non-loopback MCP bind \"203.0.113.7\" without --allow-remote"
            );
            assert!(!path.exists());
        } else {
            assert_eq!(error, "synthetic-handler-stop");
            let token = fs::read_to_string(path).unwrap();
            assert_eq!(token.len(), 43);
            assert_eq!(
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(token)
                    .unwrap()
                    .len(),
                32
            );
        }
    }

    /// IPv4-mapped IPv6 loopback addresses are loopback binds.
    #[test]
    fn is_loopback_host_accepts_mapped_ipv6_loopback() {
        assert!(is_loopback_host("::ffff:127.0.0.1"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(!is_loopback_host("fe80::1"));
        assert!(!is_loopback_host("203.0.113.7"));
    }

    /// Authorization demands exactly one `Bearer ` header whose length and
    /// bytes match the token; every other shape is rejected.
    #[test]
    fn authorized_requires_one_well_formed_bearer_token() {
        let token = "gzT0j9pExampleTokenLongEnoughToCompare43abc";
        let mut headers = http::HeaderMap::new();

        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Basic Zm9vOmJhcg"),
        );
        assert!(!authorized(&headers, token), "non-Bearer scheme");

        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer "),
        );
        assert!(!authorized(&headers, token), "empty bearer value");

        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer short"),
        );
        assert!(!authorized(&headers, token), "length mismatch");

        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Bearer {token}")).expect("header"),
        );
        assert!(authorized(&headers, token), "exact token");

        // Equal length must still compare every byte, including the ends.
        for index in 0..token.len() {
            let mut wrong = token.as_bytes().to_vec();
            wrong[index] = if wrong[index] == b'x' { b'y' } else { b'x' };
            headers.insert(
                http::header::AUTHORIZATION,
                http::HeaderValue::from_str(&format!(
                    "Bearer {}",
                    String::from_utf8(wrong).unwrap()
                ))
                .unwrap(),
            );
            assert!(!authorized(&headers, token), "wrong bearer byte {index}");
        }
    }

    fn reply_text(reply: &HttpReply) -> String {
        String::from_utf8(reply.body.clone()).expect("reply is UTF-8")
    }

    fn request(id: i32) -> String {
        format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"initialize"}}"#)
    }

    /// A batch joins the per-item responses into one array with the trailing
    /// newlines stripped.
    #[test]
    fn protocol_reply_joins_batched_responses() {
        let handler = no_backend_handler();
        let body = format!("[{}, {}, {}]", request(1), request(2), request(3));
        let reply = protocol_reply(body.as_bytes(), &handler);
        assert_eq!(reply.status, 200);
        let text = reply_text(&reply);
        assert!(text.starts_with('['), "{text}");
        assert!(text.ends_with("]\n"), "{text}");
        assert_eq!(text.matches("\"id\":1").count(), 1, "{text}");
        assert_eq!(text.matches("\"id\":2").count(), 1, "{text}");
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 3);
        for (index, item) in parsed.as_array().unwrap().iter().enumerate() {
            assert_eq!(item["id"], index + 1);
        }
        assert!(
            !text.contains("}\n,"),
            "item newlines must be stripped: {text}"
        );
    }

    /// A batch whose items are all notifications answers 204 with no body.
    #[test]
    fn protocol_reply_answers_204_when_no_batch_item_wants_a_response() {
        let handler = no_backend_handler();
        let reply = protocol_reply(br#"[{"jsonrpc":"2.0","method":"initialize"}]"#, &handler);
        assert_eq!(reply.status, 204);
        assert!(reply.body.is_empty());
    }

    /// Empty batches are invalid requests; unskippable items abort with a
    /// parse error; items without a separating comma stop the loop after the
    /// first response.
    #[test]
    fn protocol_reply_rejects_empty_and_malformed_batches() {
        let handler = no_backend_handler();

        let empty = protocol_reply(b"[]", &handler);
        assert_eq!(empty.status, 200);
        assert!(reply_text(&empty).contains("-32600"), "empty batch");

        let malformed = format!("[{}, truX]", request(1));
        let reply = protocol_reply(malformed.as_bytes(), &handler);
        assert_eq!(reply.status, 200);
        assert!(reply_text(&reply).contains("-32700"), "unskippable item");

        let unseparated = format!("[{} {}]", request(1), request(2));
        let joined = protocol_reply(unseparated.as_bytes(), &handler);
        assert_eq!(joined.status, 200);
        let text = reply_text(&joined);
        // A missing separator aborts the whole batch — neither item answers.
        assert!(text.contains("-32700"), "batch parse error: {text}");
        assert!(
            !text.contains("\"result\""),
            "no item is dispatched: {text}"
        );
    }

    /// The Go-compatible scanner frames 200 nested arrays, but serde_json's
    /// recursion limit rejects the catalogue dispatch — that divergence must
    /// surface as a JSON-RPC parse error, not a dropped request.
    #[test]
    fn protocol_reply_reports_a_parse_error_for_frames_serde_rejects() {
        let deep = format!("{}1{}", "[".repeat(200), "]".repeat(200));
        let single = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{"deep":{deep}}}}}"#
        );
        let handler = no_backend_handler();
        let reply = protocol_reply(single.as_bytes(), &handler);
        assert_eq!(reply.status, 200);
        let text = reply_text(&reply);
        assert!(text.contains("-32700"), "{text}");
        assert!(text.contains("parse error"), "{text}");
    }

    struct BlockingCancellationHandler {
        started: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        finished: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>,
    }

    impl ToolHandler for BlockingCancellationHandler {
        fn call(
            &self,
            _: &str,
            _: &serde_json::Map<String, serde_json::Value>,
        ) -> Result<serde_json::Value, super::super::handler::ToolError> {
            unreachable!("cancellable dispatch is expected")
        }

        fn call_cancellable(
            &self,
            _: &str,
            _: &serde_json::Map<String, serde_json::Value>,
            cancellation: &CancellationToken,
        ) -> Result<serde_json::Value, super::super::handler::ToolError> {
            if let Some(started) = self.started.lock().unwrap().take() {
                let _ = started.send(());
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while !cancellation.is_cancelled() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            let message = if cancellation.is_cancelled() {
                "context canceled"
            } else {
                "cancellation timed out"
            };
            if let Some(finished) = self.finished.lock().unwrap().take() {
                let _ = finished.send(message.to_owned());
            }
            Err(super::super::handler::ToolError(message.to_owned()))
        }
    }

    #[test]
    fn dropping_http_client_cancels_the_inflight_tool_handler() {
        let go: serde_json::Value =
            serde_json::from_str(GO_AGENT_CANCEL_FIXTURE).expect("Go cancellation oracle parses");
        assert_eq!(go["schema"], "symeraseme.go-oracle.agent-cancel.v1");
        assert_eq!(go["go_version"], "go1.26.6");
        assert_eq!(go["handler_error"], "context canceled");
        assert_eq!(go["child_exited"], true);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind local test server");
            let address = listener.local_addr().unwrap();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
            let handler: Arc<dyn ToolHandler> = Arc::new(BlockingCancellationHandler {
                started: std::sync::Mutex::new(Some(started_tx)),
                finished: std::sync::Mutex::new(Some(finished_tx)),
            });
            let server_handler = Arc::clone(&handler);
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept request");
                let service = service_fn(move |request| {
                    let handler = Arc::clone(&server_handler);
                    async move {
                        Ok::<_, std::convert::Infallible>(
                            handle_request(request, "test_token", handler).await,
                        )
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });

            let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"classify_reply","arguments":{"request_id":1}}}"#;
            let mut client = std::net::TcpStream::connect(address).expect("connect test client");
            let request = format!(
                "POST / HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer test_token\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            client
                .write_all(request.as_bytes())
                .expect("send MCP tool request");
            tokio::time::timeout(Duration::from_secs(2), started_rx)
                .await
                .expect("handler starts promptly")
                .expect("handler start signal");
            drop(client);
            let message = tokio::time::timeout(Duration::from_secs(2), finished_rx)
                .await
                .expect("handler observes the HTTP disconnect")
                .expect("handler result signal");
            assert_eq!(message, "context canceled");
            tokio::time::timeout(Duration::from_secs(2), server)
                .await
                .expect("server connection exits")
                .expect("server task");
        });
    }

    #[test]
    fn completed_requests_disarm_cancellation_without_hiding_real_disconnects() {
        let complete = CancellationToken::default();
        let mut guard = CancelOnDrop::new(complete.clone());
        guard.disarm();
        drop(guard);
        assert!(!complete.is_cancelled());
        let abandoned = CancellationToken::default();
        drop(CancelOnDrop::new(abandoned.clone()));
        assert!(abandoned.is_cancelled());
    }

    fn actual_http_reply(packet: Vec<u8>) -> Vec<u8> {
        use std::io::{Read, Write};
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let service = service_fn(|request| async move {
                    Ok::<_, std::convert::Infallible>(
                        handle_request(
                            request,
                            "synthetic-auth-token",
                            Arc::new(no_backend_handler()),
                        )
                        .await,
                    )
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
            let client = tokio::task::spawn_blocking(move || {
                let mut stream = std::net::TcpStream::connect(address).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                if let Err(error) = stream.write_all(&packet) {
                    assert!(matches!(
                        error.kind(),
                        std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                    ));
                }
                let mut response = Vec::new();
                stream.read_to_end(&mut response).unwrap();
                response
            });
            let response = tokio::time::timeout(Duration::from_secs(4), client)
                .await
                .unwrap()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(4), server)
                .await
                .unwrap()
                .unwrap();
            response
        })
    }

    fn packet(method: &str, extra: &str, body: &[u8], declared: usize) -> Vec<u8> {
        let mut bytes = format!("{method} / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {declared}\r\n{extra}\r\n").into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    fn expect_http(response: &[u8], status: u16, code: Option<i64>) {
        let split = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap();
        let headers = std::str::from_utf8(&response[..split]).unwrap();
        assert!(
            headers.starts_with(&format!("HTTP/1.1 {status} ")),
            "{headers}"
        );
        let body = &response[split + 4..];
        if let Some(code) = code {
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("content-type: application/json")
            );
            let value: serde_json::Value = serde_json::from_slice(body).unwrap();
            assert_eq!(value["jsonrpc"], "2.0");
            assert_eq!(value["error"]["code"], code);
        } else {
            assert_eq!(status, 204);
            assert!(body.is_empty());
            assert!(!headers.to_ascii_lowercase().contains("content-type:"));
        }
    }

    #[test]
    fn actual_http_wire_rejects_unsafe_auth_origin_methods_and_frames() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"unknown"}"#;
        let auth = "Authorization: Bearer synthetic-auth-token\r\n";
        let cases = [
            ("GET", auth.to_owned(), body.as_slice(), 405, -32600),
            ("POST", String::new(), body.as_slice(), 401, -32000),
            (
                "POST",
                "Authorization: Bearer synthetic-auth-tokex\r\n".to_owned(),
                body.as_slice(),
                401,
                -32000,
            ),
            (
                "POST",
                format!("{auth}Origin: https://example.invalid\r\n"),
                body.as_slice(),
                403,
                -32000,
            ),
            ("POST", auth.to_owned(), b"{".as_slice(), 200, -32700),
            ("POST", auth.to_owned(), body.as_slice(), 200, -32601),
        ];
        for (method, headers, body, status, code) in cases {
            expect_http(
                &actual_http_reply(packet(method, &headers, body, body.len())),
                status,
                Some(code),
            );
        }
        let notification = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        expect_http(
            &actual_http_reply(packet("POST", auth, notification, notification.len())),
            204,
            None,
        );
    }

    #[test]
    fn actual_http_body_limit_accepts_exactly_five_mib_and_rejects_the_next_byte() {
        // Independent Go contract boundary, never derived from the mutable
        // implementation's MAX_BODY_BYTES constant.
        const CONTRACT_LIMIT: usize = 5_242_880;
        let auth = "Authorization: Bearer synthetic-auth-token\r\n";
        let mut exact = Vec::from(b"{}".as_slice());
        exact.resize(CONTRACT_LIMIT, b' ');
        expect_http(
            &actual_http_reply(packet("POST", auth, &exact, CONTRACT_LIMIT)),
            200,
            Some(-32600),
        );
        expect_http(
            &actual_http_reply(packet("POST", auth, &[], CONTRACT_LIMIT + 1)),
            413,
            Some(-32600),
        );
        exact.push(b' ');
        let mut chunked = format!("POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n{auth}\r\n{:x}\r\n", exact.len()).into_bytes();
        chunked.extend_from_slice(&exact);
        chunked.extend_from_slice(b"\r\n0\r\n\r\n");
        // Go's chunked over-limit contract is HTTP 200 / JSON-RPC parse error;
        // only an oversized declared Content-Length produces HTTP 413.
        expect_http(&actual_http_reply(chunked), 200, Some(-32700));
    }
}
