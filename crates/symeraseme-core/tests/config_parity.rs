use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
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

struct OracleCommandOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    _stderr: Vec<u8>,
}

const ORACLE_TIMEOUT: Duration = Duration::from_secs(30);
const ORACLE_MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

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

struct TempRootGuard(PathBuf);

impl Drop for TempRootGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_file_backed(
    command: &mut Command,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
) -> std::io::Result<OracleCommandOutput> {
    let stdout = fs::File::create(stdout_path)?;
    let stderr = fs::File::create(stderr_path)?;
    configure_process_group(command)?;
    command.stdout(Stdio::from(stdout.try_clone()?));
    command.stderr(Stdio::from(stderr.try_clone()?));
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            kill_process_tree(&mut child)?;
            let _ = child.wait()?;
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle subprocess exceeded its bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    drop(stdout);
    drop(stderr);
    let stdout = read_capped_file(stdout_path, ORACLE_MAX_OUTPUT_BYTES)?;
    let stderr = read_capped_file(stderr_path, ORACLE_MAX_OUTPUT_BYTES)?;
    Ok(OracleCommandOutput {
        status,
        stdout,
        _stderr: stderr,
    })
}

fn read_capped_file(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut output = Vec::new();
    file.by_ref().take(limit + 1).read_to_end(&mut output)?;
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

#[cfg(windows)]
fn configure_process_group(command: &mut Command) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn configure_process_group(_command: &mut Command) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "oracle process-tree cleanup is unsupported on this platform",
    ))
}

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
        let status = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
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
    let encoded = serde_json::to_string(&json!({ "config": config, "storage": storage }))
        .expect("serialize result")
        .replace(root.to_str().expect("UTF-8 test root"), "$ROOT");
    serde_json::from_str(&encoded).expect("normalized result JSON")
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
