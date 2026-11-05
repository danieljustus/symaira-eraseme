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
