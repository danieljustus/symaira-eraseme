//! Scheduler file generation, ported from `internal/scheduler/scheduler.go`.
//!
//! This module intentionally keeps scheduling concerns separate from the
//! deadlines/tick engine: the generated jobs invoke the exact `symeraseme`
//! binary selected at generation time, never a `PATH`-resolved name.
//!
//! [`generate`], [`write_files`] and legacy-Python unit detection
//! ([`detect_legacy_python_unit`], [`detect_legacy_python_units`],
//! [`scan_legacy_python_units`]) are ported in this slice. Install, uninstall
//! and status (the process-executing half of the Go package, which shells
//! out to `crontab`/`launchctl`/`systemctl`) are a later migration slice; see
//! Task 5.4 in `docs/plans/2026-09-04-go-to-rust-implementation-plan.md`.

mod cron;
mod launchd;
mod systemd;

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};

const WRAPPER_DIR_PLACEHOLDER: &str = "__WRAPPER_DIR__";

/// A supported scheduler backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Cron,
    Launchd,
    Systemd,
}

impl Platform {
    /// Parses a platform name case-insensitively, matching Go's
    /// `strings.ToLower` normalization before comparison.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "cron" => Some(Self::Cron),
            "launchd" => Some(Self::Launchd),
            "systemd" => Some(Self::Systemd),
            _ => None,
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cron => "cron",
            Self::Launchd => "launchd",
            Self::Systemd => "systemd",
        })
    }
}

/// Selects launchd on macOS, systemd when available on Linux, and cron as
/// the portable fallback. Explicit platform selection is preferred by
/// callers that need deterministic output.
pub fn detect_platform() -> Platform {
    if cfg!(target_os = "macos") {
        return Platform::Launchd;
    }
    if cfg!(target_os = "linux") && which_systemctl().is_some() {
        return Platform::Systemd;
    }
    Platform::Cron
}

fn which_systemctl() -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join("systemctl"))
        .find(|candidate| is_executable_file(candidate))
}

/// Matches Go's `exec.LookPath`, which on Unix skips a `PATH` candidate that
/// exists but is not executable rather than accepting the first same-named
/// file it finds.
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Describes the schedule to generate. Default values select the Python
/// integration's defaults, except `binary_path`: an empty `binary_path`
/// resolves to the currently running executable so generated jobs never
/// depend on `PATH`.
#[derive(Debug, Clone)]
pub struct Config {
    pub platform: Option<Platform>,
    pub output_dir: String,
    pub tick_hour: i32,
    pub tick_minute: i32,
    pub poll_hours: Vec<i32>,
    pub project_dir: String,
    pub binary_path: String,
    pub venv_activate: String,
}

impl Default for Config {
    /// The schedule used by the Python implementation.
    fn default() -> Self {
        Self {
            platform: None,
            output_dir: "./schedules".to_string(),
            tick_hour: 10,
            tick_minute: 0,
            poll_hours: vec![8, 12, 16, 20],
            project_dir: String::new(),
            binary_path: String::new(),
            venv_activate: String::new(),
        }
    }
}

/// An error generating or writing scheduler files.
#[derive(Debug)]
pub enum SchedulerError {
    UnsupportedPlatform(String),
    InvalidTickTime,
    InvalidPollHour(i32),
    ResolveBinaryPath(io::Error),
    ResolveProjectDirectory(io::Error),
    EmptyOutputDirectory,
    ResolveOutputDirectory(io::Error),
    CreateOutputDirectory(io::Error),
    InvalidGeneratedFilename(String),
    WriteFile { path: PathBuf, source: io::Error },
    ResolveHomeDirectory(io::Error),
    ReadLegacyUnit { path: PathBuf, source: io::Error },
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform(platform) => write!(
                f,
                "unsupported platform: {platform} (choose cron, launchd, or systemd)"
            ),
            Self::InvalidTickTime => write!(f, "tick time must be a valid 24-hour time"),
            Self::InvalidPollHour(hour) => {
                write!(f, "poll hour must be between 0 and 23: {hour}")
            }
            Self::ResolveBinaryPath(source) => {
                write!(f, "resolve symeraseme executable: {source}")
            }
            Self::ResolveProjectDirectory(source) => {
                write!(f, "resolve project directory: {source}")
            }
            Self::EmptyOutputDirectory => write!(f, "output directory must not be empty"),
            Self::ResolveOutputDirectory(source) => {
                write!(f, "resolve output directory: {source}")
            }
            Self::CreateOutputDirectory(source) => {
                write!(f, "create output directory: {source}")
            }
            Self::InvalidGeneratedFilename(name) => {
                write!(f, "invalid generated filename: {name:?}")
            }
            Self::WriteFile { path, source } => {
                write!(f, "write {}: {source}", path.display())
            }
            Self::ResolveHomeDirectory(source) => {
                write!(f, "resolve home directory: {source}")
            }
            Self::ReadLegacyUnit { path, source } => {
                write!(f, "read legacy unit {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for SchedulerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ResolveBinaryPath(source)
            | Self::ResolveProjectDirectory(source)
            | Self::ResolveOutputDirectory(source)
            | Self::CreateOutputDirectory(source)
            | Self::WriteFile { source, .. }
            | Self::ResolveHomeDirectory(source)
            | Self::ReadLegacyUnit { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Returns an explicit path unchanged. With no override it resolves the
/// current executable, making the generated command independent of `PATH`
/// or the shell's working directory.
pub fn resolve_binary_path(explicit: &str) -> Result<String, SchedulerError> {
    if !explicit.is_empty() {
        return Ok(explicit.to_string());
    }
    let mut path = env::current_exe().map_err(SchedulerError::ResolveBinaryPath)?;
    if !path.is_absolute() {
        path = env::current_dir()
            .map_err(SchedulerError::ResolveBinaryPath)?
            .join(path);
    }
    Ok(lexically_clean(&path).to_string_lossy().into_owned())
}

/// Lexically cleans a path the way Go's `filepath.Clean` does: `.`
/// components are dropped; a `..` component is collapsed against an
/// immediately preceding `Normal` component when one exists; a `..` with no
/// such preceding component to cancel is kept literally if the path is
/// relative at that point, or dropped if it would climb above a root/prefix
/// (matching Go's "eliminate `..` elements that begin a rooted path").
///
/// This is used both for absolute paths (`resolve_binary_path`, the
/// `write_files` output root) and, via [`is_safe_relative_filename`], as the
/// basis for rejecting relative filenames that would escape `output_dir` —
/// callers must not skip the `..`-collapsing step before validating, or a
/// name like `"a/../../b"` slips through uncollapsed and is written one
/// level above the intended root.
fn lexically_clean(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                _ => out.push(component),
            },
            other => out.push(other),
        }
    }
    let mut cleaned = PathBuf::new();
    for component in &out {
        cleaned.push(component.as_os_str());
    }
    if cleaned.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        cleaned
    }
}

/// Creates deterministic scheduler files keyed by their relative filename.
/// The output contains wrappers, native unit definitions, and portable
/// install/uninstall helpers.
pub fn generate(cfg: &Config) -> Result<BTreeMap<String, String>, SchedulerError> {
    let platform = match cfg.platform {
        Some(platform) => platform,
        None => detect_platform(),
    };

    if cfg.tick_hour < 0 || cfg.tick_hour > 23 || cfg.tick_minute < 0 || cfg.tick_minute > 59 {
        return Err(SchedulerError::InvalidTickTime);
    }
    let poll_hours = if cfg.poll_hours.is_empty() {
        vec![8, 12, 16, 20]
    } else {
        cfg.poll_hours.clone()
    };
    for &hour in &poll_hours {
        if !(0..=23).contains(&hour) {
            return Err(SchedulerError::InvalidPollHour(hour));
        }
    }
    let binary_path = resolve_binary_path(&cfg.binary_path)?;
    let project_dir = if cfg.project_dir.is_empty() {
        env::current_dir()
            .map_err(SchedulerError::ResolveProjectDirectory)?
            .to_string_lossy()
            .into_owned()
    } else {
        cfg.project_dir.clone()
    };

    Ok(match platform {
        Platform::Cron => cron::generate(cfg, &binary_path, &project_dir, &poll_hours),
        Platform::Launchd => launchd::generate(cfg, &binary_path, &project_dir, &poll_hours),
        Platform::Systemd => systemd::generate(cfg, &binary_path, &project_dir, &poll_hours),
    })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn wrapper(command: &[&str], project_dir: &str, venv: &str) -> String {
    let mut text = String::new();
    text.push_str("#!/usr/bin/env bash\n");
    text.push_str("# Symaira EraseMe scheduled task wrapper\n");
    text.push_str("# Generated by symeraseme generate-scheduler (Go)\n");
    text.push_str("set -euo pipefail\n\n");
    if !venv.is_empty() {
        text.push_str("source ");
        text.push_str(&shell_quote(venv));
        text.push_str("\n\n");
    }
    text.push_str("export SYMERASEME_HEADLESS=1\n");
    text.push_str("cd ");
    text.push_str(&shell_quote(project_dir));
    text.push_str("\n\nexec");
    for arg in command {
        text.push(' ');
        text.push_str(&shell_quote(arg));
    }
    text.push('\n');
    text
}

fn poll_wrapper(binary_path: &str, project_dir: &str, venv: &str, poll_hours: &[i32]) -> String {
    let mut text = String::new();
    text.push_str("#!/usr/bin/env bash\n");
    text.push_str("# Symaira EraseMe scheduled inbox poll wrapper\n");
    text.push_str("# Generated by symeraseme generate-scheduler (Go)\n");
    text.push_str("set -euo pipefail\n\n");
    if !venv.is_empty() {
        text.push_str("source ");
        text.push_str(&shell_quote(venv));
        text.push('\n');
    }
    text.push_str("export SYMERASEME_HEADLESS=1\n");
    text.push_str("cd ");
    text.push_str(&shell_quote(project_dir));
    text.push_str("\n\n");
    text.push_str("CURRENT_HHMM=$(date +%H:%M)\n");
    text.push_str("case \"$CURRENT_HHMM\" in\n");
    for (index, hour) in poll_hours.iter().enumerate() {
        if index > 0 {
            text.push('|');
        }
        text.push_str(&format!("{hour:02}:00"));
    }
    text.push_str(")\n    exec ");
    text.push_str(&shell_quote(binary_path));
    text.push_str(" poll-inbox --output json\n    ;;\nesac\n");
    text
}

/// Writes generated files in sorted order and returns absolute paths. Only
/// relative paths are accepted, preventing a caller from escaping
/// `output_dir`.
pub fn write_files(
    output_dir: &str,
    files: &BTreeMap<String, String>,
) -> Result<Vec<PathBuf>, SchedulerError> {
    if output_dir.is_empty() {
        return Err(SchedulerError::EmptyOutputDirectory);
    }
    let root = if Path::new(output_dir).is_absolute() {
        PathBuf::from(output_dir)
    } else {
        env::current_dir()
            .map_err(SchedulerError::ResolveOutputDirectory)?
            .join(output_dir)
    };
    let root = lexically_clean(&root);
    fs::create_dir_all(&root).map_err(SchedulerError::CreateOutputDirectory)?;

    for name in files.keys() {
        if !is_safe_relative_filename(name) {
            return Err(SchedulerError::InvalidGeneratedFilename(name.clone()));
        }
    }

    let mut written = Vec::with_capacity(files.len());
    for (name, contents) in files {
        let path = root.join(clean_relative(name));
        let mode: u32 = if name.ends_with(".sh") { 0o755 } else { 0o644 };
        write_with_mode(&path, contents.as_bytes(), mode).map_err(|source| {
            SchedulerError::WriteFile {
                path: path.clone(),
                source,
            }
        })?;
        written.push(path);
    }
    Ok(written)
}

fn is_safe_relative_filename(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let path = Path::new(name);
    // `has_root` alone misses a Windows drive-relative prefix like `C:temp`
    // (a `Prefix` component with no following root), which `is_absolute`
    // also misses since it requires *both* a prefix and a root; reject
    // either shape rather than relying on `is_absolute` alone.
    if path.has_root() || path.components().any(|c| matches!(c, Component::Prefix(_))) {
        return false;
    }
    let clean = lexically_clean(path);
    clean != Path::new(".") && clean != Path::new("..") && !clean.starts_with("..")
}

fn clean_relative(name: &str) -> PathBuf {
    lexically_clean(Path::new(name))
}

/// Writes `contents` at a temporary name in `path`'s parent directory, sets
/// its permission bits, then renames it into place — so `path` never appears
/// at its final name with a mode other than the intended one, unlike
/// `fs::write` followed by a separate `fs::set_permissions` call.
fn write_with_mode(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let mut temporary = match parent {
        Some(parent) => tempfile::NamedTempFile::new_in(parent)?,
        None => tempfile::NamedTempFile::new()?,
    };
    temporary.write_all(contents)?;
    temporary.flush()?;
    tighten_permissions(temporary.path(), mode)?;
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|error| error.error)
}

fn tighten_permissions(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

const LEGACY_MARKER: &str = "Generated by symeraseme generate-scheduler";

/// The known native unit filenames the Go-generated scheduler ever installs,
/// grouped launchd-first then systemd, matching `legacyNames` in
/// `internal/scheduler/scheduler.go`. [`scan_legacy_python_units`] slices
/// this by platform rather than duplicating the literals.
const LEGACY_NAMES: [&str; 9] = [
    "com.symeraseme.tick.plist",
    "com.symeraseme.poll.plist",
    "com.symeraseme.rescan.plist",
    "symeraseme-tick.service",
    "symeraseme-tick.timer",
    "symeraseme-poll.service",
    "symeraseme-poll.timer",
    "symeraseme-rescan.service",
    "symeraseme-rescan.timer",
];

/// Describes an existing scheduler definition that must be explicitly
/// replaced before installation. Existing known unit paths are reported even
/// when their contents do not contain Python, preventing silent duplication
/// with manually edited legacy schedules.
///
/// `platform`/`kind`/`name` are only populated by [`scan_legacy_python_units`],
/// which already knows which native location and unit family it scanned;
/// [`detect_legacy_python_units`] leaves them at their `None`/empty defaults,
/// matching the Go zero-value `LegacyUnit{Path: ..., IsPython: ..., Reason:
/// ...}` construction in `DetectLegacyPythonUnits`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyUnit {
    pub platform: Option<Platform>,
    pub kind: String,
    pub name: String,
    pub path: PathBuf,
    pub is_python: bool,
    pub reason: String,
}

/// Reports whether `content` carries a marker associated with the Python
/// scheduler integration, or the pre-Go marker comment without the `(Go)`
/// suffix the Rust/Go wrappers always append.
fn is_python_scheduler_content(content: &str) -> bool {
    const MARKERS: [&str; 5] = [
        "python",
        "site-packages",
        "symeraseme.core.scheduler",
        "uv run",
        "venv/bin/activate",
    ];
    let lower = content.to_lowercase();
    if MARKERS.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    content.contains(LEGACY_MARKER) && !content.contains(&format!("{LEGACY_MARKER} (Go)"))
}

/// Reports whether a file contains markers associated with the Python
/// scheduler. It is content-based and safe for missing files.
pub fn detect_legacy_python_unit(path: &Path) -> io::Result<bool> {
    match fs::read(path) {
        Ok(bytes) => Ok(is_python_scheduler_content(&String::from_utf8_lossy(
            &bytes,
        ))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

/// A convenience scanner for caller-provided paths; missing paths and
/// non-Python content are silently skipped rather than reported.
pub fn detect_legacy_python_units(paths: &[PathBuf]) -> io::Result<Vec<LegacyUnit>> {
    let mut units = Vec::new();
    for path in paths {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        if !is_python_scheduler_content(&String::from_utf8_lossy(&bytes)) {
            continue;
        }
        units.push(LegacyUnit {
            platform: None,
            kind: String::new(),
            name: String::new(),
            path: path.clone(),
            is_python: true,
            reason: "existing Python scheduler unit; explicit replacement required".to_string(),
        });
    }
    Ok(units)
}

#[cfg(windows)]
const HOME_ENV_VAR: &str = "USERPROFILE";
#[cfg(not(windows))]
const HOME_ENV_VAR: &str = "HOME";

/// Matches `os.UserHomeDir()`'s Unix/Windows behavior: `$HOME` (or
/// `%USERPROFILE%` on Windows), erroring when unset or empty.
fn resolve_home_dir() -> io::Result<PathBuf> {
    match env::var_os(HOME_ENV_VAR) {
        Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        _ => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("${HOME_ENV_VAR} is not defined"),
        )),
    }
}

/// Scans the native per-user unit locations for `platform` (or the
/// autodetected platform when `None`) under `home` (or the real user home
/// directory when `None`). Existing known Symaira unit names are returned as
/// replacement candidates; `is_python` distinguishes units whose contents
/// explicitly point at the Python runtime. Cron has no native unit files —
/// its entries are inspected by install/status instead — so this always
/// returns an empty list for [`Platform::Cron`].
pub fn scan_legacy_python_units(
    home: Option<&str>,
    platform: Option<Platform>,
) -> Result<Vec<LegacyUnit>, SchedulerError> {
    let home_dir = match home {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => resolve_home_dir().map_err(SchedulerError::ResolveHomeDirectory)?,
    };
    let platform = platform.unwrap_or_else(detect_platform);
    let (root, names, kind): (PathBuf, &[&str], &str) = match platform {
        Platform::Launchd => (
            home_dir.join("Library").join("LaunchAgents"),
            &LEGACY_NAMES[0..3],
            "launchd",
        ),
        Platform::Systemd => (
            home_dir.join(".config").join("systemd").join("user"),
            &LEGACY_NAMES[3..9],
            "systemd",
        ),
        Platform::Cron => return Ok(Vec::new()),
    };

    let mut units = Vec::with_capacity(names.len());
    for &name in names {
        let path = root.join(name);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(SchedulerError::ReadLegacyUnit { path, source }),
        };
        let is_python = is_python_scheduler_content(&String::from_utf8_lossy(&bytes));
        let reason = if is_python {
            "existing Python scheduler unit; explicit replacement required"
        } else {
            "existing Symaira scheduler unit; explicit replacement required"
        };
        units.push(LegacyUnit {
            platform: Some(platform),
            kind: kind.to_string(),
            name: name.to_string(),
            path,
            is_python,
            reason: reason.to_string(),
        });
    }
    Ok(units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn generate_rejects_unsupported_platform() {
        let cfg = Config {
            platform: None,
            ..Config::default()
        };
        assert!(Platform::parse("windows").is_none());
        let mut cfg = cfg;
        cfg.project_dir = "/srv".to_string();
        cfg.binary_path = "/bin/symeraseme".to_string();
        // Platform::parse returning None for an unsupported name is what the
        // CLI layer surfaces as SchedulerError::UnsupportedPlatform; this
        // module's own `generate` only ever receives an already-parsed
        // Option<Platform>, so the unsupported-name path is exercised at the
        // CLI boundary (Task 8.4), not here.
        let _ = generate(&cfg);
    }

    #[test]
    fn generate_rejects_invalid_tick_time() {
        let cfg = Config {
            tick_hour: 24,
            project_dir: "/srv".to_string(),
            binary_path: "/bin/symeraseme".to_string(),
            ..Config::default()
        };
        assert!(matches!(
            generate(&cfg),
            Err(SchedulerError::InvalidTickTime)
        ));
    }

    #[test]
    fn generate_rejects_invalid_poll_hour() {
        let cfg = Config {
            poll_hours: vec![25],
            project_dir: "/srv".to_string(),
            binary_path: "/bin/symeraseme".to_string(),
            ..Config::default()
        };
        assert!(matches!(
            generate(&cfg),
            Err(SchedulerError::InvalidPollHour(25))
        ));
    }

    #[test]
    fn resolve_binary_path_default_is_absolute() {
        let path = resolve_binary_path("").expect("resolve default binary path");
        assert!(Path::new(&path).is_absolute());
    }

    #[test]
    fn write_files_sorts_and_sets_wrapper_modes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let out = temp.path().join("nested").join("schedules");
        let mut files = BTreeMap::new();
        files.insert("z.txt".to_string(), "z".to_string());
        files.insert("a.sh".to_string(), "#!/bin/sh\n".to_string());
        let written = write_files(out.to_str().unwrap(), &files).expect("write files");
        assert_eq!(written[0].file_name().unwrap(), "a.sh");
        assert_eq!(written[1].file_name().unwrap(), "z.txt");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let sh_mode = fs::metadata(&written[0]).unwrap().permissions().mode() & 0o777;
            let txt_mode = fs::metadata(&written[1]).unwrap().permissions().mode() & 0o777;
            assert_eq!(sh_mode, 0o755);
            assert_eq!(txt_mode, 0o644);
        }

        let mut traversal = BTreeMap::new();
        traversal.insert("../escape".to_string(), "no".to_string());
        assert!(matches!(
            write_files(out.to_str().unwrap(), &traversal),
            Err(SchedulerError::InvalidGeneratedFilename(_))
        ));
    }

    #[test]
    fn is_safe_relative_filename_rejects_every_traversal_shape() {
        // The uncollapsed multi-component case a prior version of this check
        // missed: "foo/../../outside.txt" lexically cleans to
        // "../outside.txt", which must be rejected exactly like the
        // single-component "../escape" case already was.
        assert!(!is_safe_relative_filename("foo/../../outside.txt"));
        assert!(!is_safe_relative_filename("a/../../b"));
        assert!(!is_safe_relative_filename("../escape"));
        assert!(!is_safe_relative_filename(".."));
        assert!(!is_safe_relative_filename(""));
        assert!(!is_safe_relative_filename("/etc/passwd"));
        // A resolvable ".." must still be accepted, matching Go's
        // filepath.Clean("a/../b") == "b".
        assert!(is_safe_relative_filename("a/../b"));
        // A deeper resolvable case: every ".." here cancels a preceding
        // real segment, matching Go's filepath.Clean("a/b/../../c") == "c".
        assert!(is_safe_relative_filename("a/b/../../c"));
        assert!(is_safe_relative_filename("install.sh"));
    }

    #[test]
    #[cfg(windows)]
    fn is_safe_relative_filename_rejects_windows_rooted_and_drive_relative_forms() {
        // Rooted with no prefix: `is_absolute()` is false for this shape,
        // so a check based on `is_absolute()` alone would wrongly accept it.
        assert!(!is_safe_relative_filename(r"\Windows\System32\evil.sh"));
        // Drive-relative with no root: also `is_absolute() == false`.
        assert!(!is_safe_relative_filename(r"C:temp\evil.sh"));
        assert!(!is_safe_relative_filename(r"C:\Windows\evil.sh"));
    }

    #[test]
    fn write_files_rejects_multi_component_traversal_end_to_end() {
        let temp = tempfile::tempdir().expect("tempdir");
        let out = temp.path().join("schedules");
        let mut files = BTreeMap::new();
        files.insert("foo/../../escape.txt".to_string(), "no".to_string());
        assert!(matches!(
            write_files(out.to_str().unwrap(), &files),
            Err(SchedulerError::InvalidGeneratedFilename(_))
        ));
        assert!(!temp.path().join("escape.txt").exists());
    }

    #[test]
    fn write_files_rejects_empty_output_directory() {
        let files = BTreeMap::new();
        assert!(matches!(
            write_files("", &files),
            Err(SchedulerError::EmptyOutputDirectory)
        ));
    }

    #[test]
    fn write_files_reports_create_directory_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let blocked = temp.path().join("blocked");
        fs::write(&blocked, b"not a directory").expect("create blocking file");
        let files = BTreeMap::new();
        let result = write_files(blocked.to_str().unwrap(), &files);
        assert!(matches!(
            result,
            Err(SchedulerError::CreateOutputDirectory(_))
        ));
        let error = result.unwrap_err();
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    #[cfg(unix)]
    fn write_files_reports_write_failure() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("tempdir");
        let out = temp.path().join("schedules");
        fs::create_dir_all(&out).expect("create output dir");
        fs::set_permissions(&out, fs::Permissions::from_mode(0o555)).expect("make read-only");
        let mut files = BTreeMap::new();
        files.insert("a.sh".to_string(), "#!/bin/sh\n".to_string());
        let result = write_files(out.to_str().unwrap(), &files);
        // Restore write access so the tempdir can clean itself up.
        fs::set_permissions(&out, fs::Permissions::from_mode(0o755)).expect("restore access");
        let error = match result {
            Err(SchedulerError::WriteFile { source, .. }) => source,
            other => panic!("expected SchedulerError::WriteFile, got {other:?}"),
        };
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn write_with_mode_never_leaves_a_wrong_mode_window() {
        // A regression guard for the TOCTOU pattern this function replaced
        // (fs::write then a separate fs::set_permissions call): the file
        // must not exist at its final path until it already carries the
        // intended mode.
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("script.sh");
        write_with_mode(&path, b"#!/bin/sh\n", 0o755).expect("write with mode");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
        assert_eq!(fs::read(&path).unwrap(), b"#!/bin/sh\n");
    }
}
