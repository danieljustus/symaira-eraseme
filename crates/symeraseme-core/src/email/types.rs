//! Shared value types for the inbox port.

use chrono::{DateTime, FixedOffset};
use std::fmt;

/// One parsed mailbox message. Field order mirrors Go's `email.Message` because
/// the wire form of this struct is part of the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: Option<DateTime<FixedOffset>>,
    pub body: String,
    /// `None` reproduces Go's nil slice, which marshals as JSON `null`.
    pub flags: Option<Vec<String>>,
    pub message_id: String,
    pub thread_id: String,
    pub imap_uid: u32,
}

/// A request a reply can be matched against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalRequest {
    pub id: i64,
    pub broker_id: String,
}

/// A message plus the matching decision. Every message carries an explicit
/// `thread`, `subject` or `unmatched` method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedMessage {
    pub message: Message,
    pub request_id: Option<i64>,
    pub match_method: MatchMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMethod {
    Thread,
    Subject,
    Unmatched,
}

impl MatchMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchMethod::Thread => "thread",
            MatchMethod::Subject => "subject",
            MatchMethod::Unmatched => "unmatched",
        }
    }
}

/// IMAP connection settings. `password` and the OAuth2 token are write-only
/// operational inputs: no error produced here echoes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapConfig {
    pub host: String,
    pub port: i64,
    pub username: String,
    pub password: String,
    pub use_tls: bool,
    pub folder: String,
    pub since_days: i64,
    pub max_messages: i64,
    pub oauth2: Option<OAuth2Token>,
    pub timeout_seconds: i64,
    pub allow_insecure_cleartext_auth: bool,
}

impl Default for ImapConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 993,
            username: String::new(),
            password: String::new(),
            use_tls: true,
            folder: String::new(),
            since_days: 0,
            max_messages: 0,
            oauth2: None,
            timeout_seconds: 30,
            allow_insecure_cleartext_auth: false,
        }
    }
}

impl ImapConfig {
    /// The values that must never appear in an error message.
    pub fn secrets(&self) -> Vec<String> {
        let mut secrets = vec![self.password.clone()];
        if let Some(oauth2) = &self.oauth2
            && !oauth2.access_token.is_empty()
        {
            secrets.push(oauth2.access_token.clone());
            let username = if oauth2.username.is_empty() {
                self.username.clone()
            } else {
                oauth2.username.clone()
            };
            secrets.push(crate::email::config::xoauth2_payload(
                &username,
                &oauth2.access_token,
            ));
        }
        secrets
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuth2Token {
    pub username: String,
    pub access_token: String,
}

/// An IMAP failure. `Display` is the exact message Go produces, including the
/// `email: imap error:` prefix, so callers and fixtures can compare text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapError(String);

impl ImapError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ImapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ImapError {}
