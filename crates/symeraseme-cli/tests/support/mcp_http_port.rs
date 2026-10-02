use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

const MAX_START_ATTEMPTS: usize = 3;

pub struct StartedChild {
    child: Child,
    stderr_file: Option<NamedTempFile>,
}

impl StartedChild {
    #[allow(dead_code)]
    pub fn from_child(child: Child) -> Self {
        Self {
            child,
            stderr_file: None,
        }
    }

    pub fn stderr_text(&mut self) -> String {
        if let Some(stderr_file) = &mut self.stderr_file {
            let file = stderr_file.as_file_mut();
            let _ = file.seek(SeekFrom::Start(0));
            let mut stderr = String::new();
            let _ = file.read_to_string(&mut stderr);
            stderr
        } else if let Some(mut stderr) = self.child.stderr.take() {
            let mut output = String::new();
            let _ = stderr.read_to_string(&mut output);
            output
        } else {
            String::new()
        }
    }
}

impl Deref for StartedChild {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl DerefMut for StartedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

impl Drop for StartedChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

pub fn free_port() -> u16 {
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

/// Start a real HTTP child, retrying only when its first bind fails because a
/// released candidate was claimed during the parent/child handoff. Readiness
/// is accepted only after the attempt has rotated the token file and the
/// current token authenticates successfully. Every attempt shares one deadline.
pub fn spawn_with_handoff<F>(
    port: &mut u16,
    token_path: &Path,
    timeout: Duration,
    mut spawn: F,
) -> StartedChild
where
    F: FnMut(u16, Stdio) -> std::io::Result<Child>,
{
    let deadline = Instant::now() + timeout;

    for attempt in 1..=MAX_START_ATTEMPTS {
        let token_before_attempt = fs::read_to_string(token_path).ok();
        let captured_stderr = NamedTempFile::new().expect("create startup stderr capture");
        let stderr = captured_stderr
            .as_file()
            .try_clone()
            .expect("clone startup stderr capture");
        let child = spawn(*port, Stdio::from(stderr))
            .unwrap_or_else(|error| panic!("spawn MCP HTTP process on port {}: {error}", *port));
        let mut child = StartedChild {
            child,
            stderr_file: Some(captured_stderr),
        };

        loop {
            if let Some(status) = child.try_wait().expect("check MCP HTTP process") {
                let stderr = child.stderr_text();
                if is_address_in_use(&stderr, *port)
                    && attempt < MAX_START_ATTEMPTS
                    && Instant::now() < deadline
                {
                    child.wait().expect("reap collided MCP HTTP process");
                    drop(child);
                    *port = free_port();
                    break;
                }
                panic!("MCP HTTP process exited early ({status}): {stderr}");
            }

            if Instant::now() >= deadline {
                let _ = child.kill();
                let status = child.wait().expect("reap MCP HTTP process at deadline");
                let stderr = child.stderr_text();
                panic!("MCP HTTP startup deadline expired ({status}): {stderr}");
            }

            if let Ok(token) = fs::read_to_string(token_path)
                && token_before_attempt.as_deref() != Some(token.as_str())
                && accepts_token(*port, &token, deadline)
                && Instant::now() < deadline
                && child
                    .try_wait()
                    .expect("check authenticated MCP child")
                    .is_none()
            {
                return child;
            }

            thread::sleep(Duration::from_millis(20));
        }
    }

    unreachable!("the bounded startup loop either returns or panics")
}

fn is_address_in_use(stderr: &str, port: u16) -> bool {
    stderr.lines().any(|line| {
        line.starts_with("listen tcp ")
            && line.contains(&format!(":{port}: bind: "))
            && (line.contains(": bind: address already in use")
                || line.contains(": bind: Only one usage of each socket address"))
    })
}

/// Bound the complete connect/write/read probe, not just each individual read.
pub fn accepts_token(port: u16, token: &str, deadline: Instant) -> bool {
    let deadline = deadline.min(Instant::now() + Duration::from_millis(500));
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return false;
    }
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, remaining) else {
        return false;
    };
    let body = br#"{"jsonrpc":"2.0","id":0,"method":"initialize"}"#;
    let mut request =
        b"POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAuthorization: ".to_vec();
    request.extend_from_slice(b"Bearer ");
    request.extend_from_slice(token.as_bytes());
    request.extend_from_slice(format!("\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes());
    request.extend_from_slice(body);
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero()
        || stream.set_write_timeout(Some(remaining)).is_err()
        || stream.write_all(&request).is_err()
    {
        return false;
    }
    let mut response = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return false;
        }
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) if response.len() + count <= 64 * 1024 => {
                response.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
            Ok(_) | Err(_) => return false,
        }
    }
    let Some(split) = response.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
        return false;
    };
    let Ok(head) = std::str::from_utf8(&response[..split]) else {
        return false;
    };
    let body = &response[split + 4..];
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok());
    head.lines().next() == Some("HTTP/1.1 200 OK")
        && length == Some(body.len())
        && serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .is_some_and(|body| body["result"]["serverInfo"]["name"] == "symeraseme")
}
