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
pub struct Normalizer {
    pub path: String,
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
        Self {
            path: path.into(),
            reason: reason.into(),
            from: from.to_vec(),
            to: to.to_vec(),
        }
    }
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
    pub sqlite_queries: Vec<String>,
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
            sqlite_queries: Vec::new(),
        }
    }
}
