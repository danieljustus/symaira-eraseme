use super::RegistryError;
use super::loader::load_from_dir;
use flate2::read::GzDecoder;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tar::Archive;

const MAX_COMPRESSED_BYTES: usize = 64 << 20;
const MAX_EXPANDED_BYTES: u64 = 64 << 20;
const MAX_FILE_BYTES: u64 = 1 << 20;
const MAX_FILES: usize = 4_096;
const MAX_ARCHIVE_ENTRIES: usize = 8_192;
const MAX_PATH_BYTES: usize = 4_096;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A transport response used by `sync_with_transport`, allowing deterministic
/// loopback test servers without weakening production URL validation.
pub struct SyncResponse {
    pub status: u16,
    pub body: Box<dyn Read + Send>,
}

pub trait SyncTransport {
    fn get(&self, url: &str, timeout: Duration) -> Result<SyncResponse, RegistryError>;
}

struct CurlTransport;

impl SyncTransport for CurlTransport {
    fn get(&self, url: &str, timeout: Duration) -> Result<SyncResponse, RegistryError> {
        let temporary =
            tempfile::NamedTempFile::new().map_err(|source| io_error(Path::new("sync"), source))?;
        let path = temporary.path().to_owned();
        let seconds = timeout.as_secs().max(1).to_string();
        let output = Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--max-filesize",
                "67108864",
                "--max-time",
                &seconds,
                "--output",
            ])
            .arg(&path)
            .arg(url)
            .output()
            .map_err(|_| transport_error())?;
        if !output.status.success() {
            return Err(transport_error());
        }
        let mut file = File::open(&path).map_err(|source| io_error(Path::new("sync"), source))?;
        let body = read_bounded(&mut file, MAX_COMPRESSED_BYTES)?;
        Ok(SyncResponse {
            status: 200,
            body: Box::new(Cursor::new(body)),
        })
    }
}

/// Downloads and atomically installs a validated registry archive.
pub fn sync(url: &str, destination: impl AsRef<Path>) -> Result<(), RegistryError> {
    sync_with_transport(url, destination, &CurlTransport)
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
    let backup = destination.with_extension("registry-backup");
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|source| io_error(&backup, source))?;
    }
    let had_old = destination.exists();
    if had_old {
        fs::rename(destination, &backup).map_err(|source| io_error(destination, source))?;
    }
    if let Err(source) = fs::rename(staging, destination) {
        if had_old {
            let _ = fs::rename(&backup, destination);
        }
        return Err(io_error(destination, source));
    }
    if had_old {
        fs::remove_dir_all(&backup).map_err(|source| io_error(&backup, source))?;
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
