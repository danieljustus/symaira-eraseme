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
