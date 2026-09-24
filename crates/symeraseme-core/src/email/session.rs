//! The transport boundary. Nothing in this module talks to a network: the
//! policy receives a session and asks it to select, search and fetch.

use crate::email::types::ImapConfig;
use chrono::{DateTime, Utc};

/// One bounded FETCH result. `header` is RFC 5322 header bytes and `body` the
/// requested plain-text snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedMessage {
    pub uid: u32,
    pub header: Vec<u8>,
    pub body: Vec<u8>,
    /// `None` reproduces Go's nil slice, which marshals as JSON `null`.
    pub flags: Option<Vec<String>>,
    pub internal_date: Option<DateTime<Utc>>,
}

pub trait ImapSession {
    fn select(&mut self, folder: &str) -> Result<u32, String>;
    /// `since` is present only when the configuration sets a since window.
    fn search_uid(
        &mut self,
        uid_range: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<u32>, String>;
    fn fetch(&mut self, uids: &[u32]) -> Result<Vec<FetchedMessage>, String>;
    fn close(&mut self);
}

pub trait ImapDialer: Send + Sync {
    fn dial(&self, config: &ImapConfig) -> Result<Box<dyn ImapSession>, String>;
}
