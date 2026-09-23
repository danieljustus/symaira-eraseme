//! Real network dialer for plain-TCP and TLS IMAP (the main parity target).
//! Based on Go's `internal/email/dialer.go` but ported to Rust.
//!
//! This matches the exact wire semantics defined in the fixture:
//! - On connect: read greeting, then send '<tag> CAPABILITY'
//! - select is READ ONLY: '<tag> EXAMINE <folder>' (Go passed readOnly=true)
//! - search: '<tag> UID SEARCH CHARSET UTF-8 UID <range>'
//! - fetch: '<tag> UID FETCH <collapsed-uid-set> (UID FLAGS INTERNALDATE BODY.PEEK[HEADER] BODY.PEEK[TEXT])'
//! - login: '<tag> LOGIN "<user>" "<password>"'
//! - XOAUTH2 uses SASL IR: '<tag> AUTHENTICATE XOAUTH2 <base64...>'
//! - close() sends '<tag> LOGOUT' and expects '* BYE ...' + a tagged OK
//!
//! TLS support:
//! - Implicit TLS (use_tls=true): TLS handshake on connect, then IMAP over TLS.
//! - STARTTLS: when the server advertises STARTTLS in CAPABILITY, send
//!   '<tag> STARTTLS', expect its tagged OK, handshake, then continue.
//! - TLS configuration precedence: an injected configuration wins over the
//!   dialer-level default; the default is MinVersion TLS 1.2 with ServerName
//!   = config host. Roots for the product default come from bundled webpki-roots.
//!
//! DOCUMENTED DIVERGENCE: Go uses the *platform* root store; the port uses the
//! bundled webpki roots, so a private/enterprise CA installed in the OS store is
//! trusted by Go and not by the port.

use crate::email::policy::ERR_IMAP;
use crate::email::session::{FetchedMessage, ImapSession};
use crate::email::types::ImapConfig;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use chrono::{DateTime, Utc};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use webpki_roots;

/// Read+Write abstraction shared between plain TCP and TLS sessions.
pub trait ReadWrite: Read + Write + Send + Sync {}
impl<T: Read + Write + Send + Sync> ReadWrite for T {}

/// A single owned stream that can be cloned (shared via Arc<Mutex>) so that
/// the reader (BufReader) and writer both operate on the same underlying
/// connection. Plain TCP and TLS both use this wrapper.
pub struct IoStream {
    inner: Arc<Mutex<Box<dyn ReadWrite>>>,
}

impl IoStream {
    pub fn new(stream: Box<dyn ReadWrite>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(stream)),
        }
    }

    /// Replaces the underlying transport while every existing handle keeps
    /// pointing at it — the STARTTLS upgrade needs exactly that, because the
    /// reader and the writer were created before the handshake.
    pub fn replace_with(
        &self,
        swap: impl FnOnce(Box<dyn ReadWrite>) -> Box<dyn ReadWrite>,
    ) -> Result<(), String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| format!("{}: stream lock poisoned", ERR_IMAP))?;
        let current = std::mem::replace(&mut *guard, Box::new(std::io::empty()));
        *guard = swap(current);
        Ok(())
    }

    /// Shares the same underlying transport; used to keep the reader and the
    /// writer on one connection.
    fn share(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Clone for IoStream {
    fn clone(&self) -> Self {
        self.share()
    }
}

impl Read for IoStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("IoStream lock poisoned"))?;
        guard.read(buf)
    }
}

impl Write for IoStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("IoStream lock poisoned"))?;
        guard.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("IoStream lock poisoned"))?;
        guard.flush()
    }
}

/// Build a rustls ClientConfig with the given root store, MinVersion TLS 1.2,
/// and ServerName set to the host. The injected config wins over the default.
/// Go's `imapTLSConfig` sets `MinVersion: tls.VersionTLS12` and `ServerName`
/// to the configured host; an injected configuration wins over the default.
/// rustls' default protocol versions are exactly "TLS 1.2 or newer", so the
/// minimum-version semantics match. Go uses the platform root store, this port
/// the bundled webpki roots — an enterprise CA installed in the OS store is
/// trusted by Go and not here.
fn make_tls_config(root_store: Option<RootCertStore>) -> Result<ClientConfig, String> {
    let root_store =
        root_store.unwrap_or_else(|| webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect());
    // The provider is named explicitly: relying on rustls' process-level
    // auto-detection makes the build's feature unification load-bearing.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("{}: TLS configuration failed: {}", ERR_IMAP, e))?
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(config)
}

/// Perform a TLS handshake on an already-connected TcpStream, returning a
/// rustls StreamOwned that can be used for both reading and writing.
/// Wraps an established transport in a rustls session. rustls performs the
/// handshake lazily on the first read or write, so the IMAP greeting read that
/// follows is what completes it — mirroring Go, where the session object is
/// handed to the client before the first exchange.
fn tls_handshake<S: Read + Write + Send + Sync + 'static>(
    stream: S,
    config: &ClientConfig,
    host: &str,
) -> Result<rustls::StreamOwned<ClientConnection, S>, String> {
    let server_name = ServerName::try_from(host.to_string()).map_err(|e| {
        format!(
            "{}: connect/login failed: invalid server name '{}': {}",
            ERR_IMAP, host, e
        )
    })?;
    let conn = ClientConnection::new(Arc::new(config.clone()), server_name).map_err(|e| {
        format!(
            "{}: connect/login failed: TLS handshake failed: {}",
            ERR_IMAP, e
        )
    })?;
    Ok(rustls::StreamOwned::new(conn, stream))
}

/// IMAP dialer that creates plain-TCP and TLS connections.
///
/// `roots` mirrors Go's injectable `*tls.Config` on the dialer: when set it
/// replaces the bundled webpki root store. Neither Go's product paths nor this
/// port set it outside tests, and Go's `InsecureSkipVerify` escape hatch has no
/// counterpart here because no product path uses it.
pub struct ImapDialer {
    roots: Option<RootCertStore>,
}

impl ImapDialer {
    pub fn new() -> Self {
        Self { roots: None }
    }

    /// Trusts exactly these roots instead of the bundled store.
    pub fn with_root_certificates(roots: RootCertStore) -> Self {
        Self { roots: Some(roots) }
    }
}

impl Default for ImapDialer {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::email::session::ImapDialer for ImapDialer {
    fn dial(&self, config: &ImapConfig) -> Result<Box<dyn ImapSession>, String> {
        if config.host.is_empty() {
            return Err(format!("{}: host is empty", ERR_IMAP));
        }
        let port = if config.port > 0 {
            config.port as u16
        } else {
            143
        };
        let host = config
            .host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(&config.host);
        let host_port = if host.parse::<std::net::Ipv6Addr>().is_ok() {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let timeout = Duration::from_secs(if config.timeout_seconds > 0 {
            config.timeout_seconds as u64
        } else {
            30
        });

        // ponytail: std DNS lookup is blocking; use a cancellable resolver if DNS timeout bounds become required.
        let addresses = host_port.to_socket_addrs().map_err(|_| {
            format!(
                "{}: connect/login failed: dial tcp: lookup {}: no such host",
                ERR_IMAP, config.host
            )
        })?;
        let stream = connect_addresses(addresses, timeout).map_err(|failure| {
            if let Some((address, error)) = failure {
                format!(
                    "{}: connect/login failed: dial tcp {}: connect: {}",
                    ERR_IMAP,
                    address,
                    go_dial_error(&error)
                )
            } else {
                format!(
                    "{}: connect/login failed: dial tcp {}: connect: no suitable address found",
                    ERR_IMAP, host_port
                )
            }
        })?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("{}: read timeout failed: {}", ERR_IMAP, e))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| format!("{}: write timeout failed: {}", ERR_IMAP, e))?;

        // Try to convert to TLS stream if needed.
        let stream: Box<dyn ReadWrite> = if config.use_tls {
            // Implicit TLS: handshake immediately.
            let tls_config = make_tls_config(self.roots.clone())?;
            let tls_stream = tls_handshake(stream, &tls_config, &config.host)?;
            Box::new(tls_stream)
        } else {
            Box::new(stream)
        };

        let io_stream = IoStream::new(stream);
        let mut session = ImapSessionImpl {
            reader: BufReader::new(io_stream.clone()),
            writer: io_stream,
            tag_counter: 0,
            selected_folder: None,
        };

        // Read greeting
        session.read_greeting()?;

        // Send and parse CAPABILITY
        let caps = session.capability()?;
        let starttls_supported = caps.iter().any(|c| c.as_str() == "STARTTLS");

        if config.use_tls {
            // Already connected over TLS. Verify we can proceed.
            // If server advertised STARTTLS but we connected via implicit TLS,
            // that's fine - we're already encrypted.
        } else if starttls_supported {
            // Perform STARTTLS handshake.
            let tag = session.next_tag();
            let cmd = format!("{} STARTTLS\r\n", tag);
            write_line(&mut session.writer, &cmd)?;
            // Read the tagged response.
            let response = read_line(&mut session.reader)?;
            if !response.starts_with(&format!("{} OK", tag)) {
                return Err(format!(
                    "{}: connect/login failed: STARTTLS failed: {}",
                    ERR_IMAP, response
                ));
            }
            // Upgrade the transport in place: the reader and the writer were
            // created before the handshake and keep working on the new stream.
            // The server sends nothing between its tagged OK and our
            // ClientHello (the client speaks first), so the reader's buffer is
            // empty at this point.
            let tls_config = make_tls_config(self.roots.clone())?;
            let host = config.host.clone();
            let mut handshake_error: Option<String> = None;
            session.writer.replace_with(|inner| {
                match tls_handshake(inner, &tls_config, &host) {
                    Ok(tls_stream) => Box::new(tls_stream),
                    Err(error) => {
                        handshake_error = Some(error);
                        Box::new(std::io::empty())
                    }
                }
            })?;
            if let Some(error) = handshake_error {
                return Err(error);
            }
        } else {
            // No STARTTLS: check cleartext auth permission.
            if !config.allow_insecure_cleartext_auth {
                return Err(format!(
                    "{}: cleartext authentication prohibited: server does not support STARTTLS",
                    ERR_IMAP
                ));
            }
        }

        // Authenticate
        if let Some(oauth2) = &config.oauth2 {
            if !oauth2.access_token.is_empty() {
                let user = if !oauth2.username.is_empty() {
                    oauth2.username.clone()
                } else {
                    config.username.clone()
                };
                session.authenticate_xoauth2(&user, &oauth2.access_token)?;
            }
        } else if !config.password.is_empty() {
            session.login(&config.username, &config.password)?;
        }

        Ok(Box::new(session))
    }
}

/// Go's `net.Dial` uses platform-independent syscall wording in its error.
fn go_dial_error(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused => "connection refused",
        std::io::ErrorKind::ConnectionReset => "connection reset by peer",
        std::io::ErrorKind::ConnectionAborted => "software caused connection abort",
        std::io::ErrorKind::NotConnected => "transport endpoint is not connected",
        std::io::ErrorKind::AddrInUse => "address already in use",
        std::io::ErrorKind::AddrNotAvailable => "cannot assign requested address",
        std::io::ErrorKind::TimedOut => "i/o timeout",
        std::io::ErrorKind::PermissionDenied => "permission denied",
        _ => return error.to_string(),
    }
    .to_owned()
}

/// Attempts resolved addresses in order. A later connection may succeed, but if
/// every address fails Go's dialSerial reports the first address's error.
fn connect_addresses(
    addresses: impl IntoIterator<Item = std::net::SocketAddr>,
    timeout: Duration,
) -> Result<TcpStream, Option<(std::net::SocketAddr, std::io::Error)>> {
    connect_addresses_with(addresses, timeout, |address, timeout| {
        TcpStream::connect_timeout(address, timeout)
    })
}

fn connect_addresses_with<I, F>(
    addresses: I,
    timeout: Duration,
    mut connect: F,
) -> Result<TcpStream, Option<(std::net::SocketAddr, std::io::Error)>>
where
    I: IntoIterator<Item = std::net::SocketAddr>,
    F: FnMut(&std::net::SocketAddr, Duration) -> std::io::Result<TcpStream>,
{
    let mut first_failure = None;
    for address in addresses {
        match connect(&address, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) if first_failure.is_none() => first_failure = Some((address, error)),
            Err(_) => {}
        }
    }
    Err(first_failure)
}

pub struct ImapSessionImpl {
    reader: BufReader<IoStream>,
    writer: IoStream,
    tag_counter: u32,
    selected_folder: Option<String>,
}

/// Reads a line from the stream (CRLF terminated).
/// A mailbox argument the way the Go client library emits it: `INBOX` is sent
/// raw (servers handle a quoted INBOX badly — `imap.FormatMailboxName` special
/// cases it, case-insensitively) and every other name is a quoted string.
///
fn mailbox_argument(folder: &str) -> String {
    if folder.eq_ignore_ascii_case("INBOX") {
        return folder.to_string();
    }
    fn flush_shift(encoded: &mut String, utf16_bytes: &mut Vec<u8>) {
        if !utf16_bytes.is_empty() {
            encoded.push('&');
            encoded.push_str(&STANDARD_NO_PAD.encode(&*utf16_bytes).replace('/', ","));
            encoded.push('-');
            utf16_bytes.clear();
        }
    }

    let mut encoded = String::new();
    let mut utf16_bytes = Vec::new();
    for ch in folder.chars() {
        if (' '..='~').contains(&ch) {
            flush_shift(&mut encoded, &mut utf16_bytes);
            if ch == '&' {
                encoded.push_str("&-");
            } else {
                encoded.push(ch);
            }
        } else {
            for unit in ch.encode_utf16(&mut [0; 2]).iter() {
                utf16_bytes.extend_from_slice(&unit.to_be_bytes());
            }
        }
    }
    flush_shift(&mut encoded, &mut utf16_bytes);
    format!("\"{}\"", encoded.replace('\\', "\\\\").replace('"', "\\\""))
}

fn read_line(reader: &mut BufReader<IoStream>) -> Result<String, String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("IMAP read failed: {}", e))?;
    // Remove trailing CRLF
    while line.ends_with('\r') || line.ends_with('\n') {
        line.pop();
    }
    Ok(line)
}

/// Writes a command to the stream (appends CRLF).
fn write_line(writer: &mut IoStream, line: &str) -> Result<(), String> {
    writer
        .write_all(line.as_bytes())
        .map_err(|e| format!("IMAP write failed: {}", e))?;
    writer
        .flush()
        .map_err(|e| format!("IMAP flush failed: {}", e))?;
    Ok(())
}

impl ImapSession for ImapSessionImpl {
    fn select(&mut self, folder: &str) -> Result<u32, String> {
        let tag = self.next_tag();
        let cmd = format!("{} EXAMINE {}\r\n", tag, mailbox_argument(folder));
        write_line(&mut self.writer, &cmd)?;
        let uid_validity = read_examine_response(&mut self.reader, &tag)?;
        self.selected_folder = Some(folder.to_string());
        Ok(uid_validity)
    }

    fn search_uid(
        &mut self,
        uid_range: &str,
        _since: Option<DateTime<Utc>>,
    ) -> Result<Vec<u32>, String> {
        // Validate the UID range first (matches Go imap.ParseSeqSet)
        validate_uid_range(uid_range)?;
        let tag = self.next_tag();
        let cmd = format!("{} UID SEARCH CHARSET UTF-8 UID {}\r\n", tag, uid_range);
        write_line(&mut self.writer, &cmd)?;
        let uids = read_search_response(&mut self.reader, &tag)?;
        Ok(uids)
    }

    fn fetch(&mut self, uids: &[u32]) -> Result<Vec<FetchedMessage>, String> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let tag = self.next_tag();
        // Collapse UID list into ranges
        let uid_set = collapse_uids(uids);
        let cmd = format!(
            "{} UID FETCH {} (UID FLAGS INTERNALDATE BODY.PEEK[HEADER] BODY.PEEK[TEXT])\r\n",
            tag, uid_set
        );
        write_line(&mut self.writer, &cmd)?;
        let fetched = read_fetch_response(&mut self.reader, &tag)?;
        Ok(fetched)
    }

    fn close(&mut self) {
        let tag = self.next_tag();
        let _ = write_line(&mut self.writer, &format!("{} LOGOUT\r\n", tag));
        // Drain server response (* BYE + tagged OK)
        let mut buf = Vec::new();
        let _ = self.reader.read_until(b'\n', &mut buf);
        let _ = self.reader.read_until(b'\n', &mut buf);
    }
}

impl ImapSessionImpl {
    /// Best-effort LOGOUT for a connection that will not be used; the response
    /// is drained so the server's completion line does not linger.
    fn logout_after_failure(&mut self) {
        let tag = self.next_tag();
        let cmd = format!("{} LOGOUT\r\n", tag);
        if write_line(&mut self.writer, &cmd).is_err() {
            return;
        }
        for _ in 0..4 {
            match read_line(&mut self.reader) {
                Ok(line) if line.starts_with(&format!("{} ", tag)) => return,
                Ok(_) => continue,
                Err(_) => return,
            }
        }
    }

    fn next_tag(&mut self) -> String {
        self.tag_counter += 1;
        format!("A{:03}", self.tag_counter)
    }

    fn read_greeting(&mut self) -> Result<(), String> {
        let greeting = read_line(&mut self.reader)?;
        if !greeting.contains("OK IMAP4rev1") {
            return Err(format!(
                "{}: connect/login failed: unexpected greeting: {}",
                ERR_IMAP, greeting
            ));
        }
        Ok(())
    }

    fn capability(&mut self) -> Result<Vec<String>, String> {
        let tag = self.next_tag();
        let cmd = format!("{} CAPABILITY\r\n", tag);
        write_line(&mut self.writer, &cmd)?;
        let mut caps = Vec::new();
        loop {
            let line = read_line(&mut self.reader)?;
            if line.starts_with("* CAPABILITY") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                for part in parts.iter().skip(2) {
                    caps.push(part.to_string());
                }
            }
            if line.starts_with(&format!("{} ", tag)) {
                break;
            }
        }
        Ok(caps)
    }

    fn login(&mut self, username: &str, password: &str) -> Result<(), String> {
        let tag = self.next_tag();
        let cmd = format!("{} LOGIN \"{}\" \"{}\"\r\n", tag, username, password);
        write_line(&mut self.writer, &cmd)?;
        loop {
            let line = read_line(&mut self.reader)?;
            if line.starts_with(&format!("{} ", tag)) {
                if line.contains(" NO ") {
                    // Server responds: tag NO [AUTHENTICATIONFAILED] Invalid credentials
                    // Extract error after "NO ", stripping [AUTHENTICATIONFAILED] prefix
                    let after_no = line.split("NO ").nth(1).unwrap_or("").trim();
                    let err_msg = after_no
                        .strip_prefix("[AUTHENTICATIONFAILED] ")
                        .unwrap_or(after_no);
                    // The Go client closes the connection with LOGOUT when
                    // authentication fails, and the oracle records that line.
                    self.logout_after_failure();
                    return Err(format!("{}: connect/login failed: {}", ERR_IMAP, err_msg));
                }
                if line.contains(" OK ") {
                    return Ok(());
                }
                return Err(format!("{}: IMAP LOGIN failed: {}", ERR_IMAP, line));
            }
        }
    }

    fn authenticate_xoauth2(&mut self, username: &str, token: &str) -> Result<(), String> {
        let tag = self.next_tag();
        let payload = format!("user={}\u{1}auth=Bearer {}\u{1}\u{1}", username, token);
        let encoded = STANDARD.encode(payload.as_bytes());
        let cmd = format!("{} AUTHENTICATE XOAUTH2 {}\r\n", tag, encoded);
        write_line(&mut self.writer, &cmd)?;
        loop {
            let line = read_line(&mut self.reader)?;
            if line.starts_with("+") {
                // Continuation - send empty response
                write_line(&mut self.writer, "\r\n")?;
                continue;
            }
            if line.starts_with(&format!("{} ", tag)) {
                if line.contains(" NO ") {
                    let after_no = line.split("NO ").nth(1).unwrap_or("").trim();
                    let err_msg = after_no
                        .strip_prefix("[AUTHENTICATIONFAILED] ")
                        .unwrap_or(after_no);
                    return Err(format!("{}: connect/login failed: {}", ERR_IMAP, err_msg));
                }
                if line.contains(" OK ") {
                    return Ok(());
                }
                return Err(format!("{}: IMAP AUTHENTICATE failed: {}", ERR_IMAP, line));
            }
        }
    }
}

/// Validates a UID range string, mirroring Go's `imap.ParseSeqSet`.
fn validate_uid_range(range: &str) -> Result<(), String> {
    if range.is_empty() {
        return Err(format!(
            "{}: invalid UID range: imap: bad sequence set value \"\"",
            ERR_IMAP
        ));
    }
    for part in range.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!(
                "{}: invalid UID range: imap: bad sequence set value \"\"",
                ERR_IMAP
            ));
        }
        if part.contains(':') {
            let bounds: Vec<&str> = part.split(':').collect();
            if bounds.len() != 2 {
                return Err(format!(
                    "{}: invalid UID range: imap: bad sequence set value \"{}\"",
                    ERR_IMAP, part
                ));
            }
            for bound in bounds {
                if !bound.is_empty() && bound != "*" && !bound.chars().all(|c| c.is_ascii_digit()) {
                    return Err(format!(
                        "{}: invalid UID range: imap: bad sequence set value \"{}\"",
                        ERR_IMAP, part
                    ));
                }
            }
        } else if part != "*" && !part.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "{}: invalid UID range: imap: bad sequence set value \"{}\"",
                ERR_IMAP, part
            ));
        }
    }
    Ok(())
}

/// Collapses a sorted list of UIDs into IMAP sequence-set notation.
fn collapse_uids(uids: &[u32]) -> String {
    if uids.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<u32> = uids.to_vec();
    sorted.sort_unstable();

    let mut ranges: Vec<String> = Vec::new();
    let mut start = sorted[0];
    let mut end = sorted[0];

    for &uid in &sorted[1..] {
        if uid == end + 1 {
            end = uid;
        } else {
            if start == end {
                ranges.push(start.to_string());
            } else {
                ranges.push(format!("{}:{}", start, end));
            }
            start = uid;
            end = uid;
        }
    }
    if start == end {
        ranges.push(start.to_string());
    } else {
        ranges.push(format!("{}:{}", start, end));
    }
    ranges.join(",")
}

/// Reads lines until a tagged response, extracting UIDVALIDITY from untagged OK lines.
fn read_examine_response(reader: &mut BufReader<IoStream>, tag: &str) -> Result<u32, String> {
    let mut uid_validity: Option<u32> = None;
    loop {
        let line = read_line(reader)?;
        if let Some(start) = line
            .strip_prefix("* ")
            .and_then(|rest| rest.find("[UIDVALIDITY "))
        {
            let after = &line[start + "* ".len() + "[UIDVALIDITY ".len()..];
            if let Some(end) = after.find(']')
                && let Ok(value) = after[..end].parse::<u32>()
            {
                uid_validity = Some(value);
            }
        }
        if line.starts_with(tag) {
            if let Some(start) = line.find(" NO ") {
                let error = line[start + 4..].trim().to_string();
                return Err(format!("{}: {}", ERR_IMAP, error));
            }
            break;
        }
    }
    uid_validity.ok_or_else(|| format!("{}: folder select failed: no UIDVALIDITY", ERR_IMAP))
}

/// Reads lines until a tagged response, collecting UIDs from * SEARCH lines.
fn read_search_response(reader: &mut BufReader<IoStream>, tag: &str) -> Result<Vec<u32>, String> {
    let mut uids = Vec::new();
    loop {
        let line = read_line(reader)?;
        if line.starts_with("* SEARCH") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for part in parts.iter().skip(2) {
                if let Ok(uid) = part.parse::<u32>() {
                    uids.push(uid);
                }
            }
        }
        if line.starts_with(tag) {
            if let Some(start) = line.find(" NO ") {
                let error = line[start + 4..].trim().to_string();
                return Err(format!("{}: {}", ERR_IMAP, error));
            }
            break;
        }
    }
    Ok(uids)
}

/// Reads lines and literals until a tagged response, parsing FETCH responses.
/// Handles the Go fake server's wire format:
/// * N FETCH (UID N FLAGS (...) BODY[HEADER] {len}\r\n<header> BODY[TEXT] {len}\r\n<body>)\r\n
fn read_fetch_response(
    reader: &mut BufReader<IoStream>,
    tag: &str,
) -> Result<Vec<FetchedMessage>, String> {
    let mut fetched = Vec::new();
    loop {
        let response = read_response_bytes(reader)?;
        let text = String::from_utf8_lossy(&response).to_string();
        let head = text.trim_end_matches(['\r', '\n']).to_string();

        if head.starts_with("* ") && head.contains(" FETCH ") {
            let uid = parse_uid_from_fetch_line(&head);
            let flags = parse_flags_from_fetch_line(&head);
            let header = extract_literal(&response, "BODY[HEADER]").ok_or_else(|| {
                format!(
                    "{}: read fetched body failed: missing BODY[HEADER]",
                    ERR_IMAP
                )
            })?;
            let body = extract_literal(&response, "BODY[TEXT]").ok_or_else(|| {
                format!("{}: read fetched body failed: missing BODY[TEXT]", ERR_IMAP)
            })?;
            fetched.push(FetchedMessage {
                uid,
                flags,
                header,
                body,
                internal_date: None,
            });
            continue;
        }

        if head.starts_with(tag) {
            if let Some(start) = head.find(" NO ") {
                return Err(format!(
                    "{}: read fetched body failed: {}",
                    ERR_IMAP,
                    head[start + 4..].trim()
                ));
            }
            break;
        }
    }
    Ok(fetched)
}

/// Reads one logical response, resolving IMAP literals: a line that ends in
/// `{n}` is followed by exactly n bytes of payload, and the rest of the same
/// response follows after it. Line-based reading alone truncates a section at
/// the CRLF inside the payload, so the bytes are assembled here.
fn read_response_bytes(reader: &mut BufReader<IoStream>) -> Result<Vec<u8>, String> {
    let mut assembled = Vec::new();
    loop {
        let mut line = Vec::new();
        reader
            .read_until(b'\n', &mut line)
            .map_err(|e| format!("IMAP read failed: {}", e))?;
        if line.is_empty() {
            return Err(format!(
                "{}: read fetched body failed: connection closed",
                ERR_IMAP
            ));
        }
        assembled.extend_from_slice(&line);
        let text = String::from_utf8_lossy(&line).to_string();
        let trimmed = text.trim_end_matches(['\r', '\n']).to_string();
        match literal_length(&trimmed) {
            Some(len) => {
                let literal = read_bounded_body(reader, len)?;
                assembled.extend_from_slice(&literal);
                continue;
            }
            None => return Ok(assembled),
        }
    }
}

/// The `{n}` marker that ends a line when the next bytes are a literal.
fn literal_length(line: &str) -> Option<usize> {
    if !line.ends_with('}') {
        return None;
    }
    let open = line.rfind('{')?;
    line[open + 1..line.len() - 1].parse::<usize>().ok()
}

/// The payload of one section marker inside an assembled response.
fn extract_literal(response: &[u8], marker: &str) -> Option<Vec<u8>> {
    let needle = marker.as_bytes();
    let start = response
        .windows(needle.len())
        .position(|window| window == needle)?;
    let rest = &response[start + needle.len()..];
    let open = rest.iter().position(|byte| *byte == b'{')?;
    let close = rest[open..].iter().position(|byte| *byte == b'}')? + open;
    let len: usize = std::str::from_utf8(&rest[open + 1..close])
        .ok()?
        .parse()
        .ok()?;
    let after_marker = &rest[close + 1..];
    let skip = after_marker.windows(2).position(|pair| pair == b"\r\n")? + 2;
    Some(after_marker.get(skip..skip + len)?.to_vec())
}

/// Parses UID from a FETCH response line.
/// The UID from a FETCH line. The item list opens with `(UID`, so the token is
/// compared without its opening parenthesis.
fn parse_uid_from_fetch_line(line: &str) -> u32 {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    tokens
        .iter()
        .position(|token| token.trim_start_matches('(') == "UID")
        .and_then(|index| tokens.get(index + 1))
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Parses FLAGS from a FETCH response line.
fn parse_flags_from_fetch_line(line: &str) -> Option<Vec<String>> {
    let start = line.find("FLAGS (")?;
    let after = &line[start + "FLAGS (".len()..];
    let end = after.find(')')?;
    let flags_str = &after[..end];
    Some(
        flags_str
            .split_whitespace()
            .map(|s| s.to_string())
            .collect(),
    )
}

/// Reads a literal (body section) of the given length and checks the 64KB bound.
fn read_bounded_body(reader: &mut BufReader<IoStream>, len: usize) -> Result<Vec<u8>, String> {
    let max_body_bytes = 64 * 1024;
    if len > max_body_bytes {
        // Need to read and discard the oversized bytes
        let mut to_discard = len;
        let mut buf = [0u8; 1024];
        while to_discard > 0 {
            let to_read = std::cmp::min(to_discard, buf.len());
            match reader.read_exact(&mut buf[..to_read]) {
                Ok(_) => to_discard -= to_read,
                Err(_) => break,
            }
        }
        return Err(format!(
            "{}: read fetched body failed: IMAP body section exceeds {} bytes",
            ERR_IMAP, max_body_bytes
        ));
    }
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("IMAP read body failed: {}", e))?;
    Ok(body)
}

#[cfg(test)]
mod dial_tests {
    use super::*;
    use crate::email::session::ImapDialer as _;

    #[test]
    fn resolves_hostnames_and_formats_refused_connections_like_go() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port() as i64;
        drop(listener);
        let expected_address = format!("localhost:{port}")
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();
        let config = ImapConfig {
            host: "localhost".to_owned(),
            port,
            ..ImapConfig::default()
        };
        let error = ImapDialer::new().dial(&config).err().unwrap();
        assert_eq!(
            error,
            format!(
                "email: imap error: connect/login failed: dial tcp {expected_address}: connect: connection refused"
            )
        );
    }

    #[test]
    fn falls_back_after_refused_ipv6_address_to_live_ipv4_address() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let ipv4_address = listener.local_addr().unwrap();
        let ipv6_refusal =
            std::net::SocketAddr::new(std::net::Ipv6Addr::LOCALHOST.into(), ipv4_address.port());
        let addresses = [ipv6_refusal, ipv4_address];

        let (connected, accepted) = std::thread::scope(|scope| {
            let accepting = scope.spawn(|| listener.accept().unwrap().0);
            let connected = connect_addresses(addresses, Duration::from_secs(1)).unwrap();
            (connected, accepting.join().unwrap())
        });

        assert!(connected.peer_addr().unwrap().is_ipv4());
        assert!(accepted.peer_addr().unwrap().is_ipv4());
    }

    #[test]
    fn reports_first_resolved_error_when_later_address_has_a_different_error() {
        let first: std::net::SocketAddr = "[::1]:143".parse().unwrap();
        let second: std::net::SocketAddr = "127.0.0.1:143".parse().unwrap();
        let result =
            connect_addresses_with([first, second], Duration::from_secs(1), |address, _| {
                let kind = if address.is_ipv6() {
                    std::io::ErrorKind::TimedOut
                } else {
                    std::io::ErrorKind::ConnectionRefused
                };
                Err(std::io::Error::from(kind))
            });
        let (_, error) = result.expect_err("all resolved addresses fail").unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }
}
