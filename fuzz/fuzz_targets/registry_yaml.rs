#![no_main]

use libfuzzer_sys::fuzz_target;
use std::fs;
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
    if fs::create_dir(&root).is_err() {
        return;
    }
    if fs::create_dir(&brokers).is_err() || fs::create_dir(&schemas).is_err() {
        let _ = fs::remove_dir_all(&root);
        return;
    }
    if fs::write(root.join("manifest.json"), MANIFEST).is_err()
        || fs::write(schemas.join("broker.schema.json"), SCHEMA).is_err()
        || fs::write(brokers.join("fuzz.yaml"), input).is_err()
    {
        let _ = fs::remove_dir_all(&root);
        return;
    }

    let _ = symeraseme_core::registry::load_reporting_from_dir(&root);
    let _ = fs::remove_dir_all(root);
});
