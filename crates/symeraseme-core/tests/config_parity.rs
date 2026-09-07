use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Child, Command, Stdio};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use symeraseme_core::config::{
    Config, ConfigContext, ConfigError, Storage, default_encrypted_temp_dir, defaults, load,
    resolve_storage,
};

const GO_FIXTURE: &str = include_str!("../../../rust-tests/parity/oracle/config/config_cases.json");

struct TestTree {
    root: PathBuf,
}

impl TestTree {
    fn new(case: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("symeraseme-config-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test tree");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn context(&self, environment: BTreeMap<String, String>) -> ConfigContext {
        fs::create_dir_all(self.home()).expect("create home");
        fs::create_dir_all(self.project()).expect("create project");
        ConfigContext::new(self.home(), self.project(), environment)
    }

    fn write(&self, path: impl AsRef<Path>, contents: &str) {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, contents).expect("write fixture");
    }
}

impl Drop for TestTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture(case: &str) -> Value {
    let document: Value = serde_json::from_str(GO_FIXTURE).expect("valid Go config fixture");
    document.get(case).cloned().expect("fixture case")
}

#[cfg(unix)]
#[derive(Debug)]
struct OracleCommandOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    _stderr: Vec<u8>,
}

#[cfg(unix)]
const ORACLE_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(unix)]
const ORACLE_MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
#[cfg(unix)]
const ORACLE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(unix)]
fn run_go_config_oracle() -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let temp_root = std::env::temp_dir().join(format!(
        "symeraseme-config-oracle-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));
    fs::create_dir(&temp_root).expect("create isolated oracle build directory");
    let _cleanup = TempRootGuard(temp_root.clone());
    let executable = temp_root.join(if cfg!(windows) {
        "config-oracle.exe"
    } else {
        "config-oracle"
    });

    let mut build = Command::new("go");
    build
        .args(["build", "-o"])
        .arg(&executable)
        .arg("./rust-tests/parity/oracle/config")
        .current_dir(&root)
        .env("GOWORK", "off");
    let build_output = run_file_backed(
        &mut build,
        &temp_root.join("build.stdout"),
        &temp_root.join("build.stderr"),
        ORACLE_TIMEOUT,
    )
    .expect("Go must be available for the committed config oracle");
    assert!(
        build_output.status.success(),
        "Go config oracle build failed"
    );

    let mut oracle = Command::new(&executable);
    oracle.current_dir(&root).env_clear();
    let output = run_file_backed(
        &mut oracle,
        &temp_root.join("oracle.stdout"),
        &temp_root.join("oracle.stderr"),
        ORACLE_TIMEOUT,
    )
    .expect("Go config oracle execution must complete within its bounded timeout");
    assert!(
        output.status.success(),
        "Go config oracle exited unsuccessfully"
    );
    serde_json::from_slice(&output.stdout).expect("Go config oracle must emit valid JSON")
}

#[cfg(unix)]
struct TempRootGuard(PathBuf);

#[cfg(unix)]
impl Drop for TempRootGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn run_file_backed(
    command: &mut Command,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
) -> std::io::Result<OracleCommandOutput> {
    run_file_backed_with_limit(
        command,
        stdout_path,
        stderr_path,
        timeout,
        ORACLE_MAX_OUTPUT_BYTES,
    )
}

#[cfg(unix)]
fn run_file_backed_with_limit(
    command: &mut Command,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
    output_limit: u64,
) -> std::io::Result<OracleCommandOutput> {
    let stdout = fs::File::create(stdout_path)?;
    let stderr = fs::File::create(stderr_path)?;
    configure_process_group(command)?;
    command.stdout(Stdio::from(stdout.try_clone()?));
    command.stderr(Stdio::from(stderr.try_clone()?));
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if output_exceeded(stdout_path, stderr_path, output_limit)? {
            if child.try_wait()?.is_none() {
                terminate_child_bounded(&mut child, ORACLE_CLEANUP_TIMEOUT)?;
            }
            return Err(std::io::Error::other(
                "oracle subprocess output exceeded its bounded limit",
            ));
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            terminate_child_bounded(&mut child, ORACLE_CLEANUP_TIMEOUT)?;
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle subprocess exceeded its bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    drop(stdout);
    drop(stderr);
    let stdout = read_capped_file(stdout_path, output_limit)?;
    let stderr = read_capped_file(stderr_path, output_limit)?;
    Ok(OracleCommandOutput {
        status,
        stdout,
        _stderr: stderr,
    })
}

#[cfg(unix)]
fn output_exceeded(stdout_path: &Path, stderr_path: &Path, limit: u64) -> std::io::Result<bool> {
    for path in [stdout_path, stderr_path] {
        match fs::metadata(path) {
            Ok(metadata) if metadata.len() > limit => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

#[cfg(unix)]
fn terminate_child_bounded(child: &mut Child, timeout: Duration) -> std::io::Result<()> {
    let tree_cleanup = kill_process_tree(child);
    let direct_cleanup = child.kill();
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle subprocess cleanup exceeded its bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    if let Err(error) = tree_cleanup {
        return Err(std::io::Error::other(format!(
            "oracle process-tree cleanup failed: {error}"
        )));
    }
    if let Err(error) = direct_cleanup {
        let already_exited = matches!(
            error.kind(),
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
        ) || error.raw_os_error() == Some(3);
        if !already_exited {
            return Err(std::io::Error::other(format!(
                "oracle direct-child cleanup failed: {error}"
            )));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn read_capped_file(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut output = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut output)?;
    if output.len() as u64 > limit {
        return Err(std::io::Error::other(
            "oracle subprocess output exceeded its bounded limit",
        ));
    }
    Ok(output)
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    unsafe extern "C" {
        fn setpgid(pid: i32, pgid: i32) -> i32;
    }
    unsafe {
        command.pre_exec(|| {
            if setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    Ok(())
}

#[cfg(unix)]
fn kill_process_tree(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        let pid =
            i32::try_from(child.id()).map_err(|_| std::io::Error::other("invalid child pid"))?;
        const SIGKILL: i32 = 9;
        let result = unsafe { kill(-pid, SIGKILL) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(3) {
                return Err(error);
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        let status = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            // taskkill can lose a race with a process that exits after the
            // preflight check. Treat that already-completed cleanup as
            // idempotent; a still-running process remains a hard failure.
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            return Err(std::io::Error::other(
                "taskkill failed to clean oracle tree",
            ));
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "oracle process-tree cleanup is unsupported on this platform",
        ))
    }
}

#[cfg(unix)]
fn helper_command(test_name: &str, marker: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("current test executable"));
    command
        .args(["--exact", test_name, "--nocapture"])
        .env(marker, "1");
    command
}

#[cfg(unix)]
#[test]
fn oracle_runner_output_helper() {
    if std::env::var_os("SYMERASEME_CONFIG_ORACLE_OUTPUT_HELPER").is_none() {
        return;
    }
    let chunk = [b'x'; 4096];
    let mut stdout = std::io::stdout().lock();
    for _ in 0..1024 {
        stdout.write_all(&chunk).expect("write helper output");
    }
    stdout.flush().expect("flush helper output");
}

#[cfg(unix)]
#[test]
fn oracle_runner_timeout_helper() {
    if std::env::var_os("SYMERASEME_CONFIG_ORACLE_TIMEOUT_HELPER").is_some() {
        thread::sleep(Duration::from_secs(30));
    }
}

// These probes require tree-safe termination and therefore remain Unix-only
// until the documented Windows Job Object capability is implemented.
#[cfg(unix)]
#[test]
fn run_file_backed_enforces_live_output_limit() {
    let tree = TestTree::new("runner-output-limit");
    let mut command = helper_command(
        "oracle_runner_output_helper",
        "SYMERASEME_CONFIG_ORACLE_OUTPUT_HELPER",
    );
    let error = run_file_backed_with_limit(
        &mut command,
        &tree.root.join("stdout"),
        &tree.root.join("stderr"),
        Duration::from_secs(5),
        1024,
    )
    .expect_err("oversized output must fail closed");
    assert!(
        error.to_string().contains("bounded limit"),
        "unexpected runner error: {error}"
    );
}

#[cfg(unix)]
#[test]
fn run_file_backed_timeout_cleanup_is_bounded() {
    let tree = TestTree::new("runner-timeout");
    let mut command = helper_command(
        "oracle_runner_timeout_helper",
        "SYMERASEME_CONFIG_ORACLE_TIMEOUT_HELPER",
    );
    let started = Instant::now();
    let error = run_file_backed_with_limit(
        &mut command,
        &tree.root.join("stdout"),
        &tree.root.join("stderr"),
        Duration::from_millis(50),
        1024 * 1024,
    )
    .expect_err("timed-out subprocess must fail closed");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3));
}

// This oracle and the process-cleanup probes require Unix process groups.
// Windows parity remains explicitly capability-gated until Job Object support
// exists; the native config cases still run with semantic path normalization.
#[cfg(unix)]
#[test]
fn go_config_oracle_provenance_fixture_and_rust_results_match() {
    let fixture: Value = serde_json::from_str(GO_FIXTURE).expect("valid Go config fixture");
    let oracle = run_go_config_oracle();
    assert_eq!(
        oracle["provenance"],
        json!({
            "source_revision": "119ee9f84fe7c9e1485d25ab10aac8582e98395c",
            "source_path": "internal/config/config.go",
            "source_sha256": "d197afc83776a85880428994e32b0c1585c245ed52d86f9a31fe51889cbce32c",
            "schema": "symaira-eraseme.config-parity.v1"
        })
    );
    assert_eq!(
        oracle["cases"], fixture,
        "normalized Go config oracle output differs from the committed fixture"
    );
}

fn normalized_result(root: &Path, config: &Config, storage: &Storage) -> Value {
    let cache_root = storage
        .temp_dir
        .parent()
        .and_then(Path::parent)
        .expect("encrypted temp dir has cache/tool/database shape");
    json!({
        "config": config,
        "storage": {
            "data_dir": normalize_path(root, "$ROOT", &storage.data_dir),
            "db_dir": normalize_path(root, "$ROOT", &storage.db_dir),
            "db_path": normalize_path(root, "$ROOT", &storage.db_path),
            "temp_dir": normalize_path(cache_root, "$CACHE", &storage.temp_dir),
            "encrypt_db": storage.encrypt_db,
        },
    })
}

fn normalize_path(root: &Path, placeholder: &str, value: &Path) -> String {
    let root = root.to_string_lossy().replace('\\', "/");
    let value = value.to_string_lossy().replace('\\', "/");
    let root = root.trim_end_matches('/');
    if value == root {
        return placeholder.to_owned();
    }
    if let Some(suffix) = value
        .strip_prefix(root)
        .filter(|suffix| suffix.starts_with('/'))
    {
        return format!("{placeholder}{suffix}");
    }
    value
}

fn assert_field(error: ConfigError, field: &str) {
    assert_eq!(error.field(), Some(field), "error = {error}");
    assert!(error.to_string().contains(field), "error = {error}");
}

#[test]
fn cfg_001_defaults_and_persistent_storage_match_go_fixture() {
    let tree = TestTree::new("cfg-001");
    let context = tree.context(BTreeMap::new());

    let config = defaults();
    assert_eq!(config.port, 8000);
    assert!(!config.encrypt_db);
    assert!(!config.allow_remote);

    let storage = resolve_storage(&context).expect("default storage");
    assert_eq!(
        normalized_result(&tree.root, &config, &storage),
        fixture("CFG-001")
    );
    assert!(
        !storage
            .db_path
            .starts_with(std::env::temp_dir().join("symeraseme"))
    );
}

#[test]
fn cfg_002_precedence_is_defaults_global_project_then_environment() {
    let tree = TestTree::new("cfg-002");
    let xdg = tree.root.join("xdg");
    let global = xdg.join("symeraseme/config.toml");
    tree.write(
        &global,
        "data_dir = \"global-data\"\ndb_dir = \"global-db\"\nencrypt_db = true\nport = 8100\nallow_remote = false\n",
    );
    tree.write(
        tree.project().join(".symeraseme.toml"),
        "data_dir = \"project-data\"\nencrypt_db = false\n",
    );
    let environment = BTreeMap::from([
        ("XDG_CONFIG_HOME".into(), xdg.to_string_lossy().into_owned()),
        ("SYMERASEME_DATA_DIR".into(), "~/env-data".into()),
        ("SYMERASEME_DB_DIR".into(), "env-db".into()),
        ("SYMERASEME_ENCRYPT_DB".into(), "true".into()),
        ("SYMERASEME_PORT".into(), "8123".into()),
        ("SYMERASEME_ALLOW_REMOTE".into(), "yes".into()),
    ]);
    let context = tree.context(environment);

    let config = load(&context).expect("precedence config");
    let storage = resolve_storage(&context).expect("precedence storage");
    assert_eq!(
        normalized_result(&tree.root, &config, &storage),
        fixture("CFG-002")
    );
}

#[test]
fn cfg_003_relative_and_tilde_paths_are_absolute_from_context() {
    let tree = TestTree::new("cfg-003");
    tree.write(
        tree.project().join(".symeraseme.toml"),
        "data_dir = \"~/custom/data\"\ndb_dir = \"relative-db\"\n",
    );
    let context = tree.context(BTreeMap::new());

    let storage = resolve_storage(&context).expect("path storage");
    assert_eq!(storage.data_dir, tree.home().join("custom/data"));
    assert_eq!(storage.db_dir, tree.project().join("relative-db"));
    assert!(storage.data_dir.is_absolute());
    assert!(storage.db_dir.is_absolute());
    assert!(storage.db_path.is_absolute());
    assert_eq!(
        normalized_result(&tree.root, &load(&context).unwrap(), &storage),
        fixture("CFG-003")
    );
}

#[test]
fn cfg_004_bool_and_integer_coercion_ignore_unknown_nested_fields() {
    let tree = TestTree::new("cfg-004");
    let xdg = tree.root.join("xdg");
    tree.write(
        xdg.join("symeraseme/config.toml"),
        "encrypt_db = 1\nallow_remote = \" oN \"\nport = \"8124\"\nunknown = true\n[legacy.server]\nport = 1\n",
    );
    let environment =
        BTreeMap::from([("XDG_CONFIG_HOME".into(), xdg.to_string_lossy().into_owned())]);
    let context = tree.context(environment);

    let config = load(&context).expect("coercion config");
    assert!(config.encrypt_db);
    assert!(config.allow_remote);
    assert_eq!(config.port, 8124);
    let storage = resolve_storage(&context).expect("coercion storage");
    assert_eq!(
        normalized_result(&tree.root, &config, &storage),
        fixture("CFG-004")
    );
}

#[test]
fn cfg_005_malformed_supported_values_are_classified_by_field() {
    let tree = TestTree::new("cfg-005");
    let mut environment = BTreeMap::from([("SYMERASEME_ENCRYPT_DB".into(), "maybe".into())]);
    let context = tree.context(environment.clone());
    assert_field(load(&context).expect_err("invalid bool"), "encrypt_db");

    environment.insert("SYMERASEME_ENCRYPT_DB".into(), "false".into());
    environment.insert("SYMERASEME_PORT".into(), "65536".into());
    assert_field(
        load(&tree.context(environment.clone())).expect_err("invalid port"),
        "port",
    );

    environment.remove("SYMERASEME_PORT");
    environment.insert("SYMERASEME_DATA_DIR".into(), "".into());
    assert!(
        load(&tree.context(environment)).is_ok(),
        "empty env is ignored"
    );

    tree.write(
        tree.project().join(".symeraseme.toml"),
        "db_dir = \"   \"\n",
    );
    assert_field(
        load(&tree.context(BTreeMap::new())).expect_err("empty path"),
        "db_dir",
    );
    tree.write(
        tree.project().join(".symeraseme.toml"),
        "db_dir = \"bad\\u0000path\"\n",
    );
    assert_field(
        load(&tree.context(BTreeMap::new())).expect_err("NUL path"),
        "db_dir",
    );

    let fixture = fixture("CFG-005");
    assert_eq!(
        fixture["errors"],
        json!(["encrypt_db", "port", "db_dir", "db_dir"])
    );
}

#[test]
fn cfg_006_relative_xdg_falls_back_and_temp_dir_is_user_scoped() {
    let tree = TestTree::new("cfg-006");
    tree.write(
        tree.home().join(".config/symeraseme/config.toml"),
        "data_dir = \"default-global\"\nencrypt_db = 1\nport = 9000\n",
    );
    let environment = BTreeMap::from([("XDG_CONFIG_HOME".into(), "relative-xdg".into())]);
    let context = tree.context(environment);

    let config = load(&context).expect("fallback config");
    let storage = resolve_storage(&context).expect("fallback storage");
    assert!(storage.encrypt_db);
    assert_eq!(config.port, 9000);
    assert_eq!(storage.data_dir, tree.project().join("default-global"));
    assert_eq!(
        storage.temp_dir,
        default_encrypted_temp_dir(&context).expect("temp dir")
    );
    assert!(storage.temp_dir.starts_with(tree.home()));
    assert!(
        storage.temp_dir != std::env::temp_dir().join("symeraseme/database"),
        "encrypted temp dir must not be the shared temp location"
    );
    assert_eq!(
        normalized_result(&tree.root, &config, &storage),
        fixture("CFG-006")
    );
}
