//! Rust-owned goldens for the Symaira-designed HTML exports (dashboard, report).
//!
//! Until the Liquid Topography redesign these files were compared byte for byte
//! against frozen Go output. The frozen Go records stay untouched as history;
//! the rendered HTML is compared here instead (see docs/rust-port-contract-matrix.md,
//! CLI-019 and DOM-003). Regenerate with `SYMERASEME_UPDATE_HTML_GOLDENS=1` and
//! review the diff like any other UI change.

use std::path::PathBuf;

pub fn assert_html_golden(name: &str, actual: &str) {
    assert_no_external_requests(name, actual);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/html-exports")
        .join(format!("{name}.html"));
    if std::env::var_os("SYMERASEME_UPDATE_HTML_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().expect("golden directory")).expect("create goldens");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{name}: missing golden {}: {error}", path.display()));
    assert!(
        actual == expected,
        "{name}: HTML export differs from tests/fixtures/html-exports/{name}.html; \
         regenerate with SYMERASEME_UPDATE_HTML_GOLDENS=1 and review the diff"
    );
}

/// A privacy tool's local export must not call out when opened (fonts, scripts, images).
pub fn assert_no_external_requests(name: &str, html: &str) {
    for needle in ["http://", "https://", "//fonts.", "@import"] {
        assert!(
            !html.contains(needle),
            "{name}: HTML export references {needle}"
        );
    }
}
