//! Real network dialer for plain-TCP IMAP (the main parity target).
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
//! The implementation only handles the plain-TCP path; TLS/STARTTLS cases
//! are not ported (see go-only replay cases). For use_tls=true or an accepted
//! STARTTLS connection, return an explicit error saying the TLS transport
//! is not ported yet.

use crate::email::policy::ERR_IMAP;
use crate::email::session::{FetchedMessage, ImapSession};
use crate::email::types::ImapConfig;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// IMAP dialer that creates plain-TCP connections.
pub struct ImapDialer;

impl ImapDialer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ImapDialer {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::email::session::ImapDialer for ImapDialer {
    fn dial(&self, config: &ImapConfig) -> Result<Box<dyn ImapSession>, String> {
        if config.use_tls {
            return Err(format!("{}: the TLS transport is not ported yet", ERR_IMAP));
        }
        if config.host.is_empty() {
            return Err(format!("{}: host is empty", ERR_IMAP));
        }
        let port = if config.port > 0 {
            config.port as u16
        } else {
            143
        };
        let addr = format!("{}:{}", config.host, port);
        let timeout = Duration::from_secs(if config.timeout_seconds > 0 {
            config.timeout_seconds as u64
        } else {
            30
        });

        let parsed_addr = addr
            .parse()
            .map_err(|e| format!("{}: connect/login failed: {}", ERR_IMAP, e))?;
        let stream = TcpStream::connect_timeout(&parsed_addr, timeout)
            .map_err(|e| format!("{}: connect/login failed: {}", ERR_IMAP, e))?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("{}: read timeout failed: {}", ERR_IMAP, e))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| format!("{}: write timeout failed: {}", ERR_IMAP, e))?;

        let mut session = ImapSessionImpl {
            reader: BufReader::new(
                stream
                    .try_clone()
                    .map_err(|e| format!("{}: connect/login failed: {}", ERR_IMAP, e))?,
            ),
            writer: stream,
            tag_counter: 0,
            selected_folder: None,
        };

        // Read greeting
        session.read_greeting()?;

        // Send and parse CAPABILITY
        let caps = session.capability()?;
        let starttls_supported = caps.iter().any(|c| c.as_str() == "STARTTLS");

        // We don't port STARTTLS: if the server advertises it, we cannot proceed.
        if starttls_supported {
            return Err(format!("{}: the TLS transport is not ported yet", ERR_IMAP));
        }
        // No STARTTLS: check cleartext auth permission.
        if !config.allow_insecure_cleartext_auth {
            return Err(format!(
                "{}: cleartext authentication prohibited: server does not support STARTTLS",
                ERR_IMAP
            ));
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

pub struct ImapSessionImpl {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    tag_counter: u32,
    selected_folder: Option<String>,
}

/// Reads a line from the stream (CRLF terminated).
/// A mailbox argument the way the Go client library emits it: `INBOX` is sent
/// raw (servers handle a quoted INBOX badly — `imap.FormatMailboxName` special
/// cases it, case-insensitively) and every other name is a quoted string.
///
/// ponytail: names outside US-ASCII are sent as UTF-8 instead of Go's modified
/// UTF-7 (`utf7.Encoding`). The configuration's folder comes from `IMAP_FOLDER`
/// and is ASCII in every pinned case; upgrade by encoding the name in modified
/// UTF-7 before quoting when a non-ASCII folder has to be supported.
fn mailbox_argument(folder: &str) -> String {
    if folder.eq_ignore_ascii_case("INBOX") {
        return folder.to_string();
    }
    format!("\"{}\"", folder.replace('\\', "\\\\").replace('"', "\\\""))
}

fn read_line(reader: &mut BufReader<TcpStream>) -> Result<String, String> {
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
fn write_line(writer: &mut TcpStream, line: &str) -> Result<(), String> {
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
fn read_examine_response(reader: &mut BufReader<TcpStream>, tag: &str) -> Result<u32, String> {
    let mut uid_validity: Option<u32> = None;
    loop {
        let line = read_line(reader)?;
        if line.starts_with("* ")
            && let Some(start) = line.find("[UIDVALIDITY ")
        {
            let after = &line[start + "[UIDVALIDITY ".len()..];
            if let Some(end) = after.find(']')
                && let Ok(val) = after[..end].parse::<u32>()
            {
                uid_validity = Some(val);
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
fn read_search_response(reader: &mut BufReader<TcpStream>, tag: &str) -> Result<Vec<u32>, String> {
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
    reader: &mut BufReader<TcpStream>,
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
fn read_response_bytes(reader: &mut BufReader<TcpStream>) -> Result<Vec<u8>, String> {
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
fn read_bounded_body(reader: &mut BufReader<TcpStream>, len: usize) -> Result<Vec<u8>, String> {
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
