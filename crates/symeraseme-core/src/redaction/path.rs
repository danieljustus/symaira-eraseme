use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File};
use std::fmt;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

pub const MAX_FILE_BYTES: u64 = 16 << 20;

#[derive(Debug)]
pub enum WorkspaceRootError {
    InvalidPath,
    RootUnavailable,
    Io(io::Error),
    NotRegularFile,
    FileTooLarge,
}
impl fmt::Display for WorkspaceRootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPath => "invalid workspace-relative path",
            Self::RootUnavailable => "workspace root is unavailable",
            Self::Io(_) => "workspace file read failed",
            Self::NotRegularFile => "workspace target is not a regular file",
            Self::FileTooLarge => "workspace file exceeds the configured limit",
        })
    }
}
impl std::error::Error for WorkspaceRootError {}

pub struct WorkspaceRoot {
    dir: Dir,
}
impl WorkspaceRoot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkspaceRootError> {
        let path = path.as_ref();
        let metadata =
            std::fs::symlink_metadata(path).map_err(|_| WorkspaceRootError::RootUnavailable)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(WorkspaceRootError::RootUnavailable);
        }
        let canonical =
            std::fs::canonicalize(path).map_err(|_| WorkspaceRootError::RootUnavailable)?;
        let dir = Dir::open_ambient_dir(canonical, cap_std::ambient_authority())
            .map_err(|_| WorkspaceRootError::RootUnavailable)?;
        Ok(Self { dir })
    }

    pub fn current() -> Result<Self, WorkspaceRootError> {
        Self::open(std::env::current_dir().map_err(|_| WorkspaceRootError::RootUnavailable)?)
    }

    pub fn read(&self, relative: &str) -> Result<Vec<u8>, WorkspaceRootError> {
        let components = validate_relative(relative)?;
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
        let mut options = cap_fs_ext::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let file = parent
            .open_with(&file_name, &options)
            .map_err(map_open_error)?;
        read_open_file(file)
    }
}

fn validate_relative(relative: &str) -> Result<Vec<PathBuf>, WorkspaceRootError> {
    if relative.is_empty()
        || relative.as_bytes().contains(&0)
        || relative.contains('\\')
        || relative.chars().any(char::is_control)
    {
        return Err(WorkspaceRootError::InvalidPath);
    }
    let raw_parts: Vec<&str> = relative.split('/').collect();
    if raw_parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(WorkspaceRootError::InvalidPath);
    }
    let raw_bytes = relative.as_bytes();
    if raw_bytes.len() >= 2 && raw_bytes[1] == b':' && raw_bytes[0].is_ascii_alphabetic() {
        return Err(WorkspaceRootError::InvalidPath);
    }
    let path = Path::new(relative);
    if path.is_absolute() {
        return Err(WorkspaceRootError::InvalidPath);
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
    if error.kind() == io::ErrorKind::InvalidInput {
        WorkspaceRootError::InvalidPath
    } else {
        WorkspaceRootError::Io(error)
    }
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
