//! SMTP transaction adapter corresponding to Go's NetSMTPTransport.

use super::SmtpTransport;
use crate::email::{OAuth2Token, redact_error};
use base64::{Engine, engine::general_purpose::STANDARD};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use std::collections::BTreeSet;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: i64,
    pub username: String,
    pub password: String,
    pub use_tls: bool,
    pub from: String,
    pub timeout: Duration,
    pub oauth2: Option<OAuth2Token>,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: 587,
            username: String::new(),
            password: String::new(),
            use_tls: true,
            from: String::new(),
            timeout: Duration::ZERO,
            oauth2: None,
        }
    }
}

impl fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("use_tls", &self.use_tls)
            .field("has_password", &!self.password.is_empty())
            .field("has_oauth2", &self.oauth2.is_some())
            .finish_non_exhaustive()
    }
}

/// Sends only when explicitly supplied to the campaign's email adapter.
/// Construction does not connect or change the CLI's default no-sender behavior.
pub struct NetSmtpTransport {
    config: SmtpConfig,
    tls_config: Option<Arc<ClientConfig>>,
}

impl NetSmtpTransport {
    pub fn new(config: SmtpConfig) -> Self {
        Self {
            config,
            tls_config: None,
        }
    }

    /// Explicit roots/configuration for applications with a private CA.
    pub fn with_tls_config(mut self, config: Arc<ClientConfig>) -> Self {
        self.tls_config = Some(config);
        self
    }

    fn send_inner(&self, recipients: Option<&[String]>, raw: &[u8]) -> Result<(), String> {
        let recipients = recipients
            .filter(|items| !items.is_empty())
            .ok_or("no recipients")?;
        let cfg = &self.config;
        if cfg.host.is_empty() {
            return Err("smtp host is empty".into());
        }
        if cfg.port <= 0 {
            return Err("smtp port must be positive".into());
        }
        if cfg.port > 65535 {
            return Err("smtp port is outside TCP range".into());
        }
        let timeout = if cfg.timeout.is_zero() {
            Duration::from_secs(30)
        } else {
            cfg.timeout
        };
        let addresses = (cfg.host.as_str(), cfg.port as u16)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?;
        let mut failure = "smtp host resolved to no addresses".to_owned();
        let mut connected = None;
        for address in addresses {
            match TcpStream::connect_timeout(&address, timeout) {
                Ok(socket) => {
                    connected = Some(socket);
                    break;
                }
                Err(error) => failure = error.to_string(),
            }
        }
        let socket = connected.ok_or(failure)?;
        socket
            .set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        socket
            .set_write_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        let mut session = Session {
            io: BufReader::new(Connection::Plain(socket)),
            extensions: BTreeSet::new(),
        };
        session.expect(220)?;
        session.hello()?;
        if cfg.use_tls {
            if !session.extensions.contains("STARTTLS") {
                return Err("SMTP server does not support STARTTLS".into());
            }
            session.command("STARTTLS", 220)?;
            if !session.io.buffer().is_empty() {
                return Err("SMTP bytes preceded TLS handshake".into());
            }
            let Connection::Plain(socket) = session.io.into_inner() else {
                unreachable!()
            };
            let tls = match &self.tls_config {
                Some(config) => config.clone(),
                None => {
                    let roots: RootCertStore = crate::email::imap::client::platform_root_store()
                        .map_err(|e| e.replace("email: imap error: ", ""))?;
                    Arc::new(
                        ClientConfig::builder_with_provider(Arc::new(
                            rustls::crypto::ring::default_provider(),
                        ))
                        .with_safe_default_protocol_versions()
                        .map_err(|e| e.to_string())?
                        .with_root_certificates(roots)
                        .with_no_client_auth(),
                    )
                }
            };
            let name = cfg
                .host
                .clone()
                .try_into()
                .map_err(|_| "invalid SMTP TLS server name")?;
            let connection = ClientConnection::new(tls, name).map_err(|e| e.to_string())?;
            session = Session {
                io: BufReader::new(Connection::Tls(Box::new(StreamOwned::new(
                    connection, socket,
                )))),
                extensions: BTreeSet::new(),
            };
            session.hello()?;
        }
        if let Some(token) = &cfg.oauth2 {
            if token.username.is_empty() || token.access_token.is_empty() {
                return Err("oauth2 username/token is empty".into());
            }
            let payload = format!(
                "user={}\x01auth=Bearer {}\x01\x01",
                token.username, token.access_token
            );
            session.authenticate(
                "XOAUTH2",
                payload.as_bytes(),
                "SMTP XOAUTH2 authentication rejected",
            )?;
        } else if !cfg.username.is_empty() && !cfg.password.is_empty() {
            if !cfg.use_tls && !matches!(cfg.host.as_str(), "localhost" | "127.0.0.1" | "::1") {
                let _ = session.command("QUIT", 221);
                return Err("unencrypted connection".into());
            }
            let payload = format!("\0{}\0{}", cfg.username, cfg.password);
            session.authenticate("PLAIN", payload.as_bytes(), "unexpected server challenge")?;
        }
        let mut mail = format!("MAIL FROM:<{}>", cfg.from);
        validate_line(&cfg.from)?;
        if session.extensions.contains("8BITMIME") {
            mail.push_str(" BODY=8BITMIME");
        }
        if session.extensions.contains("SMTPUTF8") {
            mail.push_str(" SMTPUTF8");
        }
        session.command(&mail, 250)?;
        for recipient in recipients {
            validate_line(recipient)?;
            session.command(&format!("RCPT TO:<{recipient}>"), 25)?;
        }
        session.command("DATA", 354)?;
        // Go textproto.DotWriter canonicalizes LF/CRLF and doubles a leading dot.
        let mut start = true;
        let mut previous_cr = false;
        let mut encoded = Vec::with_capacity(raw.len().saturating_add(64));
        for &byte in raw {
            if start && byte == b'.' {
                encoded.push(b'.');
            }
            if byte == b'\n' && !previous_cr {
                encoded.push(b'\r');
            }
            encoded.push(byte);
            start = byte == b'\n';
            previous_cr = byte == b'\r';
        }
        if raw.is_empty() || !start {
            if !previous_cr {
                encoded.push(b'\r');
            }
            encoded.push(b'\n');
        }
        encoded.extend_from_slice(b".\r\n");
        session.write(&encoded)?;
        session.flush()?;
        session.expect(250)?;
        session.command("QUIT", 221)?;
        Ok(())
    }
}

impl SmtpTransport for NetSmtpTransport {
    fn send(&self, recipients: Option<&[String]>, raw: &[u8]) -> Result<(), String> {
        let cfg = &self.config;
        let plain = format!("\0{}\0{}", cfg.username, cfg.password);
        let escaped_password = smtp_quoted_error(&cfg.password);
        let mut secrets = vec![
            cfg.password.clone(),
            escaped_password[1..escaped_password.len() - 1].to_owned(),
            STANDARD.encode(plain),
        ];
        if let Some(token) = &cfg.oauth2 {
            let payload = format!(
                "user={}\x01auth=Bearer {}\x01\x01",
                token.username, token.access_token
            );
            secrets.extend([
                token.access_token.clone(),
                payload.clone(),
                STANDARD.encode(payload),
            ]);
            let escaped = smtp_quoted_error(&token.access_token);
            secrets.push(escaped[1..escaped.len() - 1].to_owned());
        }
        self.send_inner(recipients, raw)
            .map_err(|error| redact_error(&error, &secrets))
    }
}

enum Connection {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}
impl Read for Connection {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(io) => io.read(bytes),
            Self::Tls(io) => io.read(bytes),
        }
    }
}
impl Write for Connection {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(io) => io.write(bytes),
            Self::Tls(io) => io.write(bytes),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(io) => io.flush(),
            Self::Tls(io) => io.flush(),
        }
    }
}

struct Session {
    io: BufReader<Connection>,
    extensions: BTreeSet<String>,
}
impl Session {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.io
            .get_mut()
            .write_all(bytes)
            .map_err(|e| e.to_string())
    }
    fn flush(&mut self) -> Result<(), String> {
        self.io.get_mut().flush().map_err(|e| e.to_string())
    }
    fn reply(&mut self) -> Result<(u16, String), String> {
        let mut code = None;
        let mut messages = Vec::new();
        let mut size = 0;
        loop {
            let mut line = String::new();
            let n = Read::by_ref(&mut self.io)
                .take(65537)
                .read_line(&mut line)
                .map_err(|e| e.to_string())?;
            size += n;
            if n == 0 {
                return Err("EOF".into());
            }
            if n > 65536 || size > 1024 * 1024 {
                return Err("SMTP response exceeds limit".into());
            }
            let bytes = line.as_bytes();
            if bytes.len() < 4
                || !bytes[..3].iter().all(u8::is_ascii_digit)
                || !matches!(bytes[3], b' ' | b'-')
            {
                return Err("malformed SMTP response".into());
            }
            let current = line[..3].parse::<u16>().map_err(|e| e.to_string())?;
            if code.is_some_and(|code| code != current) {
                return Err("inconsistent SMTP multiline response".into());
            }
            code = Some(current);
            messages.push(line[4..].trim_end_matches(['\r', '\n']).to_owned());
            if bytes[3] == b' ' {
                return Ok((current, messages.join("\n")));
            }
        }
    }
    fn expect(&mut self, expected: u16) -> Result<String, String> {
        let (code, message) = self.reply()?;
        if code == expected || (expected < 100 && code / 10 == expected) {
            Ok(message)
        } else {
            Err(format!("{code:03} {}", smtp_quoted_error(&message)))
        }
    }
    fn command(&mut self, command: &str, expected: u16) -> Result<String, String> {
        validate_line(command)?;
        self.write(command.as_bytes())?;
        self.write(b"\r\n")?;
        self.flush()?;
        self.expect(expected)
    }
    fn hello(&mut self) -> Result<(), String> {
        match self.command("EHLO localhost", 250) {
            Ok(reply) => {
                self.extensions = reply
                    .lines()
                    .skip(1)
                    .filter_map(|line| line.split(' ').next())
                    .map(str::to_owned)
                    .collect();
                Ok(())
            }
            Err(_) => {
                self.extensions.clear();
                self.command("HELO localhost", 250).map(|_| ())
            }
        }
    }
    fn authenticate(
        &mut self,
        mechanism: &str,
        payload: &[u8],
        challenge_error: &str,
    ) -> Result<(), String> {
        let command = format!("AUTH {mechanism} {}", STANDARD.encode(payload));
        self.write(command.as_bytes())?;
        self.write(b"\r\n")?;
        self.flush()?;
        let (code, message) = self.reply()?;
        if code == 235 {
            return Ok(());
        }
        let error = if code == 334 {
            challenge_error.to_owned()
        } else {
            format!("{code:03} {}", smtp_quoted_error(&message))
        };
        let _ = self.command("*", 501);
        let _ = self.command("QUIT", 221);
        Err(error)
    }
}

fn validate_line(line: &str) -> Result<(), String> {
    if line.contains(['\r', '\n']) {
        Err("smtp: A line must not contain CR or LF".into())
    } else {
        Ok(())
    }
}

// Go 1.26's textproto.Error quotes server diagnostics rather than rendering
// them as a raw line. Preserve its ASCII/control spellings at this boundary.
fn smtp_quoted_error(message: &str) -> String {
    let mut quoted = String::from("\"");
    for character in message.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\x07' => quoted.push_str("\\a"),
            '\x08' => quoted.push_str("\\b"),
            '\x0c' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\x0b' => quoted.push_str("\\v"),
            character if character.is_ascii_control() => {
                quoted.push_str(&format!("\\x{:02x}", u32::from(character)))
            }
            character
                if character.is_control() || (character.is_whitespace() && character != ' ') =>
            {
                quoted.push_str(&format!("\\u{:04x}", u32::from(character)))
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}
