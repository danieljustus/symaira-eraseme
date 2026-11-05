//! Host-portable controls for the frozen native Windows consent ACL records.
//!
//! The Windows-native replay harness was removed with Windows support; the
//! retained Go records stay authenticated by the reader's own unit tests.
#![allow(dead_code, unused_imports)]

#[path = "support/frozen_consent_acl.rs"]
mod frozen_consent_acl;

#[test]
fn consent_acl_capture_fails_off_windows_when_requested() {
    frozen_consent_acl::reject_retired_modes();
}
