use std::fs::File;
use std::io::Read;
use std::path::Path;
use symeraseme_core::config::{ConfigContext, resolve_storage};
use symeraseme_core::identity::MasterKeyResolver;
use symeraseme_core::storage::encryption::detect_version;
use symeraseme_core::storage::{EncryptedStoreError, Store, open_configured, set_master_key};

pub(crate) fn open(context: &ConfigContext) -> Result<Store, String> {
    let storage = resolve_storage(context).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&storage.db_dir)
        .map_err(|error| format!("eventstore: create database directory: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&storage.db_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("eventstore: secure database directory: {error}"))?;
    }

    if storage.encrypt_db || encrypted_header(&storage.db_path)? {
        let mut keys = MasterKeyResolver::from_process();
        let key = keys.resolve_existing().map_err(|error| error.to_string())?;
        set_master_key(*key.as_bytes());
    }

    open_configured(
        &storage.db_path,
        Some(&storage.temp_dir),
        storage.encrypt_db,
    )
    .map_err(|error| error.to_string())
}

#[must_use = "close the store and handle its finalization error"]
pub(crate) fn with_store<T>(
    store: Store,
    operation: impl FnOnce(&Store) -> T,
) -> (T, Result<(), EncryptedStoreError>) {
    let result = operation(&store);
    let close = store.close();
    (result, close)
}

fn encrypted_header(path: &Path) -> Result<bool, String> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("eventstore: inspect database encryption: {error}")),
    };
    let mut header = [0_u8; 33];
    let read = file
        .read(&mut header)
        .map_err(|error| format!("eventstore: inspect database encryption: {error}"))?;
    Ok(detect_version(&header[..read]).is_some())
}
