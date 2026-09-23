use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File};
use std::fmt;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

pub const MAX_FILE_BYTES: u64 = 16 << 20;

#[derive(Debug)]
pub enum WorkspaceRootError {
    /// Go's `ErrPathInvalid`.
    InvalidPath,
    /// Go's `ErrPathNullByte`.
    NullByte,
    /// Go's `ErrPathOutsideWorkspace`.
    OutsideWorkspace,
    RootUnavailable,
    Io(io::Error),
    NotRegularFile,
    /// Go's `ErrFileTooLarge`.
    FileTooLarge,
}
impl fmt::Display for WorkspaceRootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPath => "path is not a safe workspace-relative file name",
            Self::NullByte => "path contains a null byte",
            Self::OutsideWorkspace => "path is outside the MCP workspace",
            Self::RootUnavailable => "workspace root is unavailable",
            Self::Io(_) => "workspace file read failed",
            Self::NotRegularFile => "workspace target is not a regular file",
            Self::FileTooLarge => "workspace file exceeds the maximum size",
        })
    }
}
impl std::error::Error for WorkspaceRootError {}

pub struct WorkspaceRoot {
    dir: Dir,
    canonical: PathBuf,
}
impl WorkspaceRoot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkspaceRootError> {
        let path = path.as_ref();
        // Go's ReadWorkspaceFile wraps root resolution and capability-open
        // failures in workspaceReadError, so preserve its opaque display.
        let canonical = std::fs::canonicalize(path).map_err(WorkspaceRootError::Io)?;
        let probe = Dir::open_ambient_dir(&canonical, cap_std::ambient_authority())
            .map_err(WorkspaceRootError::Io)?;
        let pre_open = probe.metadata(".").map_err(WorkspaceRootError::Io)?;
        if !pre_open.is_dir() {
            return Err(WorkspaceRootError::Io(io::Error::new(
                io::ErrorKind::NotADirectory,
                "workspace root is not a directory",
            )));
        }
        let dir = Dir::open_ambient_dir(&canonical, cap_std::ambient_authority())
            .map_err(WorkspaceRootError::Io)?;
        let opened = dir.metadata(".").map_err(WorkspaceRootError::Io)?;
        if !same_file(&pre_open, &opened) {
            return Err(WorkspaceRootError::OutsideWorkspace);
        }
        Ok(Self { dir, canonical })
    }

    pub fn current() -> Result<Self, WorkspaceRootError> {
        Self::open(std::env::current_dir().map_err(WorkspaceRootError::Io)?)
    }

    pub fn read(&self, relative: &str) -> Result<Vec<u8>, WorkspaceRootError> {
        let relative = self.relative_path(Path::new(relative))?;
        let components = validate_relative(&relative)?;
        let mut parent = self.dir.try_clone().map_err(WorkspaceRootError::Io)?;
        let file_name = components
            .last()
            .ok_or(WorkspaceRootError::InvalidPath)?
            .clone();
        for component in &components[..components.len() - 1] {
            parent = parent
                .open_dir_nofollow(component)
                .map_err(map_open_error)?;
        }
        let metadata = parent
            .symlink_metadata(&file_name)
            .map_err(map_open_error)?;
        if !metadata.is_file() {
            return Err(WorkspaceRootError::NotRegularFile);
        }
        let mut options = cap_fs_ext::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let file = parent
            .open_with(&file_name, &options)
            .map_err(map_open_error)?;
        read_open_file(file)
    }
    fn relative_path(&self, path: &Path) -> Result<String, WorkspaceRootError> {
        if !path.is_absolute() {
            return path
                .to_str()
                .map(str::to_owned)
                .ok_or(WorkspaceRootError::InvalidPath);
        }
        // Go's `canonicalPath`: resolve symlinks when the target exists,
        // otherwise resolve the parent and re-attach the leaf; when even the
        // parent is missing, keep the cleaned absolute path unresolved.
        let absolute = if let Ok(canonical) = std::fs::canonicalize(path) {
            canonical
        } else {
            let parent = path.parent().ok_or(WorkspaceRootError::InvalidPath)?;
            match std::fs::canonicalize(parent) {
                Ok(parent) => parent.join(path.file_name().ok_or(WorkspaceRootError::InvalidPath)?),
                Err(_) => path.to_path_buf(),
            }
        };
        // A path outside the root — including one whose symlink-resolved form
        // diverges — is Go's `ErrPathOutsideWorkspace` (its `filepath.Rel`
        // fails or yields a `..` prefix).
        let relative = absolute
            .strip_prefix(&self.canonical)
            .map_err(|_| WorkspaceRootError::OutsideWorkspace)?;
        if relative.as_os_str().is_empty() {
            return Err(WorkspaceRootError::OutsideWorkspace);
        }
        let value = relative.to_str().ok_or(WorkspaceRootError::InvalidPath)?;
        Ok(value.replace(std::path::MAIN_SEPARATOR, "/"))
    }
}

fn validate_relative(relative: &str) -> Result<Vec<PathBuf>, WorkspaceRootError> {
    // Go checks the null byte first (`ErrPathNullByte`), then rejects unsafe
    // relative names (`ErrPathInvalid`) and escapes (`ErrPathOutsideWorkspace`).
    if relative.as_bytes().contains(&0) {
        return Err(WorkspaceRootError::NullByte);
    }
    if relative.is_empty() || relative.contains('\\') || relative.chars().any(char::is_control) {
        return Err(WorkspaceRootError::InvalidPath);
    }
    let raw_parts: Vec<&str> = relative.split('/').collect();
    if raw_parts.contains(&"..") {
        return Err(WorkspaceRootError::OutsideWorkspace);
    }
    if raw_parts.iter().any(|part| part.is_empty() || *part == ".") {
        return Err(WorkspaceRootError::InvalidPath);
    }
    let raw_bytes = relative.as_bytes();
    if raw_bytes.len() >= 2 && raw_bytes[1] == b':' && raw_bytes[0].is_ascii_alphabetic() {
        return Err(WorkspaceRootError::OutsideWorkspace);
    }
    let path = Path::new(relative);
    if path.is_absolute() {
        return Err(WorkspaceRootError::OutsideWorkspace);
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(value.to_owned().into()),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => return Err(WorkspaceRootError::InvalidPath),
        }
    }
    if components.is_empty() {
        return Err(WorkspaceRootError::InvalidPath);
    }
    Ok(components)
}

fn read_open_file(file: File) -> Result<Vec<u8>, WorkspaceRootError> {
    let metadata = file.metadata().map_err(WorkspaceRootError::Io)?;
    if !metadata.is_file() {
        return Err(WorkspaceRootError::NotRegularFile);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(WorkspaceRootError::FileTooLarge);
    }
    let mut data = Vec::with_capacity(metadata.len() as usize);
    file.take(
        MAX_FILE_BYTES
            .checked_add(1)
            .ok_or(WorkspaceRootError::FileTooLarge)?,
    )
    .read_to_end(&mut data)
    .map_err(WorkspaceRootError::Io)?;
    if data.len() as u64 > MAX_FILE_BYTES {
        return Err(WorkspaceRootError::FileTooLarge);
    }
    Ok(data)
}

fn map_open_error(error: io::Error) -> WorkspaceRootError {
    // Go wraps errors from Root.Lstat/OpenRoot/OpenFile in opaqueWorkspaceError.
    // InvalidInput here is an OS/cap-std failure after our explicit path
    // validation, not a Go ErrPathInvalid validation result.
    WorkspaceRootError::Io(error)
}

fn same_file(expected: &cap_std::fs::Metadata, opened: &cap_std::fs::Metadata) -> bool {
    use cap_fs_ext::MetadataExt;
    expected.dev() == opened.dev() && expected.ino() == opened.ino()
}
pub fn read_workspace_file(
    path: &Path,
    root: Option<&Path>,
) -> Result<Vec<u8>, WorkspaceRootError> {
    let root = match root {
        Some(path) => WorkspaceRoot::open(path)?,
        None => WorkspaceRoot::current()?,
    };
    let relative = path.to_str().ok_or(WorkspaceRootError::InvalidPath)?;
    root.read(relative)
}

pub fn read_workspace_text(path: &Path, root: Option<&Path>) -> Result<String, WorkspaceRootError> {
    String::from_utf8(read_workspace_file(path, root)?).map_err(|_| {
        WorkspaceRootError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "workspace file is not UTF-8",
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceRoot, WorkspaceRootError, map_open_error, read_workspace_file};
    use std::fs;
    use std::io;
    use std::path::Path;

    #[test]
    fn root_open_failures_match_go_opaque_error_text() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let missing = temp.path().join("missing-root");
        let regular_file = temp.path().join("root-file");
        fs::write(&regular_file, b"not a directory").expect("root fixture file");

        for path in [&missing, &regular_file] {
            let error = match read_workspace_file(Path::new("inside.txt"), Some(path)) {
                Ok(_) => panic!("{} unexpectedly read a workspace file", path.display()),
                Err(error) => error,
            };
            assert_eq!(error.to_string(), "workspace file read failed");
        }
    }

    #[test]
    fn cap_std_invalid_input_remains_an_opaque_read_error() {
        let error = map_open_error(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source-bound invalid-input probe",
        ));
        assert!(matches!(error, WorkspaceRootError::Io(_)));
        assert_eq!(error.to_string(), "workspace file read failed");
    }

    #[test]
    fn explicit_path_validation_stays_distinct_and_fail_closed() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let root = WorkspaceRoot::open(temp.path()).expect("workspace root");
        assert!(matches!(
            root.read("../outside.txt"),
            Err(WorkspaceRootError::OutsideWorkspace)
        ));
        assert!(matches!(
            root.read("bad\0name.txt"),
            Err(WorkspaceRootError::NullByte)
        ));
    }
}
