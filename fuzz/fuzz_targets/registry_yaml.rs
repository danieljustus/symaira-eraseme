#![no_main]

use libfuzzer_sys::fuzz_target;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

const MANIFEST: &[u8] =
    br#"{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}"#;
const SCHEMA: &[u8] = br#"{"schema_version":1}"#;

fuzz_target!(|input: &[u8]| {
    let root = std::env::temp_dir().join(format!(
        "symeraseme-registry-fuzz-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let brokers = root.join("brokers");
    let schemas = root.join("schemas");
    if fs::create_dir_all(&brokers).is_err() || fs::create_dir_all(&schemas).is_err() {
        return;
    }
    let _ = fs::write(root.join("manifest.json"), MANIFEST);
    let _ = fs::write(schemas.join("broker.schema.json"), SCHEMA);
    let _ = fs::write(brokers.join("fuzz.yaml"), input);

    let _ = symeraseme_core::registry::load_reporting_from_dir(PathBuf::from(&root));
    let _ = fs::remove_dir_all(root);
});
