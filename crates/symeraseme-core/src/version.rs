//! Version and build metadata for the public Symaira EraseMe contract.

pub use symaira_core_version::{Info as VersionInfo, WriteError};

/// The stable public name used by the CLI and GUI handshake.
pub const TOOL_NAME: &str = "symeraseme";
/// The version of the machine-readable handshake shared with Symaira clients.
pub const SCHEMA_VERSION: u8 = 1;
/// The version provided by Cargo package metadata, with no ambient override or
/// runtime clock input.
pub const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build a handshake payload from a version string.
pub fn new(version: impl Into<String>) -> VersionInfo {
    symaira_core_version::new(TOOL_NAME, version, i32::from(SCHEMA_VERSION))
}

/// Return the deterministic metadata for this build.
pub fn current() -> VersionInfo {
    new(BUILD_VERSION)
}

/// Serialize the compact handshake followed by its contractual newline.
pub fn json_line(info: &VersionInfo) -> Result<Vec<u8>, WriteError> {
    let mut bytes = Vec::new();
    info.write(&mut bytes)?;
    Ok(bytes)
}

/// Render the human-readable version command payload.
pub fn text(info: &VersionInfo) -> String {
    info.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_metadata_is_deterministic() {
        assert_eq!(current().tool, TOOL_NAME);
        assert_eq!(current().version, BUILD_VERSION);
        assert_eq!(current().schema_version, i32::from(SCHEMA_VERSION));
    }

    #[test]
    fn json_uses_shared_contract_order_escaping_and_newline() {
        assert_eq!(
            json_line(&new("<&>")).unwrap(),
            b"{\"tool\":\"symeraseme\",\"version\":\"\\u003c\\u0026\\u003e\",\"schema_version\":1}\n"
        );
    }

    #[test]
    fn text_uses_shared_contract_format() {
        assert_eq!(text(&new("0.13.0")), "symeraseme 0.13.0");
    }
}
