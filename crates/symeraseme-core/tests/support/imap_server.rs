//! Scripted IMAP server that mirrors Go's fake_server_test.go.
//!
//! This server implements exactly the protocol expected by the parity tests:
//! - READ-ONLY EXAMINE (not SELECT) for all folder selections
//! - CHARSET UTF-8 in UID SEARCH
//! - BODY.PEEK[HEADER] and BODY.PEEK[TEXT] in UID FETCH
//! - XOAUTH2 SASL-IR authentication
//! - Proper error handling for cleartext auth refusal, invalid credentials,
//!   oversized sections, invalid UID ranges, etc.
//!
//! The server runs on 127.0.0.1 and is used by the parity tests to compare
//! the Rust transport implementation with the Go reference.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// A transport that can be swapped in place, so the reader (created before the
/// handshake) and the writer keep working on the upgraded stream. rustls'
/// session object cannot be cloned, which is the same constraint the product
/// client solves with its own wrapper.
pub trait ReadWrite: Read + Write + Send + Sync {}
impl<T: Read + Write + Send + Sync> ReadWrite for T {}

pub struct SharedStream {
    inner: Arc<Mutex<Box<dyn ReadWrite>>>,
}

impl SharedStream {
    fn new(stream: Box<dyn ReadWrite>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(stream)),
        }
    }

    /// Shares the same underlying transport; used to keep the reader and the
    /// writer on one connection.
    fn share(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }

    fn replace_with(
        &self,
        swap: impl FnOnce(Box<dyn ReadWrite>) -> Result<Box<dyn ReadWrite>, String>,
    ) -> Result<(), String> {
        let mut guard = self.inner.lock().expect("stream lock");
        let current = std::mem::replace(&mut *guard, Box::new(std::io::empty()));
        *guard = swap(current)?;
        Ok(())
    }
}

impl Clone for SharedStream {
    fn clone(&self) -> Self {
        self.share()
    }
}

impl Read for SharedStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.inner.lock().expect("stream lock").read(buffer)
    }
}

impl Write for SharedStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.inner.lock().expect("stream lock").write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.lock().expect("stream lock").flush()
    }
}

/// How the scripted server secures the connection: not at all, after the
/// STARTTLS command, or from the first byte (implicit TLS).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    None,
    StartTls,
    Implicit,
}

fn upgrade_tls(stream: &SharedStream, config: &Arc<ServerConfig>) -> Result<(), String> {
    let connection =
        ServerConnection::new(config.clone()).map_err(|e| format!("server TLS: {e}"))?;
    stream.replace_with(|inner| Ok(Box::new(StreamOwned::new(connection, inner))))
}

/// The TLS material a scripted server was started with; bundled so the
/// connection handler keeps a manageable argument list.
#[derive(Clone)]
pub struct TlsSetup {
    config: Option<Arc<ServerConfig>>,
    mode: TlsMode,
}

pub struct ScriptedImapServer {
    pub port: u16,
    pub folders: Arc<Mutex<HashMap<String, FolderState>>>,
    pub require_user: String,
    pub require_pass: String,
    pub require_token: String,
    pub transcript: Arc<Mutex<Vec<String>>>,
}

impl ScriptedImapServer {
    /// Creates a new scripted IMAP server for parity testing.
    /// Mirrors Go's startFakeIMAPServerMode(false, false).
    pub fn new() -> Result<Self, String> {
        Self::start(None, TlsMode::None)
    }

    /// Serves over TLS, either implicitly or after `STARTTLS`.
    pub fn new_tls(tls: Arc<ServerConfig>, mode: TlsMode) -> Result<Self, String> {
        Self::start(Some(tls), mode)
    }

    fn start(tls: Option<Arc<ServerConfig>>, tls_mode: TlsMode) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("failed to bind server: {}", e))?;
        let addr = listener
            .local_addr()
            .map_err(|e| format!("failed to get local addr: {}", e))?;

        let server = Self {
            port: addr.port(),
            folders: Arc::new(Mutex::new(HashMap::new())),
            require_user: "testuser".to_string(),
            require_pass: "testpass".to_string(),
            require_token: "testtoken".to_string(),
            transcript: Arc::new(Mutex::new(Vec::new())),
        };
        let tls = TlsSetup {
            config: tls,
            mode: tls_mode,
        };

        // Default INBOX folder
        {
            let folders = server.folders.clone();
            let mut guard = folders.lock().unwrap();
            guard.insert(
                "INBOX".to_string(),
                FolderState {
                    uid_validity: 100,
                    messages: Vec::new(),
                },
            );
        }

        let folders_clone = server.folders.clone();
        let require_user = server.require_user.clone();
        let require_pass = server.require_pass.clone();
        let require_token = server.require_token.clone();
        let transcript_clone = server.transcript.clone();

        // Start serving in background
        thread::spawn(move || {
            serve_loop(
                listener,
                folders_clone,
                require_user,
                require_pass,
                require_token,
                transcript_clone,
                tls,
            );
        });

        thread::sleep(Duration::from_millis(100));
        Ok(server)
    }

    /// Returns the transcript of all commands received.
    pub fn get_transcript(&self) -> Vec<String> {
        self.transcript.lock().unwrap().clone()
    }
}

fn serve_loop(
    listener: TcpListener,
    folders: Arc<Mutex<HashMap<String, FolderState>>>,
    require_user: String,
    require_pass: String,
    require_token: String,
    transcript: Arc<Mutex<Vec<String>>>,
    tls: TlsSetup,
) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let folders = folders.clone();
                let require_user = require_user.clone();
                let require_pass = require_pass.clone();
                let require_token = require_token.clone();
                let transcript = transcript.clone();
                let tls = tls.clone();
                thread::spawn(move || {
                    handle_connection(
                        stream,
                        folders,
                        &require_user,
                        &require_pass,
                        &require_token,
                        transcript,
                        tls,
                    );
                });
            }
            Err(_) => continue,
        }
    }
}

fn handle_connection(
    tcp: TcpStream,
    folders: Arc<Mutex<HashMap<String, FolderState>>>,
    require_user: &str,
    require_pass: &str,
    require_token: &str,
    transcript: Arc<Mutex<Vec<String>>>,
    tls: TlsSetup,
) {
    let shared = SharedStream::new(Box::new(tcp));
    if let (Some(config), TlsMode::Implicit) = (&tls.config, tls.mode) {
        upgrade_tls(&shared, config).expect("implicit TLS handshake");
    }
    let reader = BufReader::new(shared.clone());
    let mut lines = reader.lines();
    let mut stream = shared;
    let mut selected_folder: Option<String> = None;

    // Send greeting
    let _ = stream.write_all(b"* OK IMAP4rev1 Service Ready\r\n");

    while let Some(Ok(line)) = lines.next() {
        // Record the raw line in the transcript (like Go fake server)
        transcript.lock().unwrap().push(line.clone());

        let line = line.trim_end_matches('\r').trim_end_matches('\n');
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let tag = parts[0];
        let cmd = parts[1].to_uppercase();
        let rest = if parts.len() > 2 {
            parts[2..].join(" ")
        } else {
            String::new()
        };

        match cmd.as_str() {
            "CAPABILITY" => {
                let starttls_capability = if tls.mode == TlsMode::StartTls {
                    " STARTTLS"
                } else {
                    ""
                };
                let response = format!(
                    "* CAPABILITY IMAP4rev1{} AUTH=PLAIN AUTH=XOAUTH2 SASL-IR\r\n{} OK CAPABILITY completed\r\n",
                    starttls_capability, tag
                );
                let _ = stream.write_all(response.as_bytes());
            }
            "STARTTLS" => match (&tls.config, tls.mode) {
                (Some(config), TlsMode::StartTls) => {
                    let _ = stream
                        .write_all(format!("{} OK Begin TLS negotiation now\r\n", tag).as_bytes());
                    let _ = stream.flush();
                    upgrade_tls(&stream, config).expect("STARTTLS handshake");
                }
                _ => {
                    let _ = stream
                        .write_all(format!("{} BAD STARTTLS unavailable\r\n", tag).as_bytes());
                }
            },
            "LOGIN" => {
                let (username, password) = parse_login(&rest);
                if username != require_user || password != require_pass {
                    // After failed login, Go client sends LOGOUT during cleanup.
                    // That line belongs in the transcript like every other one.
                    let _ = stream.write_all(
                        format!("{} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n", tag)
                            .as_bytes(),
                    );
                    if let Some(Ok(line)) = lines.next() {
                        transcript.lock().unwrap().push(line);
                    }
                    let _ = stream.write_all(format!("{} OK LOGOUT completed\r\n", tag).as_bytes());
                    break;
                }
                let _ = stream.write_all(
                    format!("{} OK [CAPABILITY IMAP4rev1] Logged in\r\n", tag).as_bytes(),
                );
            }
            "AUTHENTICATE" => {
                if parts.len() < 3 {
                    let _ = stream
                        .write_all(format!("{} BAD Missing auth mechanism\r\n", tag).as_bytes());
                    continue;
                }
                let mechanism = parts[2];
                match mechanism {
                    "XOAUTH2" => {
                        handle_xoauth2(&mut stream, &mut lines, tag, &rest, require_token);
                    }
                    _ => {
                        let _ =
                            stream.write_all(format!("{} NO Unsupported auth\r\n", tag).as_bytes());
                    }
                }
            }
            "EXAMINE" | "SELECT" => {
                let folder_name = rest.trim_matches('"');
                let mut guard = folders.lock().unwrap();
                let folder = guard
                    .entry(folder_name.to_string())
                    .or_insert_with(|| FolderState {
                        uid_validity: if folder_name == "INBOX" { 100 } else { 7 },
                        messages: Vec::new(),
                    });
                selected_folder = Some(folder_name.to_string());
                let msg_count = folder.messages.len();
                let uid_validity = folder.uid_validity;
                drop(guard);

                let response = format!(
                    "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n* {} EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY {}] UIDs valid\r\n{} OK [READ-ONLY] Select completed\r\n",
                    msg_count, uid_validity, tag
                );
                let _ = stream.write_all(response.as_bytes());
            }
            "UID" => {
                let uid_parts: Vec<&str> = rest.split_whitespace().collect();
                if uid_parts.is_empty() {
                    continue;
                }
                let uid_cmd = uid_parts[0].to_uppercase();
                match uid_cmd.as_str() {
                    "SEARCH" => {
                        let uid_range = if uid_parts.len() > 1 {
                            uid_parts[1..].join(" ")
                        } else {
                            String::new()
                        };
                        let folder_name = selected_folder.as_deref().unwrap_or("INBOX");
                        let guard = folders.lock().unwrap();
                        let folder = guard.get(folder_name);
                        let start_uid = parse_uid_range_start(&uid_range);
                        if let Some(folder) = folder {
                            let matching: Vec<u32> = folder
                                .messages
                                .iter()
                                .filter(|m| m.uid >= start_uid)
                                .map(|m| m.uid)
                                .collect();
                            if matching.is_empty() {
                                let _ = stream.write_all(b"* SEARCH\r\n");
                            } else {
                                let uids_str = matching
                                    .iter()
                                    .map(|u| u.to_string())
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                let _ = stream
                                    .write_all(format!("* SEARCH {}\r\n", uids_str).as_bytes());
                            }
                        }
                        drop(guard);
                        let _ = stream
                            .write_all(format!("{} OK UID SEARCH completed\r\n", tag).as_bytes());
                    }
                    "FETCH" => {
                        // Only the sequence set: the parenthesised item list that
                        // follows it is not part of the UID argument.
                        let fetch_args = uid_parts.get(1).copied().unwrap_or("").to_string();
                        let folder_name = selected_folder.as_deref().unwrap_or("INBOX");
                        let guard = folders.lock().unwrap();
                        if let Some(folder) = guard.get(folder_name) {
                            perform_fetch(&mut stream, tag, folder, &fetch_args);
                        } else {
                            let _ = stream
                                .write_all(format!("{} NO No folder selected\r\n", tag).as_bytes());
                        }
                        drop(guard);
                    }
                    _ => {
                        let _ = stream.write_all(
                            format!("{} BAD Unsupported UID command\r\n", tag).as_bytes(),
                        );
                    }
                }
            }
            "LOGOUT" => {
                let _ = stream.write_all(b"* BYE IMAP4rev1 Server logging out\r\n");
                let _ = stream.write_all(format!("{} OK LOGOUT completed\r\n", tag).as_bytes());
                break;
            }
            _ => {
                let _ =
                    stream.write_all(format!("{} BAD Command unrecognized\r\n", tag).as_bytes());
            }
        }
    }
}

fn handle_xoauth2(
    stream: &mut SharedStream,
    _lines: &mut std::io::Lines<BufReader<SharedStream>>,
    tag: &str,
    rest: &str,
    require_token: &str,
) {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let mut auth_payload = String::new();

    if parts.len() > 1 {
        auth_payload = parts[1].to_string();
    } else {
        let _ = stream.write_all(b"+\r\n");
        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf) {
            auth_payload = String::from_utf8_lossy(&buf[..n]).trim().to_string();
        }
    }

    let mut token_valid = false;
    if !auth_payload.is_empty()
        && let Ok(decoded) = STANDARD.decode(&auth_payload)
    {
        let payload_str = String::from_utf8_lossy(&decoded);
        token_valid = payload_str.contains(require_token);
    }

    if token_valid {
        let _ = stream.write_all(format!("{} OK Authenticated\r\n", tag).as_bytes());
    } else {
        let _ = stream
            .write_all(format!("{} NO [AUTHENTICATIONFAILED] Invalid token\r\n", tag).as_bytes());
    }
}

fn perform_fetch(stream: &mut SharedStream, tag: &str, folder: &FolderState, fetch_args: &str) {
    let uids = parse_fetch_uids(fetch_args);
    if uids.is_empty() {
        let _ = stream.write_all(format!("{} BAD Invalid FETCH syntax\r\n", tag).as_bytes());
        return;
    }

    let mut seq = 1;
    for msg in &folder.messages {
        if !uids.contains(&msg.uid) {
            seq += 1;
            continue;
        }
        write_fetch_response(stream, seq, msg);
        seq += 1;
    }
    let _ = stream.write_all(format!("{} OK UID FETCH completed\r\n", tag).as_bytes());
}

fn write_fetch_response(stream: &mut SharedStream, seq: u32, msg: &MessageState) {
    let header_bytes = msg.header.as_bytes();
    let body_bytes = msg.body.as_bytes();
    let flags_str = if !msg.flags.is_empty() {
        msg.flags.join(" ")
    } else {
        "\\Seen".to_string()
    };

    // A literal is announced by {n} at the end of a line; the payload follows
    // after that CRLF and the rest of the response continues behind it.
    let mut response = Vec::new();
    response.extend_from_slice(
        format!(
            "* {} FETCH (UID {} FLAGS ({}) BODY[HEADER] {{{}}}\r\n",
            seq,
            msg.uid,
            flags_str,
            header_bytes.len()
        )
        .as_bytes(),
    );
    response.extend_from_slice(header_bytes);
    response.extend_from_slice(format!(" BODY[TEXT] {{{}}}\r\n", body_bytes.len()).as_bytes());
    response.extend_from_slice(body_bytes);
    response.extend_from_slice(b")\r\n");
    let _ = stream.write_all(&response);
}

fn parse_login(rest: &str) -> (String, String) {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let username = parts
        .first()
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_default();
    let password = parts
        .get(1)
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_default();
    (username, password)
}

fn parse_uid_range_start(range: &str) -> u32 {
    if range.is_empty() {
        return 1;
    }
    for part in range.split(',') {
        let part = part.trim();
        if let Some(colon) = part.find(':') {
            let start = &part[..colon];
            if let Ok(val) = start.parse::<u32>() {
                return val;
            }
        } else if let Ok(val) = part.parse::<u32>() {
            return val;
        }
    }
    1
}

fn parse_fetch_uids(fetch_args: &str) -> Vec<u32> {
    let mut uids = Vec::new();
    for part in fetch_args.split(',') {
        let part = part.trim();
        if part.contains(':') {
            let bounds: Vec<&str> = part.split(':').collect();
            if bounds.len() == 2
                && let (Ok(start), Ok(end)) = (bounds[0].parse::<u32>(), bounds[1].parse::<u32>())
            {
                for uid in start..=end {
                    uids.push(uid);
                }
            }
        } else if let Ok(uid) = part.parse::<u32>() {
            uids.push(uid);
        }
    }
    uids
}

#[derive(Clone, Debug)]
pub struct FolderState {
    pub uid_validity: u32,
    pub messages: Vec<MessageState>,
}

#[derive(Clone, Debug)]
pub struct MessageState {
    pub uid: u32,
    pub flags: Vec<String>,
    pub header: String,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_starts() {
        let server = ScriptedImapServer::new();
        assert!(server.is_ok());
    }
}
