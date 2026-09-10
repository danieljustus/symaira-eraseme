//! Identity key and secret-resolution primitives.

mod consent;
mod gate;
mod keyring;
mod resolve;
mod secrets;

pub use consent::{
    CONSENT_DIR_MODE, CONSENT_FILE_MODE, ConsentError, ConsentRecord, ConsentStore, ConsentToken,
    DEFAULT_TOKEN_TTL, default_consent_directory,
};
pub use gate::{ConsentOptions, read_consent_file};
pub use keyring::{FakeKeyring, KeyringBackend, KeyringError, OsKeyring, SERVICE_NAME, USERNAME};
pub use resolve::{
    ENV_PREFIX, KEYCHAIN_PREFIX, OsSecretBackend, SYMVAULT_PREFIX, SecretBackend,
    SecretBackendError, SecretResolutionError, SecretResolver, VAULT_PREFIX,
};
pub use secrets::{
    KEY_LENGTH, MASTER_KEY_ENV, MasterKey, MasterKeyError, MasterKeyResolver, PASSPHRASE_SALT,
    SYMVAULT_PASSPHRASE_ENV, generate_master_key,
};
