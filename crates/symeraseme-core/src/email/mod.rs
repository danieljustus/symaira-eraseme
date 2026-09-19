//! Inbox policy port (`internal/email`): UID high-water mark, UIDVALIDITY
//! handling, the bounded FETCH window, reply correlation and the IMAP
//! configuration contract.
//!
//! The transport is a trait boundary ([`ImapDialer`]/[`ImapSession`]), exactly
//! as in Go: the policy decides which UID range is searched, which window is
//! fetched and when the high-water mark advances. Register row DOM-006.
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
pub use types::{
    ImapConfig, ImapError, MatchMethod, MatchedMessage, Message, OAuth2Token, RemovalRequest,
};
