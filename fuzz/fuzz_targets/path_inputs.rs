#![no_main]

use libfuzzer_sys::fuzz_target;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fuzz_target!(|input: &[u8]| {
    let path = String::from_utf8_lossy(input);
    let root = std::env::temp_dir().join(format!(
        "symeraseme-path-fuzz-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("create isolated path fuzz root");
    fs::write(root.join("fixture.txt"), b"public fixture").expect("write path fuzz fixture");
    let _ = symeraseme_core::redaction::read_workspace_file(Path::new(path.as_ref()), Some(&root));
    fs::remove_dir_all(root).expect("remove isolated path fuzz root");
});
