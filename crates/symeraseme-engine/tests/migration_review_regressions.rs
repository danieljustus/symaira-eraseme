#[cfg(unix)]
mod unix {
    use serde_json::{Value, json};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use symeraseme_engine::migration::{Options, run};

    #[test]
    fn scheduler_read_failures_match_pinned_go_observations() {
        let fixture: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../rust-tests/parity/oracle/migration-safety/io.json"
        )))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        let expected_ids = [
            "generated-unreadable",
            "native-unreadable",
            "generated-directory",
            "native-directory",
        ];
        assert_eq!(cases.len(), expected_ids.len());
        for (case, expected_id) in cases.iter().zip(expected_ids) {
            assert_eq!(case["id"], expected_id);
            let root = tempfile::tempdir().unwrap();
            let root = root.path();
            let options = Options {
                source_root: root.join("source").to_string_lossy().into_owned(),
                destination_root: root.join("destination").to_string_lossy().into_owned(),
                home_dir: root.join("home").to_string_lossy().into_owned(),
                platform: "systemd".into(),
                binary_path: root.join("bin/symeraseme").to_string_lossy().into_owned(),
                project_dir: root.join("project").to_string_lossy().into_owned(),
                ..Default::default()
            };
            fs::create_dir_all(&options.source_root).unwrap();
            fs::create_dir_all(&options.home_dir).unwrap();
            let native = expected_id.starts_with("native-");
            let directory = expected_id.ends_with("-directory");
            let unit = if native {
                root.join("home/.config/systemd/user/symeraseme-tick.service")
            } else {
                root.join("source/schedules/symeraseme-tick.service")
            };
            fs::create_dir_all(unit.parent().unwrap()).unwrap();
            if directory {
                fs::create_dir(&unit).unwrap();
                fs::set_permissions(&unit, fs::Permissions::from_mode(0o700)).unwrap();
            } else {
                fs::write(&unit, b"ExecStart=python3 -m symeraseme tick\n").unwrap();
                fs::set_permissions(&unit, fs::Permissions::from_mode(0o000)).unwrap();
                assert_eq!(
                    fs::read(&unit).unwrap_err().kind(),
                    std::io::ErrorKind::PermissionDenied,
                    "unreadable fixture must fail before migration; do not run as root"
                );
            }
            let (report, error) = run(&options);
            let info = fs::metadata(&unit).unwrap();
            let mode = info.permissions().mode() & 0o777;
            let contents = if directory {
                String::new()
            } else {
                fs::set_permissions(&unit, fs::Permissions::from_mode(0o600)).unwrap();
                let data = fs::read_to_string(&unit).unwrap();
                fs::set_permissions(&unit, fs::Permissions::from_mode(mode)).unwrap();
                data
            };
            let actual = json!({
                "error": error.unwrap_or_default().replace(root.to_str().unwrap(), "<ROOT>"),
                "report": report, "scheduler_contents": contents, "scheduler_mode": mode,
                "scheduler_is_directory": info.is_dir(),
                "destination_exists": root.join("destination").exists(),
                "backup_exists": root.join("destination.migration-backup").exists(),
            });
            assert_eq!(actual, case["observation"], "{expected_id}");
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::fs;
    use std::path::Path;
    use symeraseme_engine::migration::{Options, run};

    fn options(root: &Path) -> Options<'_> {
        Options {
            source_root: root.join("Legacy").to_string_lossy().into_owned(),
            destination_root: root.join("Target").to_string_lossy().into_owned(),
            home_dir: root.join("home").to_string_lossy().into_owned(),
            platform: "cron".into(),
            binary_path: root.join("symeraseme.exe").to_string_lossy().into_owned(),
            project_dir: root.join("project").to_string_lossy().into_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn case_aliased_source_destination_and_backup_are_rejected_without_writes() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        fs::create_dir(root.join("Legacy")).unwrap();
        fs::write(root.join("Legacy/config.toml"), b"locale = 'de'\n").unwrap();
        for (destination, backup) in [
            (root.join("legacy"), root.join("backup")),
            (root.join("LEGACY/child"), root.join("backup")),
            (root.join("Target"), root.join("legacy/backup")),
            (root.join("Target"), root.join("target/backup")),
        ] {
            let mut opts = options(root);
            opts.destination_root = destination.to_string_lossy().into_owned();
            opts.backup_dir = backup.to_string_lossy().into_owned();
            let expected_report = destination == root.join("Target");
            let (report, error) = run(&opts);
            assert_eq!(report.is_some(), expected_report);
            assert!(error.is_some());
            assert_eq!(fs::read_dir(root).unwrap().count(), 1);
            assert_eq!(fs::read_dir(root.join("Legacy")).unwrap().count(), 1);
            assert_eq!(
                fs::read(root.join("Legacy/config.toml")).unwrap(),
                b"locale = 'de'\n"
            );
        }
    }

    #[test]
    fn whole_source_backup_and_destination_preserve_windows_readonly() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        fs::create_dir(root.join("Legacy")).unwrap();
        let source = root.join("Legacy/config.toml");
        fs::write(&source, b"locale = 'de'\n").unwrap();
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&source, permissions).unwrap();
        let (report, error) = run(&options(root));
        assert!(error.is_none(), "{error:?}");
        let report = report.unwrap();
        assert!(report.complete);
        for path in [
            source,
            root.join("Target/config.toml"),
            Path::new(&report.backup_dir).join("source/config.toml"),
        ] {
            assert!(
                fs::metadata(&path).unwrap().permissions().readonly(),
                "{}",
                path.display()
            );
            assert_eq!(fs::read(path).unwrap(), b"locale = 'de'\n");
        }
        assert!(
            !fs::metadata(root.join("Target/.migration-state.json"))
                .unwrap()
                .permissions()
                .readonly()
        );
    }
}
