//! Which recorded Go sources must still match the current checkout.
//!
//! `go build` never compiles `_test.go` files, so a later test-only edit
//! cannot change a frozen binary capture. Packages whose Go tests were
//! themselves recorded as capture controls stay fully pinned. The immutable
//! `git show <revision>:<path>` checks still cover every recorded file.
//!
//! A recorded `.gitattributes` only matters for how it checks out the other
//! recorded files, so it is compared through those attributes instead.

use std::ffi::OsStr;
use std::process::Command;

const RECORDED_GO_TEST_PACKAGES: [&str; 2] =
    ["internal/campaign/", "rust-tests/parity/oracle/storage/"];

pub fn current_tree_bound(name: &str) -> bool {
    name != ".gitattributes" && !name.ends_with("_test.go")
        || RECORDED_GO_TEST_PACKAGES.iter().any(|package| {
            name.strip_prefix(package)
                .is_some_and(|rest| !rest.contains('/'))
        })
}

/// Call only after authenticating the archived bytes against the producer's
/// recorded size and digest. Go sources and capture generators stay byte-exact.
/// The four local release labels cannot change the Go observation; every other
/// lock byte, including dependencies, checksums and line endings, stays pinned.
#[allow(dead_code)] // Only verifiers that compare authenticated archives call this.
pub fn assert_current_matches_archive(name: &str, archived: &[u8], current: &[u8]) {
    if name != "Cargo.lock" {
        assert!(current == archived, "current producer input: {name}");
        return;
    }
    let mut expected = std::str::from_utf8(archived)
        .expect("authenticated Cargo.lock must be UTF-8")
        .to_owned();
    let newline = if expected.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    for package in [
        "parity",
        "symeraseme-cli",
        "symeraseme-core",
        "symeraseme-engine",
    ] {
        let name_line = format!("name = \"{package}\"");
        assert_eq!(
            expected.lines().filter(|line| *line == name_line).count(),
            1,
            "exactly one local package stanza: {package}"
        );
        let prefix = format!("[[package]]{newline}{name_line}{newline}version = \"");
        let start = expected
            .find(&prefix)
            .expect("canonical local package stanza")
            + prefix.len();
        let end = start
            + expected[start..]
                .find(&format!("\"{newline}"))
                .expect("complete local package version line");
        expected.replace_range(start..end, env!("CARGO_PKG_VERSION"));
    }
    assert!(
        current == expected.as_bytes(),
        "only local release versions may differ in Cargo.lock"
    );
}

// Only the verifiers that record `.gitattributes` call this.
#[allow(dead_code)]
pub fn assert_checkout_attributes_unchanged<I>(root: &str, revision: &str, names: I)
where
    I: IntoIterator + Clone,
    I::Item: AsRef<OsStr>,
{
    let attributes = |source: Option<String>| {
        let output = Command::new("git")
            .current_dir(root)
            .arg("check-attr")
            .args(source)
            .args(["-a", "--"])
            .args(names.clone())
            .output()
            .unwrap();
        assert!(output.status.success(), "git check-attr");
        output.stdout
    };
    assert_eq!(
        attributes(Some(format!("--source={revision}"))),
        attributes(None),
        "checkout attributes of recorded files changed"
    );
}
