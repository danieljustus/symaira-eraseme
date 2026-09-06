use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
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
