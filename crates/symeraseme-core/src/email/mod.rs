//! Email policy and deterministic outbound MIME port from `internal/email`.
//!
//! The inbox transport uses [`ImapDialer`]/[`ImapSession`]; outbound messages
//! use [`SmtpTransport`]. Both leave network access at an adapter boundary.
//! The inbox policy decides which UID range is searched, which window is fetched
//! and when the high-water mark advances (DOM-006).
//!
//! Parity evidence: `rust-tests/parity/oracle/email` records every answer in
//! this module from the production Go package; `tests/email_parity.rs` replays
//! the recorded cases. The real network dialer is not part of this module —
//! `docs/rust-port-contract-matrix.md` tracks that remaining Go fallback.

pub mod config;
pub mod hwm;
pub mod imap;
pub mod oauth2;
pub mod parse;
pub mod policy;
pub mod service;
pub mod session;
pub mod smtp;
pub mod types;
pub mod wire;

pub use config::{
    ImapConfigOptions, load_imap_config, load_imap_config_with, load_imap_config_with_options,
    resolve_imap_oauth2,
};
pub use hwm::{HwmStore, MemoryHwmStore, StagingHwmStore};
pub use imap::ImapDialer as ImapTransportDialer;
pub use parse::{decode_header, parse_fetched_message};
pub use policy::{
    ERR_IMAP, match_reply_to_request, normalize_subject, parse_email_body, poll_folders,
    poll_inbox, redact_error, subject_matches,
};
pub use service::{InboxService, ReplyStore};
pub use session::{FetchedMessage, ImapDialer, ImapSession};
pub use smtp::{
    EmailMessage, SmtpError, SmtpTransport, build_mime_at, recipients, send_message_at,
};
pub use types::{
    ImapConfig, ImapError, MatchMethod, MatchedMessage, Message, OAuth2Token, RemovalRequest,
};
