use super::RegistryError;
use super::loader::load_from_dir;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, File, OpenOptions};
use flate2::bufread::GzDecoder;
use std::fs;
use std::io::{self, BufReader, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tar::Archive;

const MAX_COMPRESSED_BYTES: usize = 64 << 20;
const MAX_EXPANDED_BYTES: u64 = 64 << 20;
const MAX_FILE_BYTES: u64 = 1 << 20;
const MAX_FILES: usize = 4_096;
const MAX_ARCHIVE_ENTRIES: usize = 8_192;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_TAR_PADDING_BYTES: usize = 20 * 512;
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
    let decoder = GzDecoder::new(BufReader::new(Cursor::new(compressed)));
    let staging_dir = Dir::open_ambient_dir(staging.path(), ambient_authority())
        .map_err(|source| io_error(staging.path(), source))?;
    extract_archive(decoder, &staging_dir)?;
    load_from_dir(staging.path())?;

    replace_directory(destination, staging.path())
}

fn validate_url(raw: &str) -> Result<(), RegistryError> {
    let uri: ureq::http::Uri = raw.parse().map_err(|_| validation("sync", "invalid URL"))?;
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| validation("sync", "invalid URL"))?;
    let authority = uri
        .authority()
        .ok_or_else(|| validation("sync", "invalid URL"))?;
    if authority.as_str().contains('@') {
        return Err(validation("sync", "URL userinfo is not allowed"));
    }
    let host = authority.host();
    if host.is_empty() {
        return Err(validation("sync", "invalid URL"));
    }
    if scheme == "https" {
        return Ok(());
    }
    if scheme == "http"
        && (host == "localhost"
            || host
                .trim_matches(['[', ']'])
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

fn extract_archive(
    reader: GzDecoder<BufReader<Cursor<Vec<u8>>>>,
    staging: &Dir,
) -> Result<(), RegistryError> {
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
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            if entry.size() != 0 {
                return Err(validation("sync", "directory entry contains data"));
            }
            create_relative_dir_all(staging, &path)?;
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
            let (parent, name) = open_parent_dir(staging, &path)?;
            let mut options = OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No)
                .nonblock(true);
            let mut file = parent
                .open_with(name, &options)
                .map_err(|source| io_error(&path, source))?;
            copy_exact(&mut entry, &mut file, size, &path)?;
            file.flush().map_err(|source| io_error(&path, source))?;
            set_file_mode(&file, &path)?;
        } else {
            return Err(validation("sync", "unsupported archive entry type"));
        }
    }

    // Force checksum/trailer validation and reject bytes after the tar end.
    let mut decoder = archive.into_inner();
    let mut trailing_decompressed = 0_usize;
    let mut padding = [0_u8; 512];
    loop {
        let count = decoder
            .read(&mut padding)
            .map_err(|source| io_error(Path::new("archive"), source))?;
        if count == 0 {
            break;
        }
        trailing_decompressed = trailing_decompressed
            .checked_add(count)
            .ok_or_else(|| validation("sync", "tar padding counter overflowed"))?;
        if trailing_decompressed > MAX_TAR_PADDING_BYTES
            || padding[..count].iter().any(|byte| *byte != 0)
        {
            return Err(validation("sync", "decompressed bytes after tar end"));
        }
    }
    if !trailing_decompressed.is_multiple_of(512) {
        return Err(validation("sync", "non-canonical tar padding"));
    }
    let mut compressed = decoder.into_inner();
    let mut trailing_compressed = Vec::new();
    compressed
        .read_to_end(&mut trailing_compressed)
        .map_err(|source| io_error(Path::new("archive"), source))?;
    if !trailing_compressed.is_empty() {
        return Err(validation(
            "sync",
            "trailing compressed bytes are not allowed",
        ));
    }
    Ok(())
}

fn create_relative_dir_all(root: &Dir, path: &Path) -> Result<(), RegistryError> {
    let mut current = root.try_clone().map_err(|source| io_error(path, source))?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(validation("sync", "unsafe archive path"));
        };
        match current.open_dir_nofollow(name) {
            Ok(next) => current = next,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                current
                    .create_dir(name)
                    .map_err(|source| io_error(path, source))?;
                current = current
                    .open_dir_nofollow(name)
                    .map_err(|source| io_error(path, source))?;
            }
            Err(source) => return Err(io_error(path, source)),
        }
    }
    Ok(())
}

fn open_parent_dir(root: &Dir, path: &Path) -> Result<(Dir, PathBuf), RegistryError> {
    let name = path
        .file_name()
        .ok_or_else(|| validation("sync", "empty archive path"))?
        .to_owned();
    let parent_path = path.parent().unwrap_or_else(|| Path::new(""));
    create_relative_dir_all(root, parent_path)?;
    let mut parent = root.try_clone().map_err(|source| io_error(path, source))?;
    for component in parent_path.components() {
        let Component::Normal(component) = component else {
            return Err(validation("sync", "unsafe archive path"));
        };
        parent = parent
            .open_dir_nofollow(component)
            .map_err(|source| io_error(path, source))?;
    }
    Ok((parent, name.into()))
}

#[cfg(unix)]
fn set_file_mode(file: &File, path: &Path) -> Result<(), RegistryError> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(cap_std::fs::Permissions::from_std(
        std::fs::Permissions::from_mode(0o600),
    ))
    .map_err(|source| io_error(path, source))
}

#[cfg(not(unix))]
fn set_file_mode(_file: &File, _path: &Path) -> Result<(), RegistryError> {
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
    if value.contains(char::from(0))
        || value.contains('\\')
        || value.starts_with('/')
        || windows_absolute(value)
    {
        return Err(validation("sync", "absolute or invalid archive path"));
    }
    let mut path = PathBuf::new();
    for component in value.split('/') {
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

fn windows_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
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

    #[test]
    fn url_validation_uses_strict_authority_semantics() {
        for valid in [
            "https://example.test/registry.tar.gz",
            "http://localhost/test",
            "http://127.0.0.1:8080/test",
            "http://[::1]:8080/test",
        ] {
            assert!(validate_url(valid).is_ok(), "rejected {valid}");
        }
        for invalid in [
            "http://example.test/registry.tar.gz",
            r"http://evil.example\@127.0.0.1/test",
            r"http://127.0.0.1\@evil.example/test",
            "https://user@example.test/test",
            "not-a-url",
        ] {
            assert!(validate_url(invalid).is_err(), "accepted {invalid}");
        }
    }

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

    #[test]
    fn archive_paths_reject_backslashes_and_windows_absolute_forms_but_keep_safe_dots() {
        for path in [
            br"dir\\file.txt".as_slice(),
            br"C:/escape.txt".as_slice(),
            br"//server/share.txt".as_slice(),
            br"../escape.txt".as_slice(),
        ] {
            assert!(safe_archive_path(path).is_err(), "accepted {:?}", path);
        }
        assert_eq!(
            safe_archive_path(br"nested/safe..name.txt").unwrap(),
            PathBuf::from("nested/safe..name.txt")
        );
    }
}
