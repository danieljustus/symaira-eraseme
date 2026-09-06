//! Version and build metadata for the public Symaira EraseMe contract.

use serde::Serialize;

/// The stable public name used by the CLI and GUI handshake.
pub const TOOL_NAME: &str = "symeraseme";
/// The version of the machine-readable handshake payload.
pub const SCHEMA_VERSION: u8 = 1;
/// The version injected by Cargo at build time, with no runtime clock input.
pub const BUILD_VERSION: &str = match option_env!("SYMERASEME_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// The machine-readable version handshake shared with Symaira clients.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VersionInfo {
    pub tool: String,
    pub version: String,
    pub schema_version: u8,
}

impl VersionInfo {
    /// Build a handshake payload from a version string.
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            tool: TOOL_NAME.to_owned(),
            version: version.into(),
            schema_version: SCHEMA_VERSION,
        }
    }

    /// Serialize the compact handshake followed by its contractual newline.
    pub fn json_line(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut bytes = serde_json::to_vec(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Render the human-readable version command payload.
    pub fn text(&self) -> String {
        format!("{} {}", self.tool, self.version)
    }
}

/// Return the deterministic metadata for this build.
pub fn current() -> VersionInfo {
    VersionInfo::new(BUILD_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_metadata_is_deterministic() {
        assert_eq!(current().tool, TOOL_NAME);
        assert_eq!(current().version, BUILD_VERSION);
        assert_eq!(current().schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn json_uses_contract_order_and_newline() {
        assert_eq!(
            VersionInfo::new("0.13.0").json_line().unwrap(),
            b"{\"tool\":\"symeraseme\",\"version\":\"0.13.0\",\"schema_version\":1}\n"
        );
    }

    #[test]
    fn json_serialization_escapes_untrusted_version_text() {
        assert_eq!(
            VersionInfo::new("quote\" newline\n").json_line().unwrap(),
            b"{\"tool\":\"symeraseme\",\"version\":\"quote\\\" newline\\n\",\"schema_version\":1}\n"
        );
    }
}
