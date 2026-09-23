use std::fs;
use symeraseme_engine::migration::{self, Options};
fn fixture() -> (tempfile::TempDir, Options<'static>) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let o = Options {
        source_root: source.to_string_lossy().into(),
        destination_root: root.path().join("destination").to_string_lossy().into(),
        platform: "cron".into(),
        ..Default::default()
    };
    (root, o)
}
#[test]
fn migration_empty_nulls() {
    let (_root, o) = fixture();
    let r = migration::dry_run(&o).unwrap();
    let v = serde_json::to_value(r).unwrap();
    assert!(v["items"].is_null());
    assert!(v["detection"]["reasons"].is_null());
    assert!(v["detection"]["artifacts"].is_null());
    assert_eq!(v["complete"], true);
}
#[test]
fn migration_detection_dry_run() {
    let (_root, mut o) = fixture();
    o.dry_run = true;
    for name in ["config.toml", "symeraseme.db", "identity.enc"] {
        fs::write(format!("{}/{name}", o.source_root), "fixture").unwrap();
    }
    let r = migration::dry_run(&o).unwrap();
    assert_eq!(r.items.unwrap().len(), 4);
    assert!(!std::path::Path::new(&o.destination_root).exists());
}
#[test]
fn migration_cleans_parent_paths() {
    let (_root, mut o) = fixture();
    o.destination_root = format!("{}/missing/..", o.source_root);
    assert!(migration::detect(&o).unwrap_err().contains("non-nested"));
}
fn write(o: &Options<'_>, name: &str) {
    let p = std::path::Path::new(&o.source_root).join(name);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, "fixture").unwrap();
}
#[test]
fn migration_backup_before_item_and_resume() {
    let (_root, mut o) = fixture();
    write(&o, "config.toml");
    write(&o, "symeraseme.db");
    write(&o, "nested/unrecognized");
    let backup = format!("{}.migration-backup", o.destination_root);
    let before = |a: &migration::Artifact| {
        assert!(std::path::Path::new(&format!("{backup}/.complete.json")).exists());
        assert_eq!(
            fs::read(format!("{backup}/source/nested/unrecognized")).unwrap(),
            b"fixture"
        );
        if a.id == "event-db" {
            Err("interrupted".into())
        } else {
            Ok(())
        }
    };
    o.before_item = Some(&before);
    let (r, e) = migration::run(&o);
    assert!(e.unwrap().contains("interrupted"));
    assert_eq!(r.unwrap().items.unwrap()[1].status, "failed");
    fs::write(format!("{}/config.toml", o.source_root), "changed").unwrap();
    o.before_item = None;
    let (r, e) = migration::run(&o);
    assert!(e.is_none(), "{e:?}");
    let r = r.unwrap();
    assert!(r.resumed && r.complete);
    assert_eq!(r.items.unwrap()[0].status, "skipped");
    assert_eq!(
        fs::read(format!("{}/config.toml", o.destination_root)).unwrap(),
        b"fixture"
    );
}
#[test]
fn migration_manual_and_explicit_secrets() {
    use std::cell::Cell;
    struct Store(Cell<usize>);
    impl migration::SecretStore for Store {
        fn inspect(&self, _: &str) -> Result<migration::SecretReport, String> {
            Ok(migration::SecretReport {
                detected: true,
                migratable: true,
                ..Default::default()
            })
        }
        fn migrate(&self, _: &str) -> Result<(), String> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }
    let (_root, mut o) = fixture();
    write(&o, "identity.enc");
    let store = Store(Cell::new(0));
    o.secret_store = Some(&store);
    let (r, e) = migration::run(&o);
    assert!(e.is_none());
    assert!(!r.unwrap().complete);
    assert_eq!(store.0.get(), 0);
    o.copy_secrets = true;
    let (r, e) = migration::run(&o);
    assert!(e.is_none());
    assert!(r.unwrap().complete);
    assert_eq!(store.0.get(), 1);
}
#[test]
fn migration_metadata_copy_refused_before_backup() {
    let (_root, mut o) = fixture();
    write(&o, "identity.enc");
    o.copy_secrets = true;
    let (r, e) = migration::run(&o);
    assert!(r.is_some());
    assert!(e.unwrap().contains("no migratable"));
    assert!(!std::path::Path::new(&o.destination_root).exists());
}
#[test]
fn migration_separate_config_and_backup_overlap() {
    let (root, mut o) = fixture();
    o.source_config_root = root.path().join("config-source").to_string_lossy().into();
    o.destination_config_root = root
        .path()
        .join("config-destination")
        .to_string_lossy()
        .into();
    fs::create_dir(&o.source_config_root).unwrap();
    fs::write(format!("{}/config.toml", o.source_config_root), "config").unwrap();
    o.backup_dir = format!("{}/backup", o.source_config_root);
    assert!(migration::run(&o).1.unwrap().contains("outside"));
    assert!(!std::path::Path::new(&o.destination_root).exists());
    o.backup_dir = root.path().join("backup").to_string_lossy().into();
    let (r, e) = migration::run(&o);
    assert!(e.is_none(), "{e:?}");
    assert!(r.unwrap().complete);
    assert_eq!(
        fs::read(format!("{}/config-0/config.toml", o.backup_dir)).unwrap(),
        b"config"
    );
    assert_eq!(
        fs::read(format!("{}/config.toml", o.destination_config_root)).unwrap(),
        b"config"
    );
}
#[cfg(unix)]
#[test]
fn migration_symlinks_fail_without_destination_mutation() {
    use std::os::unix::fs::symlink;
    let (root, o) = fixture();
    write(&o, "config.toml");
    symlink(
        root.path().join("outside"),
        format!("{}/unrecognized-link", o.source_root),
    )
    .unwrap();
    let (_, e) = migration::run(&o);
    assert!(e.unwrap().contains("refusing symlink"));
    assert!(!std::path::Path::new(&o.destination_root).exists());
    fs::remove_file(format!("{}/unrecognized-link", o.source_root)).unwrap();
    let (_, e) = migration::run(&o);
    assert!(e.unwrap().contains("without a complete marker"));
}
#[test]
fn migration_rejects_state_and_backup_identity() {
    let (_root, o) = fixture();
    write(&o, "config.toml");
    fs::create_dir(&o.destination_root).unwrap();
    let state = format!("{}/.migration-state.json", o.destination_root);
    for content in ["{", "{}"] {
        fs::write(&state, content).unwrap();
        assert!(migration::run(&o).1.is_some());
    }
    fs::remove_file(state).unwrap();
    let backup = format!("{}.migration-backup", o.destination_root);
    fs::create_dir(&backup).unwrap();
    fs::write(format!("{backup}/.complete.json"), "{}").unwrap();
    assert!(
        migration::run(&o)
            .1
            .unwrap()
            .contains("does not match this source")
    );
}
#[test]
fn migration_native_scheduler_backup_and_replacement() {
    let (root, mut o) = fixture();
    o.platform = "systemd".into();
    o.home_dir = root.path().join("home").to_string_lossy().into();
    o.binary_path = root.path().join("bin/symeraseme").to_string_lossy().into();
    o.project_dir = root.path().join("project").to_string_lossy().into();
    let native = format!("{}/.config/systemd/user", o.home_dir);
    fs::create_dir_all(&native).unwrap();
    let unit = format!("{native}/symeraseme-tick.service");
    fs::write(&unit, "ExecStart=python3 -m symeraseme tick\n").unwrap();
    let (r, e) = migration::run(&o);
    assert!(e.is_none(), "{e:?}");
    let r = r.unwrap();
    assert!(r.complete);
    let content = fs::read_to_string(&unit).unwrap();
    assert!(!content.contains("python3") && !content.contains("__WRAPPER_DIR__"));
    assert!(
        std::path::Path::new(&format!(
            "{}/external/000-symeraseme-tick.service",
            r.backup_dir
        ))
        .exists()
    );
}
#[cfg(unix)]
#[test]
fn migration_preserves_modes_and_rejects_destination_symlink() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (root, o) = fixture();
    write(&o, "config.toml");
    fs::set_permissions(
        format!("{}/config.toml", o.source_root),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    let (r, e) = migration::run(&o);
    assert!(e.is_none());
    let r = r.unwrap();
    for p in [
        format!("{}/config.toml", o.destination_root),
        format!("{}/source/config.toml", r.backup_dir),
    ] {
        assert_eq!(fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o640)
    }
    let (_r, mut second) = fixture();
    second.destination_root = root.path().join("linked").to_string_lossy().into();
    symlink(&o.destination_root, &second.destination_root).unwrap();
    assert!(migration::run(&second).1.unwrap().contains("unsafe"));
}
#[cfg(unix)]
#[test]
fn migration_scheduler_destination_symlink_is_not_followed() {
    use std::os::unix::fs::symlink;
    let (root, o) = fixture();
    write(&o, "schedules/symeraseme-tick.sh");
    fs::write(
        format!("{}/schedules/symeraseme-tick.sh", o.source_root),
        "python3 -m symeraseme tick",
    )
    .unwrap();
    fs::create_dir(&o.destination_root).unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, format!("{}/schedules", o.destination_root)).unwrap();
    let (r, e) = migration::run(&o);
    assert!(e.unwrap().contains("destination path is unsafe"));
    assert!(
        r.unwrap()
            .items
            .unwrap()
            .iter()
            .any(|i| i.status == "failed")
    );
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
#[test]
fn migration_secret_failures_keep_report_and_state() {
    struct Store;
    impl migration::SecretStore for Store {
        fn inspect(&self, _: &str) -> Result<migration::SecretReport, String> {
            Ok(migration::SecretReport {
                detected: true,
                migratable: true,
                ..Default::default()
            })
        }
        fn migrate(&self, _: &str) -> Result<(), String> {
            Err("synthetic transfer failure".into())
        }
    }
    let (_root, mut o) = fixture();
    o.secret_store = Some(&Store);
    o.copy_secrets = true;
    let (r, e) = migration::run(&o);
    assert_eq!(
        e.unwrap(),
        "migrate secret store: synthetic transfer failure"
    );
    let r = r.unwrap();
    assert_eq!(r.items.unwrap()[0].error, "synthetic transfer failure");
    assert!(!r.complete);
}
#[test]
fn migration_auxiliary_paths_and_config_overlap() {
    let (_root, mut o) = fixture();
    o.home_dir = "relative".into();
    assert_eq!(
        migration::detect(&o).unwrap_err(),
        "home must be an absolute path"
    );
    o.home_dir.clear();
    o.source_config_root = format!("{}/config", o.destination_root);
    o.destination_config_root = format!("{}-config", o.destination_root);
    assert_eq!(
        migration::detect(&o).unwrap_err(),
        "source config directory must not overlap destination"
    );
}
