//! Inbox service: poll, correlate, persist, and only then commit the
//! high-water mark. A failed insert leaves the persisted mark untouched, so a
//! retry can safely ingest the same batch again.
//!
//! Ported from `internal/email/service.go`.

use crate::email::hwm::{HwmStore, MemoryHwmStore, StagingHwmStore};
use crate::email::policy::{match_reply_to_request, parse_email_body, poll_folders};
use crate::email::session::ImapDialer;
use crate::email::types::{ImapConfig, ImapError, MatchedMessage, RemovalRequest};
use std::collections::HashMap;

/// Persistence boundary for inbox replies. Implementations store bounded
/// snippets and parsed metadata — never credentials or raw authentication
/// responses.
pub trait ReplyStore {
    fn insert(&self, reply: &MatchedMessage, snippet: &str) -> Result<(), String>;
}

pub struct InboxService<'a> {
    dialer: &'a dyn ImapDialer,
    state: Option<&'a dyn HwmStore>,
}

impl<'a> InboxService<'a> {
    pub fn new(dialer: &'a dyn ImapDialer, state: Option<&'a dyn HwmStore>) -> Self {
        Self { dialer, state }
    }

    pub fn poll_and_match(
        &self,
        config: &ImapConfig,
        folders: &[String],
        requests: &[RemovalRequest],
        thread_map: &HashMap<String, i64>,
        store: Option<&dyn ReplyStore>,
    ) -> Result<Vec<MatchedMessage>, ImapError> {
        let owned_state = MemoryHwmStore::new();
        let state: &dyn HwmStore = self.state.unwrap_or(&owned_state);
        let staged = StagingHwmStore::new(Some(state));
        let messages = poll_folders(config, folders, self.dialer, &staged)?;
        let matched = match_reply_to_request(&messages.unwrap_or_default(), requests, thread_map);
        if let Some(store) = store {
            for reply in &matched {
                store
                    .insert(reply, &parse_email_body(&reply.message.body, 200))
                    .map_err(|cause| {
                        ImapError::new(format!("email: persist inbox reply: {cause}"))
                    })?;
            }
        }
        staged.commit().map_err(|cause| {
            ImapError::new(format!("email: commit inbox high-water mark: {cause}"))
        })?;
        Ok(matched)
    }
}
