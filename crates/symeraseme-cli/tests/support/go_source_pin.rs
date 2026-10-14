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
use std::path::{Component, Path};
use std::process::Command;

const RECORDED_GO_TEST_PACKAGES: [&str; 2] =
    ["internal/campaign/", "rust-tests/parity/oracle/storage/"];

/// Last commit that still contains the Go sources retired by the Rust cutover.
/// Recorded Go bytes are read from this immutable commit, never from the checkout.
#[allow(dead_code)] // Not every test binary that includes this module reads sources.
pub const RETIREMENT_SOURCE_REVISION: &str = "22615ae67325c581fbe60cb0f2d3d2b2139cf900";

#[allow(dead_code)]
fn is_retired_source(name: &str) -> bool {
    name.ends_with(".go")
        || name.ends_with(".go.in")
        || name.ends_with(".gotmpl")
        || name == "go.mod"
        || name == "go.sum"
        || matches!(
            name,
            "scripts/consent-oracle/generate.py"
                | "rust-tests/parity/oracle/migration-state/generate.py"
                | "rust-tests/parity/profile_read.py"
        )
}

/// Immutable Go source or explicitly retired capture script. Missing objects fail; nothing fetches.
#[allow(dead_code)]
pub fn retired_go_source(root: &str, name: &str) -> Result<Vec<u8>, String> {
    let relative = Path::new(name);
    if name.is_empty()
        || name.contains('\\')
        || relative.is_absolute()
        || !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        || !is_retired_source(name)
    {
        return Err(format!("not a retired Go source path: {name:?}"));
    }
    let output = Command::new("git")
        .current_dir(root)
        .args(["cat-file", "blob"])
        .arg(format!("{RETIREMENT_SOURCE_REVISION}:{name}"))
        .output()
        .map_err(|error| format!("git unavailable: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "retired Go source {name} is not in {RETIREMENT_SOURCE_REVISION}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

/// Authenticate a retired Rust capture producer against the independent Go-era pin.
#[allow(dead_code)] // Only retired-producer replay readers use this shared boundary.
pub fn assert_retired_generator_matches_archive(root: &str, name: &str, archived: &[u8]) {
    assert!(
        matches!(
            name,
            "crates/symeraseme-core/tests/campaign_smtp.rs"
                | "crates/symeraseme-core/tests/smtp_transport.rs"
                | "crates/symeraseme-cli/tests/mcp_http_headers_process.rs"
                | "crates/symeraseme-cli/tests/mcp_http_process.rs"
                | "crates/symeraseme-cli/tests/mcp_http_native_contract.rs"
                | "crates/symeraseme-cli/tests/plan_execute_live_process.rs"
                | "crates/symeraseme-cli/tests/support/capture_plan_process.rs"
                | "crates/symeraseme-core/tests/support/capture_smtp.rs"
                | "crates/symeraseme-cli/tests/support/capture_http_headers.rs"
                | "crates/symeraseme-cli/tests/support/capture_http_wire.rs"
                | "crates/symeraseme-cli/tests/support/capture_http_aux.rs"
        ),
        "not a retired capture producer: {name}"
    );
    let pinned = Command::new("git")
        .current_dir(root)
        .args(["cat-file", "blob"])
        .arg(format!("{RETIREMENT_SOURCE_REVISION}:{name}"))
        .output()
        .expect("read independently pinned capture producer");
    assert!(
        pinned.status.success(),
        "retired capture producer unavailable: {name}"
    );
    assert!(
        archived == pinned.stdout,
        "retired capture producer mismatch: {name}"
    );
}

/// Retired Go sources/scripts come from history; all other inputs keep their checkout bytes.
#[allow(dead_code)]
pub fn current_source(root: &str, name: &str) -> Vec<u8> {
    if is_retired_source(name) {
        retired_go_source(root, name).unwrap_or_else(|error| panic!("{error}"))
    } else {
        std::fs::read(Path::new(root).join(name)).unwrap()
    }
}

#[allow(dead_code)] // Source-only consumers do not check whole capture inventories.
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
