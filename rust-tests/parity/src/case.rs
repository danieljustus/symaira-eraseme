//! Data-only parity cases. The same case is run against both implementations.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub executable: PathBuf,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureFile {
    pub relative_path: PathBuf,
    pub contents: Vec<u8>,
    pub mode: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct CwdLayout {
    pub files: Vec<FixtureFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct EnvironmentAllowlist {
    pub values: BTreeMap<String, String>,
}

impl EnvironmentAllowlist {
    pub fn with(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(name.into(), value.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComparisonMode {
    Exact,
    JsonSemantic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NormalizerScope {
    Stdout,
    Stderr,
    HttpRequest,
    HttpResponse,
    McpStdin,
    McpStdout,
    Unknown(String),
}

impl NormalizerScope {
    pub fn parse(path: &str) -> Self {
        match path {
            "stdout" => Self::Stdout,
            "stderr" => Self::Stderr,
            "http.request" => Self::HttpRequest,
            "http.response" => Self::HttpResponse,
            "mcp.stdin" => Self::McpStdin,
            "mcp.stdout" => Self::McpStdout,
            unknown => Self::Unknown(unknown.to_owned()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::HttpRequest => "http.request",
            Self::HttpResponse => "http.response",
            Self::McpStdin => "mcp.stdin",
            Self::McpStdout => "mcp.stdout",
            Self::Unknown(name) => name,
        }
    }

    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Normalizer {
    pub path: String,
    pub scope: NormalizerScope,
    pub reason: String,
    pub from: Vec<u8>,
    pub to: Vec<u8>,
}

impl Normalizer {
    pub fn exact(
        path: impl Into<String>,
        reason: impl Into<String>,
        from: &[u8],
        to: &[u8],
    ) -> Self {
        let path = path.into();
        Self {
            scope: NormalizerScope::parse(&path),
            path,
            reason: reason.into(),
            from: from.to_vec(),
            to: to.to_vec(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpFixture {
    pub response: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Case {
    pub id: String,
    pub go: Program,
    pub rust: Program,
    pub stdin: Vec<u8>,
    pub environment: EnvironmentAllowlist,
    pub cwd: CwdLayout,
    pub timeout: Duration,
    pub comparison: ComparisonMode,
    pub normalizers: Vec<Normalizer>,
    pub sqlite_database: Option<PathBuf>,
    pub sqlite_queries: Vec<String>,
    pub http: Option<HttpFixture>,
    pub capture_mcp: bool,
}

impl Case {
    pub fn new(id: impl Into<String>, go: Program, rust: Program) -> Self {
        Self {
            id: id.into(),
            go,
            rust,
            stdin: Vec::new(),
            environment: EnvironmentAllowlist::default(),
            cwd: CwdLayout::default(),
            timeout: Duration::from_secs(10),
            comparison: ComparisonMode::Exact,
            normalizers: Vec::new(),
            sqlite_database: None,
            sqlite_queries: Vec::new(),
            http: None,
            capture_mcp: true,
        }
    }
}

pub fn validate_normalizers(normalizers: &[Normalizer]) -> std::io::Result<()> {
    for rule in normalizers {
        if !rule.scope.is_known() || rule.scope.name() != rule.path {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unknown normalizer scope {:?}", rule.path),
            ));
        }
        if rule.reason.trim().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("normalizer {} requires a non-empty reason", rule.path),
            ));
        }
        if rule.from.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("normalizer {} requires a non-empty source", rule.path),
            ));
        }
    }
    Ok(())
}

pub fn normalizers_for<'a>(
    scope: &NormalizerScope,
    normalizers: &'a [Normalizer],
) -> impl Iterator<Item = &'a Normalizer> {
    normalizers.iter().filter(move |rule| &rule.scope == scope)
}

pub fn normalizer_reasons(scope: &NormalizerScope, normalizers: &[Normalizer]) -> String {
    normalizers_for(scope, normalizers)
        .map(|rule| rule.reason.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizer_validation_rejects_unknown_and_missing_reasons() {
        let unknown = Normalizer::exact("unknown", "reason", b"a", b"b");
        assert!(validate_normalizers(&[unknown]).is_err());

        let missing_reason = Normalizer::exact("stdout", "  ", b"a", b"b");
        assert!(validate_normalizers(&[missing_reason]).is_err());
    }
}
