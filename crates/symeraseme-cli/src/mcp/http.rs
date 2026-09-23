//! MCP HTTP transport and process lifecycle.

use super::handler::ToolHandler;
use super::protocol::{InitializeOutcome, initialize, skip_json_value, skip_whitespace};
use base64::Engine;
use rand::Rng;
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tiny_http::{Header, Request, Response, Server, StatusCode};

const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;

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
    let server = Server::http(address).map_err(|error| error.to_string())?;
    let stopping = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stopping))
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stopping))
        .map_err(|error| error.to_string())?;

    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    while !stopping.load(Ordering::Relaxed) {
        if let Some(request) = server
            .recv_timeout(Duration::from_millis(100))
            .map_err(|e| e.to_string())?
        {
            let token = Arc::clone(&token);
            let handler = Arc::clone(&handler);
            workers.push(thread::spawn(move || {
                handle_request(request, &token, handler.as_ref())
            }));
        }
        reap_finished(&mut workers);
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while !workers.is_empty() && Instant::now() < deadline {
        reap_finished(&mut workers);
        thread::sleep(Duration::from_millis(10));
    }
    if workers.is_empty() {
        Ok(())
    } else {
        Err("context deadline exceeded".to_owned())
    }
}

fn reap_finished(workers: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            let _ = worker.join();
        } else {
            index += 1;
        }
    }
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
    fs::write(&path, token.as_bytes())?;
    set_mode(&path)?;
    Ok(Arc::new(token))
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

fn handle_request(mut request: Request, token: &str, handler: &dyn ToolHandler) {
    let reply = if request.method().as_str() != "POST" {
        rpc_reply(405, -32600, "POST required")
    } else if request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Origin"))
        .is_some_and(|header| !allowed_origin(header.value.as_str()))
    {
        rpc_reply(403, -32000, "Forbidden: disallowed Origin")
    } else if !authorized(request.headers(), token) {
        rpc_reply(401, -32000, "Unauthorized")
    } else if request
        .body_length()
        .is_some_and(|length| length > MAX_BODY_BYTES)
    {
        rpc_reply(413, -32600, "Invalid Request")
    } else {
        let mut body = Vec::new();
        let read = request
            .as_reader()
            .take((MAX_BODY_BYTES + 1) as u64)
            .read_to_end(&mut body);
        if read.is_err() || body.len() > MAX_BODY_BYTES {
            rpc_reply(200, -32700, "parse error")
        } else {
            protocol_reply(&body, handler)
        }
    };

    let status = StatusCode(reply.status);
    let mut response = Response::from_data(reply.body).with_status_code(status);
    if reply.status != 204 {
        response = response.with_header(
            Header::from_bytes("Content-Type", "application/json").expect("static header"),
        );
    }
    let _ = request.respond(response);
}

fn authorized(headers: &[Header], token: &str) -> bool {
    let mut values = headers
        .iter()
        .filter(|header| header.field.equiv("Authorization"));
    let Some(header) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let Some(supplied) = header.value.as_str().strip_prefix("Bearer ") else {
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
