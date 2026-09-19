//! The IMAP transport boundary. This module implements the real network
//! dialer needed for parity testing the plain-TCP transport.
//!
//! The scripted test server lives at `tests/support/imap_server.rs` and is
//! included by the parity integration test (`tests/imap_transport_parity.rs`).

pub mod client;

pub use client::ImapDialer;
