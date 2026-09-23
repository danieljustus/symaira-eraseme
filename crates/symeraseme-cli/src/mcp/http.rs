//! MCP HTTP transport and process lifecycle.

use super::handler::ToolHandler;
use super::protocol::{InitializeOutcome, initialize, skip_json_value, skip_whitespace};
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
        .map_err(|error| error.to_string())?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let connection_slots = Arc::new(Semaphore::new(MAX_HTTP_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
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
                        async move { Ok::<_, std::convert::Infallible>(handle_request(request, &token, handler.as_ref()).await) }
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
    handler: &dyn ToolHandler,
) -> Response<Full<Bytes>> {
    let declared_too_large = request
        .headers()
        .get(http::header::CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok())
        .and_then(|length| length.parse::<usize>().ok())
        .is_some_and(|length| length > MAX_BODY_BYTES);
    let reply = if request.method() != Method::POST {
        rpc_reply(405, -32600, "POST required")
    } else if request
        .headers()
        .get(http::header::ORIGIN)
        .and_then(|origin| origin.to_str().ok())
        .is_some_and(|origin| !allowed_origin(origin))
    {
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
                protocol_reply(&body, handler)
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

fn protocol_reply(body: &[u8], handler: &dyn ToolHandler) -> HttpReply {
    let (start, end) = match one_json_value(body) {
        Some(span) => span,
        None => return rpc_reply(200, -32700, "parse error"),
    };
    if body[start] != b'[' {
        return protocol_outcome(initialize(&body[start..end], handler));
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
        if let InitializeOutcome::Response(mut bytes) = initialize(&body[index..item_end], handler)
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
