//! Conservative, resumable Python installation migration.
mod json;
use crate::scheduler::{self, Platform};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[cfg(any(windows, test))]
mod windows_case_fold;

/// Go's `migration.validateRoots`: required flags, absolute+cleaned paths, no
/// symlink components, a real source directory, no symlink destination, and
/// two directories that are neither equal nor nested.
fn validate_roots(source: &str, destination: &str) -> Result<(String, String), String> {
    if source.is_empty() || destination.is_empty() {
        return Err("source and destination directories are required".to_owned());
    }
    let source =
        absolute_dir(source).map_err(|error| format!("resolve source directory: {error}"))?;
    let destination = absolute_dir(destination)
        .map_err(|error| format!("resolve destination directory: {error}"))?;
    reject_symlink_components(&source)
        .map_err(|error| format!("source path is unsafe: {error}"))?;
    reject_symlink_components(&destination)
        .map_err(|error| format!("destination path is unsafe: {error}"))?;
    let info = std::fs::symlink_metadata(&source).map_err(|error| {
        format!(
            "stat source directory: lstat {source}: {}",
            go_errno_text(&error)
        )
    })?;
    if !info.is_dir() || info.file_type().is_symlink() {
        return Err("source must be a real directory".to_owned());
    }
    if let Ok(destination_info) = std::fs::symlink_metadata(&destination)
        && destination_info.file_type().is_symlink()
    {
        return Err("destination must not be a symlink".to_owned());
    }
    if source == destination
        || path_within(&source, &destination)
        || path_within(&destination, &source)
    {
        return Err("source and destination must be separate, non-nested directories".to_owned());
    }
    Ok((source, destination))
}

/// Go's `absoluteDir`: non-empty, absolute, cleaned — no symlink resolution.
fn absolute_dir(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("path must not be empty".to_owned());
    }
    let absolute = clean_absolute(path)?;
    Ok(absolute.to_string_lossy().into_owned())
}

/// Go's `rejectSymlinkComponents`: walk every component and refuse a symlink
/// outside the allowed darwin system roots; a missing component stops the
/// walk without an error (the later `Lstat` reports missing paths).
fn reject_symlink_components(path: impl AsRef<Path>) -> Result<(), String> {
    let absolute = clean_absolute(path)?;
    let mut current = PathBuf::new();
    let mut index = 0;
    for component in absolute.components() {
        current.push(component);
        if !matches!(component, Component::Normal(_)) {
            continue;
        }
        let text = current.to_string_lossy();
        let info = match std::fs::symlink_metadata(&current) {
            Ok(info) => info,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        if info.file_type().is_symlink() && (index > 0 || !is_allowed_system_symlink(&text)) {
            return Err(format!("symlink component: {text}"));
        }
        index += 1;
    }
    Ok(())
}

/// Go's `isAllowedSystemSymlink`: only the exact system roots at the first
/// component. Go guards this with `runtime.GOOS == "darwin"`; Rust's
/// `target_os` for that platform is spelled `macos`.
fn is_allowed_system_symlink(path: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        matches!(std::path::Path::new(path), p if matches!(
            p.to_str(),
            Some("/etc" | "/private" | "/tmp" | "/var")
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// Go's `pathWithin`: whether `candidate` sits at or under `root`.
fn path_within(root: &str, candidate: &str) -> bool {
    match (clean_absolute(root), clean_absolute(candidate)) {
        (Ok(root), Ok(candidate)) => {
            #[cfg(windows)]
            {
                path_prefix_casefold(&root, &candidate)
            }
            #[cfg(not(windows))]
            {
                candidate.starts_with(root)
            }
        }
        _ => false,
    }
}

// Kept executable on the host for exhaustive Go Unicode and component tests;
// actual Windows drive/UNC and filesystem behavior still needs native tests.
#[cfg(any(windows, test))]
fn path_prefix_casefold(root: &Path, candidate: &Path) -> bool {
    let mut candidates = candidate.components();
    root.components().all(|component| {
        candidates.next().is_some_and(|candidate| {
            match (
                component.as_os_str().to_str(),
                candidate.as_os_str().to_str(),
            ) {
                (Some(left), Some(right)) => {
                    let fold = |c| windows_case_fold::fold(if c == '/' { '\\' } else { c });
                    left.chars().map(fold).eq(right.chars().map(fold))
                }
                _ => false,
            }
        })
    })
}

/// Go's wrapped `os.Lstat` text: `lstat <path>: <errno>`. Rust attaches no
/// path, so the op and path are formatted here; errno text is Go's lowercase
/// spelling (ponytail: first-word lowercasing plus the recorded ENOENT case —
/// upgrade path: a full errno table).
fn go_errno_text(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        Some(2) => "no such file or directory".to_owned(),
        _ => {
            let text = error.to_string();
            let base = text.split(" (os error").next().unwrap_or(&text);
            let mut characters = base.chars();
            match characters.next() {
                Some(first) => first.to_lowercase().collect::<String>() + characters.as_str(),
                None => base.to_owned(),
            }
        }
    }
}

fn clean_absolute(path: impl AsRef<Path>) -> Result<PathBuf, String> {
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    let mut clean = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            other => clean.push(other),
        }
    }
    Ok(clean)
}
fn join(root: &str, name: &str) -> String {
    Path::new(root).join(name).to_string_lossy().into_owned()
}
fn exists(path: &str) -> bool {
    fs::symlink_metadata(path).is_ok()
}
fn overlap(a: &str, b: &str) -> bool {
    path_within(a, b) || path_within(b, a)
}
fn zero(n: &usize) -> bool {
    *n == 0
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct SecretReport {
    pub detected: bool,
    #[serde(skip_serializing_if = "zero")]
    pub reference_count: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub migratable: bool,
}
pub trait SecretStore {
    fn inspect(&self, source: &str) -> Result<SecretReport, String>;
    fn migrate(&self, destination: &str) -> Result<(), String>;
}
pub struct MetadataOnlySecretStore;
impl SecretStore for MetadataOnlySecretStore {
    fn inspect(&self, source: &str) -> Result<SecretReport, String> {
        Ok(
            if exists(&join(source, "identity.enc")) || exists(&join(source, "secret-refs.json")) {
                SecretReport {
                    detected: true,
                    description: "Python secrets are keyring-backed; values were not inspected"
                        .into(),
                    ..Default::default()
                }
            } else {
                SecretReport::default()
            },
        )
    }
    fn migrate(&self, _: &str) -> Result<(), String> {
        Err("secret migration requires an injected SecretStore; secret values are never copied by default".into())
    }
}
pub type BeforeItem<'a> = &'a dyn Fn(&Artifact) -> Result<(), String>;
pub struct Options<'a> {
    pub source_root: String,
    pub destination_root: String,
    pub source_config_root: String,
    pub destination_config_root: String,
    pub backup_dir: String,
    pub home_dir: String,
    pub platform: String,
    pub scheduler_source: String,
    pub scheduler_dest: String,
    pub binary_path: String,
    pub project_dir: String,
    pub scheduler_config: scheduler::Config,
    pub copy_secrets: bool,
    pub dry_run: bool,
    pub secret_store: Option<&'a dyn SecretStore>,
    pub before_item: Option<BeforeItem<'a>>,
}
impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            source_root: String::new(),
            destination_root: String::new(),
            source_config_root: String::new(),
            destination_config_root: String::new(),
            backup_dir: String::new(),
            home_dir: String::new(),
            platform: String::new(),
            scheduler_source: String::new(),
            scheduler_dest: String::new(),
            binary_path: String::new(),
            project_dir: String::new(),
            scheduler_config: scheduler::Config {
                output_dir: String::new(),
                ..Default::default()
            },
            copy_secrets: false,
            dry_run: false,
            secret_store: None,
            before_item: None,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Artifact {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    pub destination: String,
    pub reason: String,
}
#[derive(Debug, Default, Serialize)]
pub struct Detection {
    pub detected: bool,
    pub summary: String,
    pub reasons: Option<Vec<String>>,
    pub secret_store: SecretReport,
    pub artifacts: Option<Vec<Artifact>>,
}
#[derive(Debug, Serialize)]
pub struct ItemResult {
    pub artifact: Artifact,
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub detection: Detection,
    pub dry_run: bool,
    pub resumed: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub backup_dir: String,
    pub items: Option<Vec<ItemResult>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub complete: bool,
}
fn config_roots(o: &Options<'_>, s: &str, d: &str) -> Result<(String, String), String> {
    let s = if o.source_config_root.is_empty() {
        s.into()
    } else {
        absolute_dir(&o.source_config_root)
            .map_err(|e| format!("resolve source config directory: {e}"))?
    };
    let d = if o.destination_config_root.is_empty() {
        d.into()
    } else {
        absolute_dir(&o.destination_config_root)
            .map_err(|e| format!("resolve destination config directory: {e}"))?
    };
    if overlap(&s, &d) {
        return Err(
            "source and destination config directories must be separate, non-nested directories"
                .into(),
        );
    }
    if fs::symlink_metadata(&d).is_ok_and(|i| i.file_type().is_symlink()) {
        return Err("destination config directory must not be a symlink".into());
    }
    Ok((s, d))
}
fn platform(o: &Options<'_>) -> Result<Platform, String> {
    if o.platform.is_empty() {
        return Ok(scheduler::detect_platform());
    }
    match o.platform.as_str() {
        "cron" => Ok(Platform::Cron),
        "launchd" => Ok(Platform::Launchd),
        "systemd" => Ok(Platform::Systemd),
        p => Err(format!(
            "unsupported platform: {p} (choose cron, launchd, or systemd)"
        )),
    }
}
fn generated(o: &Options<'_>, d: &str) -> Result<BTreeMap<String, String>, String> {
    let mut c = o.scheduler_config.clone();
    c.platform = Some(platform(o)?);
    if c.output_dir.is_empty() {
        c.output_dir = join(d, "schedules")
    }
    if c.tick_hour == 0 && c.tick_minute == 0 {
        c.tick_hour = 10
    }
    if c.poll_hours.is_empty() {
        c.poll_hours = vec![8, 12, 16, 20]
    }
    if c.binary_path.is_empty() {
        c.binary_path = o.binary_path.clone()
    }
    if c.project_dir.is_empty() {
        c.project_dir = o.project_dir.clone()
    }
    scheduler::generate(&c).map_err(|e| e.to_string())
}
fn native_root(home: &str, p: Platform) -> String {
    match p {
        Platform::Launchd => join(home, "Library/LaunchAgents"),
        Platform::Systemd => join(home, ".config/systemd/user"),
        _ => String::new(),
    }
}
fn native_file(n: &str, p: Platform) -> bool {
    if p == Platform::Launchd {
        n.ends_with(".plist")
    } else {
        n.ends_with(".service") || n.ends_with(".timer")
    }
}
impl Detection {
    fn add(&mut self, id: &str, kind: &str, source: String, destination: String, reason: &str) {
        self.detected = true;
        self.reasons.get_or_insert_default().push(reason.into());
        self.artifacts.get_or_insert_default().push(Artifact {
            id: id.into(),
            kind: kind.into(),
            source,
            destination,
            reason: reason.into(),
        });
    }
}
pub fn detect(o: &Options<'_>) -> Result<Detection, String> {
    let (s, d) = validate_roots(&o.source_root, &o.destination_root)?;
    for (label, value) in [
        ("home", &o.home_dir),
        ("scheduler source", &o.scheduler_source),
        ("scheduler destination", &o.scheduler_dest),
        ("binary", &o.binary_path),
        ("project directory", &o.project_dir),
        ("scheduler output", &o.scheduler_config.output_dir),
    ] {
        if !value.is_empty() && !Path::new(value).is_absolute() {
            return Err(format!("{label} must be an absolute path"));
        }
    }
    let (cs, cd) = config_roots(o, &s, &d)?;
    for (label, path) in [
        ("source config", &cs),
        ("destination config", &cd),
        ("scheduler source", &o.scheduler_source),
        ("scheduler destination", &o.scheduler_dest),
        ("home", &o.home_dir),
    ] {
        if path.is_empty() {
            continue;
        }
        reject_symlink_components(path).map_err(|e| format!("{label} path is unsafe: {e}"))?;
        match fs::symlink_metadata(path) {
            Ok(i) if !i.is_dir() => return Err(format!("{label} must be a real directory")),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "inspect {label} directory: lstat {path}: {}",
                    go_errno_text(&e)
                ));
            }
            _ => {}
        }
    }
    if cs != s && overlap(&cs, &d) {
        return Err("source config directory must not overlap destination".into());
    }
    if cd != d && overlap(&cd, &s) {
        return Err("destination config directory must not overlap source".into());
    }
    let store = o.secret_store.unwrap_or(&MetadataOnlySecretStore);
    let mut secret = store
        .inspect(&s)
        .map_err(|e| format!("inspect secret store: {e}"))?;
    if !secret.detected && cs != s {
        secret = store
            .inspect(&cs)
            .map_err(|e| format!("inspect config secret store: {e}"))?
    }
    let mut result = Detection {
        secret_store: secret,
        ..Default::default()
    };
    for (id, kind, root, name, dest, name2, reason) in [
        (
            "event-db",
            "event-db",
            &s,
            "symeraseme.db",
            &d,
            "symeraseme.db",
            "Python-era event database found",
        ),
        (
            "config:config.toml",
            "config",
            &cs,
            "config.toml",
            &cd,
            "config.toml",
            "Python-era configuration found",
        ),
        (
            "config:.symeraseme.toml",
            "config",
            &cs,
            ".symeraseme.toml",
            &cd,
            ".symeraseme.toml",
            "Python-era configuration found",
        ),
        (
            "profile",
            "profile",
            &cs,
            "identity.enc",
            &cd,
            "identity.encrypted",
            "Python-era encrypted identity profile found",
        ),
    ] {
        let src = join(root, name);
        if exists(&src) {
            result.add(id, kind, src, join(dest, name2), reason)
        }
    }
    if result.secret_store.detected {
        let reason = result.secret_store.description.clone();
        result.add(
            "secret-store",
            "secret-store",
            "keyring://symeraseme".into(),
            "keyring://symeraseme".into(),
            &reason,
        )
    }
    let files = generated(o, &d)?;
    let p = platform(o)?;
    let src = if o.scheduler_source.is_empty() {
        join(&s, "schedules")
    } else {
        o.scheduler_source.clone()
    };
    let dst = if o.scheduler_dest.is_empty() {
        join(&d, "schedules")
    } else {
        o.scheduler_dest.clone()
    };
    for (src, dst, native) in [
        (src, dst, false),
        (
            native_root(&o.home_dir, p),
            native_root(&o.home_dir, p),
            true,
        ),
    ] {
        if native && (o.home_dir.is_empty() || src.is_empty()) {
            continue;
        }
        let names: Vec<_> = files
            .keys()
            .filter(|n| !native || native_file(n, p))
            .collect();
        let mut legacy = false;
        for n in &names {
            let path = join(&src, n);
            if exists(&path) {
                legacy |= scheduler::detect_legacy_python_unit(Path::new(&path)).map_err(|e| {
                    format!(
                        "inspect {} {path}: {}",
                        if native {
                            "native scheduler unit"
                        } else {
                            "scheduler file"
                        },
                        io_error(
                            if e.kind() == std::io::ErrorKind::IsADirectory {
                                "read"
                            } else {
                                "open"
                            },
                            &path,
                            e,
                        )
                    )
                })?;
            }
        }
        if legacy {
            for n in names {
                let path = join(&src, n);
                result.add(
                    &format!("scheduler:{}{n}", if native { "native:" } else { "" }),
                    "scheduler",
                    if exists(&path) { path } else { String::new() },
                    join(&dst, n),
                    if native {
                        "Python-era native scheduler unit found"
                    } else {
                        "Python-era generated scheduler artifact found"
                    },
                )
            }
        }
    }
    if let Some(a) = &mut result.artifacts {
        a.sort_by(|a, b| a.id.cmp(&b.id));
    }
    result.summary = if result.detected {
        format!(
            "Python-era Symaira EraseMe installation detected ({} migration item(s))",
            result.artifacts.as_ref().map_or(0, Vec::len)
        )
    } else {
        "No known Python-era Symaira EraseMe artifacts detected".into()
    };
    Ok(result)
}
/// Produces the read-only report. Mutation is performed separately by `run`.
pub fn dry_run(o: &Options<'_>) -> Result<Report, String> {
    let detection = detect(o)?;
    let items = detection.artifacts.as_ref().map(|a| {
        a.iter()
            .map(|a| ItemResult {
                artifact: a.clone(),
                status: "planned".into(),
                error: String::new(),
            })
            .collect()
    });
    Ok(Report {
        complete: !detection.detected,
        detection,
        dry_run: o.dry_run,
        items,
        ..Default::default()
    })
}

#[derive(Default, Serialize)]
struct State {
    version: isize,
    source_root: String,
    destination_root: String,
    backup_dir: String,
    items: Option<BTreeMap<String, String>>,
}
fn io_error(op: &str, path: impl AsRef<Path>, e: std::io::Error) -> String {
    format!("{op} {}: {}", path.as_ref().display(), go_errno_text(&e))
}
fn mkdir(path: impl AsRef<Path>) -> Result<(), String> {
    let path = path.as_ref();
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|e| io_error("mkdir", path, e))
}
fn file_mode(info: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        info.permissions().mode() & 0o777
    }
    #[cfg(windows)]
    {
        if info.permissions().readonly() {
            0o444
        } else {
            0o666
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = info;
        0o600
    }
}
fn write_atomic(path: impl AsRef<Path>, data: &[u8], mode: u32) -> Result<(), String> {
    use std::io::Write;
    let path = path.as_ref();
    reject_symlink_components(path).map_err(|e| format!("destination path is unsafe: {e}"))?;
    let parent = path.parent().ok_or("destination has no parent")?;
    mkdir(parent)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".migration-write-");
    #[cfg(not(windows))]
    let mut tmp = builder
        .tempfile_in(parent)
        .map_err(|e| io_error("open", path, e))?;
    // tempfile::persist on Windows resets all attributes to NORMAL. Create an
    // ordinary temporary file and rename it without discarding READONLY.
    #[cfg(windows)]
    let mut tmp = builder
        .make_in(parent, |temporary| {
            fs::OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(temporary)
        })
        .map_err(|e| io_error("open", path, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|e| io_error("chmod", path, e))?;
    }
    #[cfg(not(any(unix, windows)))]
    let _ = mode;
    tmp.write_all(data)
        .map_err(|e| io_error("write", path, e))?;
    tmp.as_file()
        .sync_all()
        .map_err(|e| io_error("sync", path, e))?;
    // Windows READONLY is a file attribute, not an ACL. Apply it to the private
    // file immediately before atomic publication, after fallible content writes.
    #[cfg(windows)]
    let mut permissions = {
        let mut permissions = tmp
            .as_file()
            .metadata()
            .map_err(|e| io_error("stat", tmp.path(), e))?
            .permissions();
        permissions.set_readonly(mode & 0o200 == 0);
        tmp.as_file()
            .set_permissions(permissions.clone())
            .map_err(|e| io_error("chmod", tmp.path(), e))?;
        permissions
    };
    #[cfg(not(windows))]
    tmp.persist(path)
        .map_err(|e| io_error("rename", path, e.error))?;
    #[cfg(windows)]
    if let Err(error) = fs::rename(tmp.path(), path) {
        // Only our already-open temporary file is made removable. Never clear
        // an existing destination's READONLY bit to force replacement.
        permissions.set_readonly(false);
        let _ = tmp.as_file().set_permissions(permissions);
        return Err(io_error("rename", path, error));
    }
    Ok(())
}
fn json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    // Go's MarshalIndent escapes HTML and the JavaScript line separators.
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    Ok(text
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .into_bytes())
}
fn write_state(path: &str, state: &State) -> Result<(), String> {
    write_atomic(path, &json_bytes(state)?, 0o600)
}
fn copy_one(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> Result<(), String> {
    let source = source.as_ref();
    reject_symlink_components(source).map_err(|e| format!("source path is unsafe: {e}"))?;
    let info = fs::symlink_metadata(source).map_err(|e| io_error("lstat", source, e))?;
    if !info.is_file() {
        return Err(format!(
            "source is not a regular file: {}",
            source.display()
        ));
    }
    let data = fs::read(source).map_err(|e| io_error("open", source, e))?;
    write_atomic(destination, &data, file_mode(&info))
}
fn copy_tree(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> Result<(), String> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    mkdir(destination)?;
    let mut entries = fs::read_dir(source)
        .map_err(|e| io_error("open", source, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let src = entry.path();
        let dst = destination.join(entry.file_name());
        let info = fs::symlink_metadata(&src).map_err(|e| io_error("lstat", &src, e))?;
        if info.file_type().is_symlink() {
            return Err(format!("refusing symlink in source: {}", src.display()));
        }
        if info.is_dir() {
            copy_tree(&src, &dst)?
        } else {
            copy_one(&src, &dst)?
        }
    }
    Ok(())
}
#[derive(Serialize)]
struct ExternalBackup {
    id: String,
    source: String,
    backup: String,
}
#[derive(Serialize)]
struct BackupManifest<'a> {
    version: u32,
    source: &'a str,
    artifacts: Option<Vec<&'a str>>,
    external: Option<Vec<ExternalBackup>>,
}
fn ensure_backup(
    backup: &str,
    source: &str,
    config: &str,
    artifacts: &[Artifact],
) -> Result<(), String> {
    let marker = join(backup, ".complete.json");
    match fs::symlink_metadata(&marker) {
        Ok(info) => {
            if !info.is_file() {
                return Err("backup completion marker is not a regular file".into());
            }
            let data = fs::read(&marker).map_err(|e| {
                format!(
                    "read backup completion marker: {}",
                    io_error("open", &marker, e)
                )
            })?;
            let valid = json::marker_matches(&data, source);
            if !valid {
                return Err("backup completion marker does not match this source".into());
            }
            return Ok(());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "inspect backup completion marker: {}",
                io_error("lstat", &marker, e)
            ));
        }
    }
    if exists(backup) {
        return Err(
            "backup directory exists without a complete marker; refusing to overwrite it".into(),
        );
    }
    copy_tree(source, join(backup, "source")).map_err(|e| format!("backup source: {e}"))?;
    if config != source && exists(config) {
        copy_tree(config, join(backup, "config-0"))
            .map_err(|e| format!("backup additional source {config}: {e}"))?
    }
    let mut external = Vec::new();
    for a in artifacts {
        if a.source.is_empty()
            || !exists(&a.source)
            || path_within(source, &a.source)
            || path_within(config, &a.source)
        {
            continue;
        }
        let name = Path::new(&a.source)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .replace("..", "_");
        let relative = format!("external/{:03}-{name}", external.len());
        copy_one(&a.source, join(backup, &relative))
            .map_err(|e| format!("backup {}: {e}", a.id))?;
        external.push(ExternalBackup {
            id: a.id.clone(),
            source: a.source.clone(),
            backup: relative,
        });
    }
    let manifest = BackupManifest {
        version: 1,
        source,
        artifacts: if artifacts.is_empty() {
            None
        } else {
            Some(artifacts.iter().map(|a| a.id.as_str()).collect())
        },
        external: if external.is_empty() {
            None
        } else {
            Some(external)
        },
    };
    write_atomic(&marker, &json_bytes(&manifest)?, 0o600)
        .map_err(|e| format!("finalize backup: {e}"))
}
/// Returns a report alongside an error whenever detection completed, just as Go Run does.
pub fn run(o: &Options<'_>) -> (Option<Report>, Option<String>) {
    let (source, destination) = match validate_roots(&o.source_root, &o.destination_root) {
        Ok(r) => r,
        Err(e) => return (None, Some(e)),
    };
    let (cs, cd) = match config_roots(o, &source, &destination) {
        Ok(r) => r,
        Err(e) => return (None, Some(e)),
    };
    let mut report = match dry_run(o) {
        Ok(r) => r,
        Err(e) => return (None, Some(e)),
    };
    if o.dry_run || !report.detection.detected {
        return (Some(report), None);
    }
    let error = mutate(o, &source, &destination, &cs, &cd, &mut report).err();
    (Some(report), error)
}
fn mutate(
    o: &Options<'_>,
    source: &str,
    destination: &str,
    cs: &str,
    cd: &str,
    report: &mut Report,
) -> Result<(), String> {
    if o.copy_secrets
        && report.detection.secret_store.detected
        && !report.detection.secret_store.migratable
    {
        return Err("secret store was detected but no migratable SecretStore was injected".into());
    }
    let state_path = join(destination, ".migration-state.json");
    let mut state = match fs::read(&state_path) {
        Ok(data) => {
            let st = json::state(&data).map_err(|e| format!("decode migration state: {e}"))?;
            if st.version != 1 || st.source_root != source || st.destination_root != destination {
                return Err(
                    "migration state does not match the requested source and destination".into(),
                );
            }
            if st.backup_dir.is_empty() {
                return Err("migration state has no backup directory".into());
            }
            report.resumed = true;
            st
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
        Err(e) => {
            return Err(format!(
                "read migration state: {}",
                io_error("open", &state_path, e)
            ));
        }
    };
    let backup = if !state.backup_dir.is_empty() {
        state.backup_dir.clone()
    } else {
        absolute_dir(&if o.backup_dir.is_empty() {
            format!("{destination}.migration-backup")
        } else {
            o.backup_dir.clone()
        })
        .map_err(|e| format!("resolve backup directory: {e}"))?
    };
    reject_symlink_components(&backup).map_err(|e| format!("backup directory is unsafe: {e}"))?;
    let native = native_root(&o.home_dir, platform(o)?);
    for root in [
        source,
        destination,
        cs,
        cd,
        &o.scheduler_source,
        &o.scheduler_dest,
        &native,
    ] {
        if !root.is_empty() && overlap(root, &backup) {
            return Err(
                "backup directory must be outside all migration source and destination directories"
                    .into(),
            );
        }
    }
    ensure_backup(
        &backup,
        source,
        cs,
        report.detection.artifacts.as_deref().unwrap_or_default(),
    )?;
    report.backup_dir = backup.clone();
    if state.version == 0 {
        state = State {
            version: 1,
            source_root: source.into(),
            destination_root: destination.into(),
            backup_dir: backup,
            items: None,
        }
    }
    state.items.get_or_insert_default();
    mkdir(destination).map_err(|e| format!("create destination: {e}"))?;
    write_state(&state_path, &state)?;
    let files = generated(o, destination)?;
    for item in report.items.iter_mut().flatten() {
        let a = &item.artifact;
        if state
            .items
            .as_ref()
            .and_then(|m| m.get(&a.id))
            .is_some_and(|s| s == "done")
        {
            item.status = "skipped".into();
            continue;
        }
        if let Some(before) = o.before_item
            && let Err(e) = before(a)
        {
            item.status = "failed".into();
            item.error = e.clone();
            return Err(format!("migrate {}: {e}", a.id));
        }
        if a.kind == "secret-store" && !o.copy_secrets {
            item.status = "manual".into();
            report.warnings.push(
                "secret values were not copied; configure the Go secret store manually".into(),
            );
            state
                .items
                .get_or_insert_default()
                .insert(a.id.clone(), "manual".into());
            write_state(&state_path, &state)?;
            continue;
        }
        let result = if a.kind == "secret-store" {
            o.secret_store
                .unwrap_or(&MetadataOnlySecretStore)
                .migrate(destination)
                .map_err(|e| (e.clone(), format!("migrate secret store: {e}")))
        } else {
            artifact_data(a, &files)
                .map_err(|e| (e.clone(), e))
                .and_then(|(mut data, mode)| {
                    if a.kind == "scheduler" {
                        let wrapper = if o.scheduler_config.output_dir.is_empty() {
                            join(destination, "schedules")
                        } else {
                            o.scheduler_config.output_dir.clone()
                        };
                        data = String::from_utf8_lossy(&data)
                            .replace("__WRAPPER_DIR__", &wrapper)
                            .into_bytes();
                    }
                    write_atomic(&a.destination, &data, mode)
                        .map_err(|e| (e.clone(), format!("write {}: {e}", a.destination)))
                })
        };
        if let Err((item_error, error)) = result {
            item.status = "failed".into();
            item.error = item_error;
            return Err(error);
        }
        state
            .items
            .get_or_insert_default()
            .insert(a.id.clone(), "done".into());
        item.status = "done".into();
        write_state(&state_path, &state)?;
    }
    report.complete = report.items.iter().flatten().all(|i| {
        state
            .items
            .as_ref()
            .and_then(|m| m.get(&i.artifact.id))
            .is_some_and(|s| s == "done")
    });
    Ok(())
}
fn artifact_data(
    a: &Artifact,
    generated: &BTreeMap<String, String>,
) -> Result<(Vec<u8>, u32), String> {
    if a.kind == "scheduler" {
        let name = Path::new(&a.destination)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let data = generated
            .get(name.as_ref())
            .ok_or_else(|| format!("no generated scheduler content for {name}"))?;
        return Ok((
            data.as_bytes().to_vec(),
            if name.ends_with(".sh") { 0o755 } else { 0o644 },
        ));
    }
    if a.source.is_empty() {
        return Err(format!("migration source missing for {}", a.id));
    }
    let info = fs::symlink_metadata(&a.source).map_err(|e| {
        format!(
            "read migration source {}: {}",
            a.source,
            io_error("lstat", &a.source, e)
        )
    })?;
    if !info.is_file() {
        return Err(format!(
            "migration source is not a regular file: {}",
            a.source
        ));
    }
    let data = fs::read(&a.source).map_err(|e| {
        format!(
            "read migration source {}: {}",
            a.source,
            io_error("open", &a.source, e)
        )
    })?;
    Ok((data, file_mode(&info)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_windows_fold_matches_every_go_unicode_scalar() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../rust-tests/parity/oracle/migration-safety/fold.json"
        )))
        .unwrap();
        assert_eq!(fixture["toolchain"], "go1.26.6");
        assert_eq!(fixture["unicode"], "15.0.0");
        let pairs = fixture["pairs"].as_array().unwrap();
        assert_eq!(pairs.len(), 1454);
        let mut remaining = pairs.iter().peekable();
        for value in 0..=0x10ffff {
            let Some(character) = char::from_u32(value) else {
                continue;
            };
            let expected = if remaining
                .peek()
                .is_some_and(|p| p[0].as_u64() == Some(u64::from(value)))
            {
                u32::try_from(remaining.next().unwrap()[1].as_u64().unwrap()).unwrap()
            } else {
                value
            };
            assert_eq!(
                windows_case_fold::fold(character),
                expected,
                "U+{value:04X}"
            );
        }
        assert!(remaining.next().is_none());
        assert!(path_prefix_casefold(
            Path::new("/Legacy/Σcope"),
            Path::new("/legacy/ςCOPE/child")
        ));
        assert!(!path_prefix_casefold(
            Path::new("/Legacy"),
            Path::new("/legacy-other")
        ));
        assert!(!path_prefix_casefold(
            Path::new("/Legacy/child"),
            Path::new("/legacy")
        ));
        assert!(!path_prefix_casefold(Path::new("/I"), Path::new("/ı")));
    }
    #[cfg(unix)]
    #[test]
    fn migration_path_cleaning_preserves_native_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let name = std::ffi::OsString::from_vec(b"raw-\xff".to_vec());
        let expected = Path::new("/base").join(&name);
        let input = expected.join("..").join(&name);
        assert_eq!(clean_absolute(&input).unwrap(), expected);
    }
    #[cfg(unix)]
    #[test]
    fn migration_atomic_write_preserves_zero_mode() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("zero").to_string_lossy().into_owned();
        write_atomic(&path, b"fixture", 0).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"fixture");
    }
    #[test]
    fn migration_backup_marker_must_be_regular() {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("backup");
        fs::create_dir_all(backup.join(".complete.json")).unwrap();
        assert_eq!(
            ensure_backup(&backup.to_string_lossy(), "source", "source", &[]).unwrap_err(),
            "backup completion marker is not a regular file"
        );
    }
}
