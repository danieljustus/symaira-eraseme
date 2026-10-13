//! Own binary so the shared helper adds no test to count-gated comparators.
#[path = "support/go_source_pin.rs"]
mod go_source_pin;

use go_source_pin::current_tree_bound;

#[test]
fn only_unrecorded_go_tests_leave_the_current_tree_pin() {
    assert!(current_tree_bound("cmd/symeraseme/main.go"));
    assert!(current_tree_bound("Cargo.lock"));
    assert!(!current_tree_bound(".gitattributes"));
    assert!(current_tree_bound(
        "internal/campaign/plan_bytes_oracle_test.go"
    ));
    assert!(current_tree_bound(
        "rust-tests/parity/oracle/storage/main_test.go"
    ));
    assert!(!current_tree_bound(
        "cmd/symeraseme/release_pipeline_test.go"
    ));
    assert!(!current_tree_bound("internal/campaign/nested/plan_test.go"));
}

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[test]
fn retired_capture_producers_reject_changed_bytes_and_unapproved_paths() {
    for name in [
        "crates/symeraseme-core/tests/campaign_smtp.rs",
        "crates/symeraseme-core/tests/smtp_transport.rs",
        "crates/symeraseme-cli/tests/mcp_http_headers_process.rs",
        "crates/symeraseme-cli/tests/mcp_http_process.rs",
        "crates/symeraseme-cli/tests/mcp_http_native_contract.rs",
    ] {
        let archived = std::process::Command::new("git")
            .current_dir(ROOT)
            .args(["cat-file", "blob"])
            .arg(format!("13ddc6a43036d6b34706db2b82bbb5a13c40d6de:{name}"))
            .output()
            .unwrap();
        assert!(archived.status.success());
        go_source_pin::assert_retired_generator_matches_archive(ROOT, name, &archived.stdout);
        let mut changed = archived.stdout.clone();
        changed.push(b'!');
        for bytes in [&changed[..], &archived.stdout[..archived.stdout.len() - 1]] {
            let error = std::panic::catch_unwind(|| {
                go_source_pin::assert_retired_generator_matches_archive(ROOT, name, bytes);
            })
            .expect_err("the actual producer binding must reject changed bytes");
            let message = error.downcast_ref::<String>().unwrap();
            assert!(message.contains("retired capture producer mismatch"));
        }
    }
    for name in ["Cargo.lock", "../outside.rs", "unapproved.rs"] {
        assert!(
            std::panic::catch_unwind(|| {
                go_source_pin::assert_retired_generator_matches_archive(ROOT, name, b"");
            })
            .is_err()
        );
    }
}

#[test]
fn retired_go_source_bytes_come_from_the_pinned_commit() {
    // Independent of the checkout: the digest is the recorded producer's sha256 for cmd/symeraseme/main.go.
    let bytes = go_source_pin::retired_go_source(ROOT, "cmd/symeraseme/main.go").unwrap();
    assert_eq!(bytes.len(), 5489);
    let digest: String = <sha2::Sha256 as sha2::Digest>::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        digest,
        "f7fd95376ae587dcef1aafeba74b9d3a7aa4c47ac520cd2b82903777cefe8bf7"
    );
    assert_eq!(
        go_source_pin::current_source(ROOT, "go.mod"),
        go_source_pin::retired_go_source(ROOT, "go.mod").unwrap()
    );
}

#[test]
fn retired_template_and_source_do_not_depend_on_checkout_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("checkout");
    let clone = std::process::Command::new("git")
        .args(["clone", "--shared", "--no-checkout", ROOT])
        .arg(&root)
        .output()
        .unwrap();
    assert!(clone.status.success(), "clone immutable source objects");
    let root = root.to_str().unwrap();
    for name in [
        "cmd/symeraseme/main.go",
        "internal/templating/templates/report.html.gotmpl",
        "scripts/consent-oracle/consent_id005_fault_test.go.in",
        "scripts/consent-oracle/consent_id005_test.go.in",
        "scripts/consent-oracle/consent_id005_unix_test.go.in",
        "scripts/consent-oracle/consent_id005_windows_test.go.in",
    ] {
        let path = std::path::Path::new(root).join(name);
        assert!(!path.exists());
        let archived = go_source_pin::retired_go_source(ROOT, name).unwrap();
        assert_eq!(go_source_pin::current_source(root, name), archived);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"not the recorded Go source").unwrap();
        assert_eq!(go_source_pin::current_source(root, name), archived);
    }
    // Non-Go inputs remain bound to the actual candidate, not the historical pin.
    std::fs::write(
        std::path::Path::new(root).join("Cargo.lock"),
        b"candidate lock",
    )
    .unwrap();
    assert_eq!(
        go_source_pin::current_source(root, "Cargo.lock"),
        b"candidate lock"
    );
}

#[test]
fn retired_go_source_rejects_unsafe_or_non_go_paths() {
    for name in [
        "",
        "/etc/passwd.go",
        "../outside.go",
        "internal/../x.go",
        "internal\\mcp\\server.go",
        "Cargo.lock",
        "tests/fixtures/x.json",
    ] {
        assert!(
            go_source_pin::retired_go_source(ROOT, name).is_err(),
            "accepted {name:?}"
        );
    }
}

#[test]
fn retired_go_source_missing_object_fails_clearly() {
    let error = go_source_pin::retired_go_source(ROOT, "internal/does/not/exist.go").unwrap_err();
    assert!(
        error.contains("is not in 22615ae67325c581fbe60cb0f2d3d2b2139cf900"),
        "{error}"
    );
}

#[test]
fn only_first_party_release_lock_versions_may_differ() {
    // Synthetic reader controls, never Go observations or replacement captures.
    let mut archived = String::from(
        "version = 4\n\n[[package]]\nname = \"hmac\"\nversion = \"0.13.0\"\nsource = \"registry+fixture\"\nchecksum = \"fixture-checksum\"\n",
    );
    let mut current = archived.clone();
    for package in [
        "parity",
        "symeraseme-cli",
        "symeraseme-core",
        "symeraseme-engine",
    ] {
        archived.push_str(&format!(
            "\n[[package]]\nname = \"{package}\"\nversion = \"0.13.0\"\ndependencies = [\"hmac\"]\n"
        ));
        current.push_str(&format!(
            "\n[[package]]\nname = \"{package}\"\nversion = \"{}\"\ndependencies = [\"hmac\"]\n",
            env!("CARGO_PKG_VERSION")
        ));
    }
    let check = |name: &str, archive: &str, candidate: &str| {
        go_source_pin::assert_current_matches_archive(
            name,
            archive.as_bytes(),
            candidate.as_bytes(),
        );
    };
    for newline in ["\n", "\r\n"] {
        let archive = archived.replace('\n', newline);
        let candidate = current.replace('\n', newline);
        check("Cargo.lock", &archive, &candidate);
        check("Cargo.lock", &candidate, &candidate);
        for changed in [
            candidate.replace("fixture-checksum", "changed-checksum"),
            candidate.replace("registry+fixture", "registry+changed"),
            candidate.replacen("version = \"0.13.0\"", "version = \"9.9.9\"", 1),
            candidate.replace("dependencies = [\"hmac\"]", "dependencies = [\"changed\"]"),
            candidate.replace("name = \"parity\"", "name = \"missing\""),
            candidate.replace("version = 4", "version = 3"),
            candidate.replace(
                &format!(
                    "name = \"parity\"{newline}version = \"{}\"",
                    env!("CARGO_PKG_VERSION")
                ),
                &format!("name = \"parity\"{newline}version = \"9.9.9\""),
            ),
            format!(
                "{candidate}{newline}[[package]]{newline}name = \"extra\"{newline}version = \"1.0.0\"{newline}"
            ),
            format!(
                "{candidate}{newline}[[package]]{newline}name = \"parity\"{newline}version = \"{}\"{newline}",
                env!("CARGO_PKG_VERSION")
            ),
            format!("{candidate}# unrelated byte{newline}"),
            candidate.replace(newline, if newline == "\n" { "\r\n" } else { "\n" }),
        ] {
            assert!(std::panic::catch_unwind(|| check("Cargo.lock", &archive, &changed)).is_err());
        }
        for bad_archive in [
            archive.replace("name = \"parity\"", "name = \"missing\""),
            format!(
                "{archive}{newline}[[package]]{newline}name = \"parity\"{newline}version = \"0.13.0\"{newline}"
            ),
        ] {
            assert!(
                std::panic::catch_unwind(|| check("Cargo.lock", &bad_archive, &candidate)).is_err()
            );
        }
    }
    check("go.mod", "unchanged", "unchanged");
    for name in [
        "go.mod",
        "cmd/symeraseme/main.go",
        "capture.rs",
        "fuzz/Cargo.lock",
    ] {
        assert!(std::panic::catch_unwind(|| check(name, &archived, &current)).is_err());
    }
}
