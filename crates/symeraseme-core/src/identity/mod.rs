//! Identity key and secret-resolution primitives.

mod keyring;
mod secrets;

pub use keyring::{FakeKeyring, KeyringBackend, KeyringError, OsKeyring, SERVICE_NAME, USERNAME};
pub use secrets::{
    KEY_LENGTH, MASTER_KEY_ENV, MasterKey, MasterKeyError, MasterKeyResolver, PASSPHRASE_SALT,
    SYMVAULT_PASSPHRASE_ENV, generate_master_key,
};
