#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::Path;

fuzz_target!(|input: &[u8]| {
    let path = String::from_utf8_lossy(input);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let _ = symeraseme_core::redaction::read_workspace_file(Path::new(path.as_ref()), Some(&root));
});
