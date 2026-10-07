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
