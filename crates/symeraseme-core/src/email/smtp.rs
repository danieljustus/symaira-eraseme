//! Deterministic MIME construction and the outbound transport boundary.
//!
//! The network implementation remains outside core. Callers provide a fixed
//! clock and message ID for reviewable bytes; an empty ID uses the OS CSPRNG.

use chrono::{DateTime, FixedOffset};
use rand::TryRng;
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub cc: String,
    pub bcc: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmtpError {
    MissingFrom,
    EmptyRecipient,
    MessageIDGeneration,
    Transport(String),
}

impl fmt::Display for SmtpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFrom => formatter.write_str("email: SMTP sender is not configured"),
            Self::EmptyRecipient => formatter.write_str("email: smtp error: recipient is empty"),
            Self::MessageIDGeneration => formatter
                .write_str("email: smtp error: generate message id: randomness unavailable"),
            Self::Transport(error) => write!(formatter, "email: smtp error: {error}"),
        }
    }
}

impl std::error::Error for SmtpError {}

/// Side-effect boundary for outbound mail. Production network access is not
/// implemented by core; tests and future adapters can provide a transport.
pub trait SmtpTransport {
    fn send(&self, recipients: Option<&[String]>, message: &[u8]) -> Result<(), String>;
}

/// Build Go-compatible multipart MIME bytes from fixed time and message ID.
pub fn build_mime_at(
    message: &EmailMessage,
    from: &str,
    now: DateTime<FixedOffset>,
    message_id: &str,
) -> Result<(Vec<u8>, String), SmtpError> {
    if from.trim().is_empty() {
        return Err(SmtpError::MissingFrom);
    }
    if message.to.trim().is_empty() {
        return Err(SmtpError::EmptyRecipient);
    }

    let message_id = if message_id.is_empty() {
        let mut random = [0_u8; 12];
        rand::rngs::SysRng
            .try_fill_bytes(&mut random)
            .map_err(|_| SmtpError::MessageIDGeneration)?;
        format!("<{}@symeraseme>", hex::encode(random))
    } else {
        message_id.to_owned()
    };
    let digest = Sha256::digest(message_id.as_bytes());
    let boundary = format!("=_symeraseme_{}", hex::encode(&digest[..8]));
    let mut mime = String::new();
    write_header(&mut mime, "From", from);
    write_header(&mut mime, "To", &message.to);
    if !message.cc.is_empty() {
        write_header(&mut mime, "Cc", &message.cc);
    }
    write_header(&mut mime, "Subject", &message.subject);
    write_header(
        &mut mime,
        "Date",
        &now.format("%a, %d %b %Y %H:%M:%S %z").to_string(),
    );
    write_header(&mut mime, "Message-ID", &message_id);
    write_header(&mut mime, "MIME-Version", "1.0");
    write_header(
        &mut mime,
        "Content-Type",
        &format!("multipart/mixed; boundary=\"{boundary}\""),
    );
    mime.push_str("\r\n--");
    mime.push_str(&boundary);
    mime.push_str("\r\nContent-Type: text/plain; charset=utf-8\r\n");
    mime.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
    mime.push_str(&message.body.replace("\r\n", "\n").replace('\r', "\n"));
    mime.push_str("\r\n--");
    mime.push_str(&boundary);
    mime.push_str("--\r\n");
    Ok((mime.into_bytes(), message_id))
}

/// Return the SMTP envelope recipients in To, Cc, Bcc order.
pub fn recipients(message: &EmailMessage) -> Option<Vec<String>> {
    let recipients = [&message.to, &message.cc, &message.bcc]
        .into_iter()
        .flat_map(|field| field.split(','))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    (!recipients.is_empty()).then_some(recipients)
}

/// Build one deterministic message and hand it to the supplied transport.
pub fn send_message_at(
    message: &EmailMessage,
    from: &str,
    now: DateTime<FixedOffset>,
    message_id: &str,
    transport: &dyn SmtpTransport,
) -> Result<String, SmtpError> {
    let (raw, message_id) = build_mime_at(message, from, now, message_id)?;
    transport
        .send(recipients(message).as_deref(), &raw)
        .map_err(SmtpError::Transport)?;
    Ok(message_id)
}

fn write_header(output: &mut String, name: &str, value: &str) {
    output.push_str(name);
    output.push_str(": ");
    output.extend(
        value
            .chars()
            .filter(|character| !matches!(character, '\r' | '\n')),
    );
    output.push_str("\r\n");
}
