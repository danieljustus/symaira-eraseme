use super::model::Broker;
pub use super::validate::LoadReport;
use super::validate::{RegistryError, decode};
use std::collections::HashSet;

include!(concat!(env!("OUT_DIR"), "/embedded_registry.rs"));

const EXPECTED_EMBEDDED_BROKERS: usize = 1_277;
const BROKER_SCHEMA_PATH: &str = "schemas/broker.schema.json";

#[derive(serde::Deserialize)]
struct Manifest {
    schema_version: u64,
    schemas: Schemas,
}

#[derive(serde::Deserialize)]
struct Schemas {
    broker: String,
}

#[derive(serde::Deserialize)]
struct Schema {
    schema_version: u64,
}

/// Loads the committed registry embedded into the binary at build time.
pub fn load_embedded() -> Result<Vec<Broker>, RegistryError> {
    let manifest = embedded("manifest.json")?;
    let schema = embedded(BROKER_SCHEMA_PATH)?;
    validate_metadata(manifest, schema)?;

    let mut brokers = Vec::with_capacity(EXPECTED_EMBEDDED_BROKERS);
    let mut ids = HashSet::with_capacity(EXPECTED_EMBEDDED_BROKERS);
    for (path, bytes) in FILES
        .iter()
        .filter(|(path, _)| path.starts_with("brokers/"))
    {
        let name = path.rsplit('/').next().unwrap_or(path);
        if name.starts_with('_') {
            continue;
        }
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        let source = std::str::from_utf8(bytes).map_err(|_| RegistryError::Validation {
            field: (*path).to_owned(),
            message: "broker document is not UTF-8".to_owned(),
        })?;
        if !ids.insert(stem.to_owned()) {
            return Err(RegistryError::Validation {
                field: "id".to_owned(),
                message: format!("duplicate embedded broker id {stem:?}"),
            });
        }
        let broker = decode(stem, source)
            .map_err(|error| super::validate::with_path(error, std::path::Path::new(path)))?;
        brokers.push(broker);
    }
    if brokers.len() != EXPECTED_EMBEDDED_BROKERS {
        return Err(RegistryError::Validation {
            field: "registry".to_owned(),
            message: format!(
                "embedded broker count {} does not equal {EXPECTED_EMBEDDED_BROKERS}",
                brokers.len()
            ),
        });
    }
    brokers.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(brokers)
}

/// Compatibility alias for callers that select the compiled-in registry.
pub fn load() -> Result<Vec<Broker>, RegistryError> {
    load_embedded()
}

/// Filesystem loading remains available for development and sync validation.
pub fn load_from_dir(root: impl AsRef<std::path::Path>) -> Result<Vec<Broker>, RegistryError> {
    super::validate::load_from_dir(root)
}

/// Filesystem loading variant that reports every per-file validation error.
pub fn load_reporting_from_dir(
    root: impl AsRef<std::path::Path>,
) -> Result<LoadReport, RegistryError> {
    super::validate::load_reporting_from_dir(root)
}

fn embedded(path: &str) -> Result<&'static [u8], RegistryError> {
    FILES
        .iter()
        .find(|(candidate, _)| *candidate == path)
        .map(|(_, bytes)| *bytes)
        .ok_or_else(|| RegistryError::Validation {
            field: path.to_owned(),
            message: "required embedded metadata is missing".to_owned(),
        })
}

fn validate_metadata(manifest_bytes: &[u8], schema_bytes: &[u8]) -> Result<(), RegistryError> {
    let manifest: Manifest =
        serde_json::from_slice(manifest_bytes).map_err(|error| RegistryError::Validation {
            field: "manifest.json".to_owned(),
            message: format!("is malformed: {error}"),
        })?;
    if manifest.schema_version != 1 {
        return Err(RegistryError::Validation {
            field: "manifest.schema_version".to_owned(),
            message: format!("unsupported version {}", manifest.schema_version),
        });
    }
    if manifest.schemas.broker != BROKER_SCHEMA_PATH {
        return Err(RegistryError::Validation {
            field: "manifest.schemas.broker".to_owned(),
            message: "does not identify the pinned broker schema".to_owned(),
        });
    }
    let schema: Schema =
        serde_json::from_slice(schema_bytes).map_err(|error| RegistryError::Validation {
            field: BROKER_SCHEMA_PATH.to_owned(),
            message: format!("is malformed: {error}"),
        })?;
    if schema.schema_version != manifest.schema_version {
        return Err(RegistryError::Validation {
            field: "schema_version".to_owned(),
            message: "manifest and broker schema versions do not match".to_owned(),
        });
    }
    Ok(())
}
