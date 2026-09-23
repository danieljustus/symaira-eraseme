//! Install/status/uninstall for the generated scheduler, ported from Go's
//! `scheduler.Install`, `Status` and `Uninstall`.
//!
//! Go resolves every path and runs every platform command through an injectable
//! `Runner`, which is what makes this contract measurable. The same shape is
//! kept here: [`Runner`] is the seam, [`ExecRunner`] is production, and the
//! parity tests inject a recording fake so no `launchctl`, `systemctl` or
//! `crontab` has to exist.
//!
//! Two behaviours are reproduced deliberately, both tracked in #1000: Go's
//! `Status` resolves launchd files under the bare name (`symeraseme-tick.plist`)
//! while `Uninstall` and the legacy scan use the label
//! (`com.symeraseme.tick.plist`), and a second `install` refuses to run because
//! it treats its own output as legacy units. Fixing either is a contract change
//! on the oracle, so this port mirrors today's behaviour until that is decided.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::scan_legacy_python_units;
use super::{Config, LegacyUnit, Platform, SchedulerError, WRAPPER_DIR_PLACEHOLDER, write_files};

/// Executes platform commands. Tests inject a recording runner; production uses
/// [`ExecRunner`].
pub trait Runner {
    fn run(&self, name: &str, args: &[&str]) -> io::Result<Vec<u8>>;
}

/// The production runner, mirroring Go's `ExecRunner`.
pub struct ExecRunner;

impl Runner for ExecRunner {
    fn run(&self, name: &str, args: &[&str]) -> io::Result<Vec<u8>> {
        let program = look_path(name)?;
        let output = Command::new(program).args(args).output()?;
        // Go's CombinedOutput returns stdout+stderr together and reports a
        // non-zero exit as an error carrying that combined text.
        let mut combined = output.stdout;
        combined.extend_from_slice(&output.stderr);
        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(io::Error::other(format!(
                "exit status {code}: {}",
                String::from_utf8_lossy(&combined).trim_end()
            )));
        }
        Ok(combined)
    }
}

/// Resolves an executable the way Go's `exec.LookPath` does: only the `PATH`
/// environment variable is searched, and an unset or empty `PATH` is an error
/// rather than a fallback to a compiled-in default. `std::process::Command`
/// alone would fall back to a system search path, so a cleared `PATH` would
/// still find `/bin/launchctl` — a silent behavioural difference from Go, and
/// one that would run a system binary where Go refuses.
fn look_path(name: &str) -> io::Result<PathBuf> {
    if name.contains('/') {
        return Ok(PathBuf::from(name));
    }
    let not_found = || {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("exec: {name:?}: executable file not found in $PATH"),
        )
    };
    let path = std::env::var_os("PATH").ok_or_else(not_found)?;
    if path.is_empty() {
        return Err(not_found());
    }
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Ok(candidate);
        }
    }
    Err(not_found())
}

fn is_executable(path: &Path) -> bool {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return false,
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The options Go's `InstallOptions` carries.
pub struct InstallOptions<'a> {
    pub config: Config,
    /// The raw platform string a caller supplied, mirroring Go's
    /// `Config.Platform`: empty auto-detects, unknown is rejected. `None` means
    /// the typed [`Config::platform`] already decided.
    pub platform_name: Option<String>,
    /// Private HOME for this run; `None` resolves the process HOME like Go.
    pub home_dir: Option<PathBuf>,
    /// XDG_CONFIG_HOME override for the systemd unit directory.
    pub xdg_config_home: Option<PathBuf>,
    /// Whether existing units may be replaced.
    pub replace_legacy: bool,
    pub runner: Option<&'a dyn Runner>,
}

impl<'a> InstallOptions<'a> {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            platform_name: None,
            home_dir: None,
            xdg_config_home: None,
            replace_legacy: false,
            runner: None,
        }
    }
}

/// Go's `InstallResult`.
#[derive(Debug, Clone)]
pub struct InstallResult {
    pub platform: Platform,
    pub output_dir: String,
    pub files: Vec<String>,
    pub legacy: Vec<LegacyUnit>,
    pub replacement_required: bool,
}

/// One entry of Go's `StatusResult`.
///
/// Go declares these fields without JSON tags, so `encoding/json` emits the Go
/// field names verbatim (`Label`, `Installed`, …). The renames keep that shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusEntry {
    #[serde(rename = "Label")]
    pub label: String,
    #[serde(rename = "Installed")]
    pub installed: bool,
    #[serde(rename = "Active")]
    pub active: bool,
    #[serde(rename = "Path")]
    pub path: String,
    #[serde(rename = "Legacy")]
    pub legacy: bool,
    #[serde(rename = "Error")]
    pub error: String,
}

/// Go's `StatusResult`.
#[derive(Debug, Clone, Serialize)]
pub struct StatusResult {
    #[serde(rename = "Platform")]
    pub platform: Platform,
    #[serde(rename = "Entries")]
    pub entries: Vec<StatusEntry>,
}

/// The launchd label Go derives with `nameToLaunchdLabel`.
fn launchd_label(name: &str) -> String {
    format!("com.symeraseme.{}", name.trim_start_matches("symeraseme-"))
}

/// The HOME string Go's `InstallOptions.HomeDir` carries into the legacy scan.
fn home_str<'a>(options: &'a InstallOptions<'a>) -> Option<&'a str> {
    options
        .home_dir
        .as_ref()
        .map(|path| path.to_str().unwrap_or(""))
}

fn resolve_home(options: &InstallOptions<'_>) -> Result<PathBuf, SchedulerError> {
    if let Some(home) = &options.home_dir {
        return Ok(home.clone());
    }
    dirs_home().ok_or_else(|| SchedulerError::ResolveHomeDirectory(io::Error::other("no HOME")))
}

/// Resolves the user's home directory the way Go's `os.UserHomeDir` does.
fn dirs_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return Some(PathBuf::from(profile));
        }
        let drive = std::env::var_os("HOMEDRIVE")?;
        let path = std::env::var_os("HOMEPATH")?;
        let mut combined = PathBuf::from(drive);
        combined.push(path);
        Some(combined)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// Go's `installRoot`: the per-user unit directory for the platform.
fn install_root(
    options: &InstallOptions<'_>,
    platform: Platform,
) -> Result<PathBuf, SchedulerError> {
    let home = resolve_home(options)?;
    if platform == Platform::Launchd {
        return Ok(home.join("Library").join("LaunchAgents"));
    }
    let mut config_home = options.xdg_config_home.clone();
    if config_home.is_none() && options.home_dir.is_none() {
        // Go only consults the environment when no explicit HOME was given.
        config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    }
    let config_home = config_home.unwrap_or_else(|| home.join(".config"));
    Ok(config_home.join("systemd").join("user"))
}

/// The token Go's `contentWithWrapperDir` expands for the crontab render.
const CRON_WRAPPER_TOKEN: &str = "{wrapper_dir}";

/// Mirrors Go's `removeCronBlock`: drops the marked Symaira block, inclusive.
pub fn remove_cron_block(content: &str) -> String {
    const START: &str = "# Symaira EraseMe scheduled tasks";
    const END: &str = "# End Symaira EraseMe scheduled tasks";
    let mut skip = false;
    let mut kept: Vec<&str> = Vec::new();
    for line in content.split('\n') {
        match line {
            START => skip = true,
            END => skip = false,
            other => {
                if !skip {
                    kept.push(other);
                }
            }
        }
    }
    kept.join("\n")
}

/// Mirrors Go's `isPythonSchedulerContent`.
pub fn is_python_scheduler_content(content: &str) -> bool {
    const LEGACY_MARKER: &str = "Generated by symeraseme generate-scheduler";
    let lower = content.to_lowercase();
    for marker in [
        "python",
        "site-packages",
        "symeraseme.core.scheduler",
        "uv run",
        "venv/bin/activate",
    ] {
        if lower.contains(marker) {
            return true;
        }
    }
    content.contains(LEGACY_MARKER) && !content.contains(&format!("{LEGACY_MARKER} (Go)"))
}

/// Unit file names `status` inspects. Launchd units live under their label
/// (`com.symeraseme.tick.plist`), the same name `write_files`/`uninstall` and
/// the legacy scan use; systemd units use the logical name.
fn status_unit_names(platform: Platform) -> [&'static str; 3] {
    match platform {
        Platform::Launchd => [
            "com.symeraseme.tick.plist",
            "com.symeraseme.poll.plist",
            "com.symeraseme.rescan.plist",
        ],
        _ => [
            "symeraseme-tick.timer",
            "symeraseme-poll.timer",
            "symeraseme-rescan.timer",
        ],
    }
}

fn unit_short_names() -> [&'static str; 3] {
    ["symeraseme-tick", "symeraseme-poll", "symeraseme-rescan"]
}

/// Go's `ErrLegacyUnits` text.
pub const ERR_LEGACY_UNITS: &str = "legacy scheduler units detected; replacement was not requested";

/// Go's `ErrLegacyUnits` text is also what the handler surfaces, so the port
/// needs the same message rather than a new error variant.
impl SchedulerError {
    pub fn legacy_units() -> Self {
        SchedulerError::LegacyUnitsDetected
    }
}

/// Resolves the platform the way Go does for Install/Status/Uninstall: the raw
/// name a caller supplied wins (empty auto-detects, unknown is rejected),
/// otherwise the typed config value, otherwise auto-detection.
fn resolve_platform(options: &InstallOptions<'_>) -> Result<Platform, SchedulerError> {
    if let Some(name) = &options.platform_name {
        return Platform::from_name(name);
    }
    match &options.config.platform {
        Some(platform) => Ok(*platform),
        None => Ok(super::detect_platform()),
    }
}

fn run(options: &InstallOptions<'_>, name: &str, args: &[&str]) -> io::Result<Vec<u8>> {
    match options.runner {
        Some(runner) => runner.run(name, args),
        None => ExecRunner.run(name, args),
    }
}

/// Mirrors Go's `Install`: scan for legacy units, refuse unless replacement was
/// requested, generate and write the wrappers, then install the native units.
pub fn install(options: &InstallOptions<'_>) -> Result<InstallResult, SchedulerError> {
    let mut config = options.config.clone();
    let platform = resolve_platform(options)?;
    config.platform = Some(platform);

    if config.output_dir.is_empty() {
        config.output_dir = Config::default().output_dir;
    }
    let legacy = scan_legacy_python_units(home_str(options), Some(platform))?;
    let mut result = InstallResult {
        platform,
        output_dir: config.output_dir.clone(),
        files: Vec::new(),
        legacy: legacy.clone(),
        replacement_required: !legacy.is_empty(),
    };
    if !legacy.is_empty() && !options.replace_legacy {
        return Err(SchedulerError::LegacyUnitsDetected);
    }

    let generated = super::generate(&config)?;
    let written = write_files(&config.output_dir, &generated)?;
    result.files = written
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();

    for unit in &legacy {
        match fs::remove_file(&unit.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(SchedulerError::RemoveLegacyUnit {
                    path: unit.path.clone(),
                    source,
                });
            }
        }
    }

    if platform == Platform::Cron {
        install_cron(options, &generated, &config.output_dir)?;
        return Ok(result);
    }

    let root = install_root(options, platform)?;
    fs::create_dir_all(&root).map_err(SchedulerError::CreateNativeUnitDirectory)?;
    install_native_units(&generated, &root, &config.output_dir, platform)?;

    if platform == Platform::Launchd {
        for short in unit_short_names() {
            // Go reports the bare unit name in the failure, not the full path,
            // so the message stays comparable across install roots.
            let name = launchd_label(short) + ".plist";
            let path = root.join(&name);
            let path_arg = path.to_string_lossy().to_string();
            let _ = run(options, "launchctl", &["unload", &path_arg]);
            if let Err(source) = run(options, "launchctl", &["load", &path_arg]) {
                return Err(SchedulerError::LoadLaunchdUnit { unit: name, source });
            }
        }
        return Ok(result);
    }

    if let Err(source) = run(options, "systemctl", &["--user", "daemon-reload"]) {
        return Err(SchedulerError::SystemctlDaemonReload(source));
    }
    for short in unit_short_names() {
        let timer = format!("{short}.timer");
        if let Err(source) = run(options, "systemctl", &["--user", "enable", "--now", &timer]) {
            return Err(SchedulerError::EnableSystemdTimer {
                unit: timer,
                source,
            });
        }
    }
    Ok(result)
}

fn install_cron(
    options: &InstallOptions<'_>,
    generated: &BTreeMap<String, String>,
    output_dir: &str,
) -> Result<(), SchedulerError> {
    let existing = run(options, "crontab", &["-l"]).unwrap_or_default();
    let existing = String::from_utf8_lossy(&existing).to_string();
    let mut updated = remove_cron_block(&existing);
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    let content = generated.get("crontab.txt").cloned().unwrap_or_default();
    // Go's `contentWithWrapperDir` replaces the literal "{wrapper_dir}" for the
    // crontab render, which is a different token from the unit placeholder.
    updated.push_str(&content.replace(CRON_WRAPPER_TOKEN, output_dir));

    let staging = staging_path(output_dir, ".crontab-")?;
    fs::write(&staging, updated).map_err(|source| SchedulerError::WriteCronStaging {
        path: staging.clone(),
        source,
    })?;
    let staging_arg = staging.to_string_lossy().to_string();
    let result = run(options, "crontab", &[&staging_arg]);
    let _ = fs::remove_file(&staging);
    result.map_err(SchedulerError::InstallCrontab)?;
    Ok(())
}

/// Creates a staging file with Go's `os.CreateTemp` naming, so the recorded
/// command carries the same shape.
fn staging_path(dir: &str, prefix: &str) -> Result<PathBuf, SchedulerError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos() as u64)
        .unwrap_or_default();
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("{prefix}{nanos}{sequence}");
    let path = Path::new(dir).join(name);
    fs::create_dir_all(dir).map_err(SchedulerError::CreateOutputDirectory)?;
    Ok(path)
}

fn install_native_units(
    generated: &BTreeMap<String, String>,
    root: &Path,
    output_dir: &str,
    platform: Platform,
) -> Result<(), SchedulerError> {
    let extension = if platform == Platform::Systemd {
        ".service"
    } else {
        ".plist"
    };
    let mut names: Vec<&String> = generated
        .keys()
        .filter(|name| {
            name.ends_with(extension) || (platform == Platform::Systemd && name.ends_with(".timer"))
        })
        .collect();
    names.sort();
    for name in names {
        let path = root.join(name);
        let content = generated[name].replace(WRAPPER_DIR_PLACEHOLDER, output_dir);
        fs::write(&path, content)
            .map_err(|source| SchedulerError::WriteNativeUnit { path, source })?;
    }
    Ok(())
}

/// Mirrors Go's `Status`, including the launchd name mismatch of #1000.
pub fn status(options: &InstallOptions<'_>) -> Result<StatusResult, SchedulerError> {
    let platform = resolve_platform(options)?;
    if platform == Platform::Cron {
        let entry = match run(options, "crontab", &["-l"]) {
            Ok(output) => {
                let text = String::from_utf8_lossy(&output).to_string();
                StatusEntry {
                    label: "cron".to_string(),
                    installed: text.contains("# Symaira EraseMe scheduled tasks"),
                    active: false,
                    path: "crontab".to_string(),
                    legacy: is_python_scheduler_content(&text),
                    error: String::new(),
                }
            }
            Err(_) => StatusEntry {
                label: "cron".to_string(),
                installed: false,
                active: false,
                path: "crontab".to_string(),
                legacy: false,
                error: "crontab not available or no crontab installed".to_string(),
            },
        };
        return Ok(StatusResult {
            platform,
            entries: vec![entry],
        });
    }

    let root = install_root(options, platform)?;
    let units = scan_legacy_python_units(home_str(options), Some(platform))?;
    let legacy_by_path: BTreeMap<String, bool> = units
        .iter()
        .map(|unit| (unit.path.to_string_lossy().to_string(), unit.is_python))
        .collect();

    let names = status_unit_names(platform);
    let mut entries = Vec::with_capacity(names.len());
    for (index, name) in names.iter().enumerate() {
        let path = root.join(name);
        let mut entry = StatusEntry {
            label: unit_short_names()[index].to_string(),
            installed: path.is_file(),
            active: false,
            path: path.to_string_lossy().to_string(),
            legacy: legacy_by_path
                .get(&path.to_string_lossy().to_string())
                .copied()
                .unwrap_or(false),
            error: String::new(),
        };
        if platform == Platform::Launchd {
            let label = launchd_label(unit_short_names()[index]);
            match run(options, "launchctl", &["list", &label]) {
                Ok(output) => {
                    entry.active = String::from_utf8_lossy(&output).contains(&label);
                }
                Err(_) => {
                    if entry.installed {
                        entry.error = "launchctl could not confirm active state".to_string();
                    }
                }
            }
        } else {
            let timer = format!("{}.timer", unit_short_names()[index]);
            match run(options, "systemctl", &["--user", "is-active", &timer]) {
                Ok(output) => {
                    entry.active = String::from_utf8_lossy(&output).trim() == "active";
                }
                Err(_) => {
                    if entry.installed {
                        entry.error = "systemctl could not confirm active state".to_string();
                    }
                }
            }
        }
        entries.push(entry);
    }
    Ok(StatusResult { platform, entries })
}

/// Mirrors Go's `Uninstall`. Generated wrappers stay on disk, so the removal is
/// reversible.
pub fn uninstall(options: &InstallOptions<'_>) -> Result<(), SchedulerError> {
    let platform = resolve_platform(options)?;
    if platform == Platform::Cron {
        let existing = match run(options, "crontab", &["-l"]) {
            Ok(output) => String::from_utf8_lossy(&output).to_string(),
            Err(_) => return Ok(()),
        };
        let cleaned = remove_cron_block(&existing);
        let staging = staging_path(
            &std::env::temp_dir().to_string_lossy(),
            ".symeraseme-crontab-",
        )?;
        fs::write(&staging, cleaned).map_err(|source| SchedulerError::WriteCronStaging {
            path: staging.clone(),
            source,
        })?;
        let staging_arg = staging.to_string_lossy().to_string();
        let result = run(options, "crontab", &[&staging_arg]);
        let _ = fs::remove_file(&staging);
        return result.map_err(SchedulerError::InstallCrontab).map(|_| ());
    }

    let root = install_root(options, platform)?;
    if platform == Platform::Launchd {
        for short in unit_short_names() {
            let path = root.join(launchd_label(short) + ".plist");
            let path_arg = path.to_string_lossy().to_string();
            let _ = run(options, "launchctl", &["unload", &path_arg]);
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(SchedulerError::RemoveNativeUnit { path, source }),
            }
        }
        return Ok(());
    }

    for short in unit_short_names() {
        let timer = format!("{short}.timer");
        let _ = run(
            options,
            "systemctl",
            &["--user", "disable", "--now", &timer],
        );
        for extension in [".service", ".timer"] {
            let path = root.join(format!("{short}{extension}"));
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(SchedulerError::RemoveNativeUnit { path, source }),
            }
        }
    }
    let _ = run(options, "systemctl", &["--user", "daemon-reload"]);
    Ok(())
}
