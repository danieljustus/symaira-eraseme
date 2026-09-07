use super::RegistryError;
use super::loader::load_from_dir;
use flate2::read::GzDecoder;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tar::Archive;

const MAX_COMPRESSED_BYTES: usize = 64 << 20;
const MAX_EXPANDED_BYTES: u64 = 64 << 20;
const MAX_FILE_BYTES: u64 = 1 << 20;
const MAX_FILES: usize = 4_096;
const MAX_ARCHIVE_ENTRIES: usize = 8_192;
const MAX_PATH_BYTES: usize = 4_096;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The release artifact URL used when `sync` receives an empty URL.
pub const DEFAULT_SYNC_URL: &str =
    "https://github.com/danieljustus/symaira-eraseme/releases/latest/download/registry.tar.gz";

/// A transport response used by `sync_with_transport`, allowing deterministic
/// loopback test servers without weakening production URL validation.
pub struct SyncResponse {
    pub status: u16,
    pub body: Box<dyn Read + Send>,
}

pub trait SyncTransport {
    fn get(&self, url: &str, timeout: Duration) -> Result<SyncResponse, RegistryError>;
}

struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .timeout_connect(Some(REQUEST_TIMEOUT))
            .timeout_recv_response(Some(REQUEST_TIMEOUT))
            .timeout_recv_body(Some(REQUEST_TIMEOUT))
            .max_redirects(10)
            .max_redirects_will_error(true)
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl SyncTransport for UreqTransport {
    fn get(&self, url: &str, timeout: Duration) -> Result<SyncResponse, RegistryError> {
        let mut response = self
            .agent
            .get(url)
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(|_| transport_error())?;
        let status = response.status().as_u16();
        let body = read_bounded(&mut response.body_mut().as_reader(), MAX_COMPRESSED_BYTES)?;
        Ok(SyncResponse {
            status,
            body: Box::new(Cursor::new(body)),
        })
    }
}

/// Downloads and atomically installs a validated registry archive.
pub fn sync(url: &str, destination: impl AsRef<Path>) -> Result<(), RegistryError> {
    let url = if url.is_empty() {
        DEFAULT_SYNC_URL
    } else {
        url
    };
    sync_with_transport(url, destination, &UreqTransport::new())
}

/// Testable sync entry point. Non-HTTPS URLs are accepted only for loopback
/// hosts, so test transports cannot turn the production route into HTTP.
pub fn sync_with_transport(
    url: &str,
    destination: impl AsRef<Path>,
    transport: &dyn SyncTransport,
) -> Result<(), RegistryError> {
    validate_url(url)?;
    let destination = destination.as_ref();
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    let staging = tempfile::Builder::new()
        .prefix(".registry-sync-")
        .tempdir_in(parent)
        .map_err(|source| io_error(parent, source))?;
    set_mode(staging.path(), 0o700)?;

    let mut response = transport.get(url, REQUEST_TIMEOUT)?;
    if response.status != 200 {
        return Err(validation(
            "sync",
            format!("download returned HTTP {}", response.status),
        ));
    }
    let compressed = read_bounded(&mut response.body, MAX_COMPRESSED_BYTES)?;
    let decoder = GzDecoder::new(Cursor::new(compressed));
    extract_archive(decoder, staging.path())?;
    load_from_dir(staging.path())?;

    replace_directory(destination, staging.path())
}

fn validate_url(raw: &str) -> Result<(), RegistryError> {
    let (scheme, remainder) = raw
        .split_once("://")
        .ok_or_else(|| validation("sync", "invalid URL"))?;
    let authority = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map_or(rest, |(host, _)| host)
    } else {
        authority
            .rsplit_once(':')
            .map_or(authority, |(host, port)| {
                if port.bytes().all(|byte| byte.is_ascii_digit()) {
                    host
                } else {
                    authority
                }
            })
    };
    if host.is_empty() {
        return Err(validation("sync", "invalid URL"));
    }
    if scheme == "https" {
        return Ok(());
    }
    if scheme == "http"
        && (host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .map(|address| address.is_loopback())
                .unwrap_or(false))
    {
        return Ok(());
    }
    Err(validation(
        "sync",
        "HTTPS is required (except loopback test URLs)",
    ))
}

fn read_bounded(reader: &mut dyn Read, limit: usize) -> Result<Vec<u8>, RegistryError> {
    let mut bytes = Vec::with_capacity(limit.min(64 << 10));
    let mut buffer = [0_u8; 32 << 10];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|source| io_error(Path::new("sync"), source))?;
        if count == 0 {
            break;
        }
        if bytes.len() > limit - count.min(limit) || bytes.len() + count > limit {
            return Err(validation(
                "sync",
                format!("compressed body exceeds {limit} bytes"),
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(bytes)
}

fn extract_archive<R: Read>(reader: R, staging: &Path) -> Result<(), RegistryError> {
    let mut archive = Archive::new(reader);
    let mut entries = 0_usize;
    let mut files = 0_usize;
    let mut expanded = 0_u64;
    for item in archive
        .entries()
        .map_err(|source| io_error(Path::new("archive"), source))?
    {
        entries += 1;
        if entries > MAX_ARCHIVE_ENTRIES {
            return Err(validation(
                "sync",
                format!("archive entry limit {MAX_ARCHIVE_ENTRIES} exceeded"),
            ));
        }
        let mut entry = item.map_err(|source| io_error(Path::new("archive"), source))?;
        let path = safe_archive_path(entry.path_bytes().as_ref())?;
        let target = staging.join(&path);
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            if entry.size() != 0 {
                return Err(validation("sync", "directory entry contains data"));
            }
            fs::create_dir_all(&target).map_err(|source| io_error(&path, source))?;
            set_mode(&target, 0o700)?;
        } else if entry_type.is_file() {
            let size = entry.size();
            if size > MAX_FILE_BYTES {
                return Err(validation(
                    "sync",
                    format!("file size limit {MAX_FILE_BYTES} exceeded"),
                ));
            }
            files += 1;
            if files > MAX_FILES {
                return Err(validation(
                    "sync",
                    format!("file limit {MAX_FILES} exceeded"),
                ));
            }
            expanded = expanded
                .checked_add(size)
                .ok_or_else(|| validation("sync", "expanded byte counter overflowed"))?;
            if expanded > MAX_EXPANDED_BYTES {
                return Err(validation(
                    "sync",
                    format!("expanded archive limit {MAX_EXPANDED_BYTES} exceeded"),
                ));
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|source| io_error(&path, source))?;
            copy_exact(&mut entry, &mut file, size, &path)?;
            file.flush().map_err(|source| io_error(&path, source))?;
            set_mode(&target, 0o600)?;
        } else {
            return Err(validation("sync", "unsupported archive entry type"));
        }
    }
    Ok(())
}

fn copy_exact(
    reader: &mut dyn Read,
    writer: &mut File,
    size: u64,
    path: &Path,
) -> Result<(), RegistryError> {
    let mut limited = reader.take(size);
    let copied = io::copy(&mut limited, writer).map_err(|source| io_error(path, source))?;
    if copied != size {
        return Err(validation(
            "sync",
            "archive file ended before declared size",
        ));
    }
    Ok(())
}

fn safe_archive_path(raw: &[u8]) -> Result<PathBuf, RegistryError> {
    if raw.is_empty() || raw.len() > MAX_PATH_BYTES {
        return Err(validation("sync", "invalid archive path"));
    }
    let value =
        std::str::from_utf8(raw).map_err(|_| validation("sync", "archive path is not UTF-8"))?;
    if value.starts_with('/') || value.starts_with('\\') || value.contains('\0') {
        return Err(validation("sync", "absolute or invalid archive path"));
    }
    let mut path = PathBuf::new();
    for component in value.replace('\\', "/").split('/') {
        if component.is_empty() {
            continue;
        }
        if component == "." || component == ".." {
            return Err(validation("sync", "traversal archive path"));
        }
        path.push(component);
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(validation("sync", "unsafe archive path"));
    }
    if path.as_os_str().is_empty() {
        return Err(validation("sync", "empty archive path"));
    }
    Ok(path)
}

fn replace_directory(destination: &Path, staging: &Path) -> Result<(), RegistryError> {
    replace_directory_with_ops(destination, staging, &rename_path, &remove_all_path)
}

fn rename_path(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

fn remove_all_path(path: &Path) -> io::Result<()> {
    fs::remove_dir_all(path)
}

fn replace_directory_with_ops(
    destination: &Path,
    staging: &Path,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
    remove_all: &dyn Fn(&Path) -> io::Result<()>,
) -> Result<(), RegistryError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let backup_container = tempfile::Builder::new()
        .prefix(".registry-backup-")
        .tempdir_in(parent)
        .map_err(|source| io_error(parent, source))?;
    set_mode(backup_container.path(), 0o700)?;
    let backup_destination = backup_container.path().join("old");

    let had_old = match fs::symlink_metadata(destination) {
        Ok(_) => true,
        Err(source) if source.kind() == io::ErrorKind::NotFound => false,
        Err(source) => return Err(io_error(destination, source)),
    };
    if had_old {
        rename(destination, &backup_destination).map_err(|source| io_error(destination, source))?;
    }

    if let Err(install_source) = rename(staging, destination) {
        if !had_old {
            return Err(io_error(destination, install_source));
        }
        if let Err(rollback_source) = rename(&backup_destination, destination) {
            let retained = backup_container.keep();
            return Err(validation(
                "sync",
                format!(
                    "install staged registry failed: {install_source}; rollback failed: {rollback_source}; backup retained at {}",
                    retained.display()
                ),
            ));
        }
        let retained = backup_container.keep();
        if let Err(cleanup_source) = remove_all(&retained) {
            return Err(validation(
                "sync",
                format!(
                    "install staged registry failed: {install_source}; rollback succeeded; backup cleanup failed at {}: {cleanup_source}",
                    retained.display()
                ),
            ));
        }
        return Err(io_error(destination, install_source));
    }

    let retained = backup_container.keep();
    if let Err(source) = remove_all(&retained) {
        return Err(io_error(&retained, source));
    }
    Ok(())
}

fn set_mode(_path: &Path, _mode: u32) -> Result<(), RegistryError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(_mode))
            .map_err(|source| io_error(_path, source))?;
    }
    Ok(())
}

fn validation(field: impl Into<String>, message: impl Into<String>) -> RegistryError {
    RegistryError::Validation {
        field: field.into(),
        message: message.into(),
    }
}

fn io_error(path: &Path, source: io::Error) -> RegistryError {
    RegistryError::Io {
        path: path.to_owned(),
        source,
    }
}

fn transport_error() -> RegistryError {
    validation("sync", "transport request failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::{Arc, Mutex};

    fn make_tree(root: &Path, name: &str, content: &[u8]) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("state"), content).unwrap();
        path
    }

    #[test]
    fn replacement_does_not_touch_predictable_sibling_and_cleans_owned_backup() {
        let root = tempfile::tempdir().unwrap();
        let destination = make_tree(root.path(), "registry", b"old");
        let staging = make_tree(root.path(), "staging", b"new");
        let sibling = destination.with_extension("registry-backup");
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("sentinel"), b"keep").unwrap();

        replace_directory(&destination, &staging).unwrap();

        assert_eq!(fs::read(destination.join("state")).unwrap(), b"new");
        assert_eq!(fs::read(sibling.join("sentinel")).unwrap(), b"keep");
        let owned = fs::read_dir(root.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".registry-backup-")
            })
            .collect::<Vec<_>>();
        assert!(owned.is_empty(), "owned backups remain: {owned:?}");
    }

    #[test]
    fn failed_install_rolls_back_and_failed_rollback_retains_backup() {
        let root = tempfile::tempdir().unwrap();
        let destination = make_tree(root.path(), "registry", b"old");
        let staging = make_tree(root.path(), "staging", b"new");
        let calls = Cell::new(0);
        let retained_backup = Arc::new(Mutex::new(None));
        let retained_for_rename = Arc::clone(&retained_backup);
        let rename = |source: &Path, target: &Path| {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 1 {
                *retained_for_rename.lock().unwrap() = Some(target.to_owned());
                fs::rename(source, target)
            } else {
                Err(io::Error::other("forced replacement failure"))
            }
        };
        let remove = |path: &Path| fs::remove_dir_all(path);

        let error = replace_directory_with_ops(&destination, &staging, &rename, &remove)
            .expect_err("forced replacement failure unexpectedly succeeded");
        assert!(error.to_string().contains("rollback failed"));
        assert!(error.to_string().contains("backup retained"));
        assert!(!destination.exists());
        let backup = retained_backup.lock().unwrap().clone().unwrap();
        assert_eq!(fs::read(backup.join("state")).unwrap(), b"old");
        let container = backup.parent().unwrap();
        fs::remove_dir_all(container).unwrap();
    }
}
