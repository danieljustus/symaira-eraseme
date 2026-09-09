//! Non-interactive destructive-operation consent gate.

use super::consent::{ConsentError, ConsentStore};
use std::fs;
use std::io;
use std::path::Path;

/// Inputs to the destructive-operation gate.
#[derive(Clone, Debug, Default)]
pub struct ConsentOptions {
    pub yes: bool,
    pub consent_token: Option<String>,
    pub consent_file: Option<String>,
    pub consent_env_var: Option<String>,
    pub consent_file_env_var: Option<String>,
    /// Interactive prompting is deliberately not implemented in this core
    /// slice. Callers must provide `yes` or a token; MCP therefore fails closed.
    pub interactive: bool,
}

impl ConsentStore {
    /// Evaluate the Go-compatible precedence chain and consume a successful token.
    pub fn authorize(&self, command: &str, options: &ConsentOptions) -> Result<(), ConsentError> {
        if options.yes {
            return Ok(());
        }
        if let Some(token) = options.consent_token.as_deref()
            && !token.is_empty()
        {
            return self.verify_and_consume(command, token);
        }
        if let Some(path) = options.consent_file.as_deref()
            && !path.is_empty()
        {
            return self.authorize_file(command, path);
        }
        let file_env = options
            .consent_file_env_var
            .as_deref()
            .unwrap_or("SYMERASEME_CONSENT_FILE");
        if let Ok(path) = std::env::var(file_env)
            && !path.is_empty()
        {
            return self.authorize_file(command, &path);
        }
        let token_env = options
            .consent_env_var
            .as_deref()
            .unwrap_or("SYMERASEME_CONSENT");
        if let Ok(token) = std::env::var(token_env)
            && !token.is_empty()
        {
            return self.verify_and_consume(command, &token);
        }
        let _ = options.interactive;
        Err(ConsentError::Denied)
    }

    fn verify_and_consume(&self, command: &str, token: &str) -> Result<(), ConsentError> {
        self.verify_token(command, token)?;
        self.consume_token(token)
    }

    fn authorize_file(&self, command: &str, path: &str) -> Result<(), ConsentError> {
        let token = read_consent_file(path).map_err(|_| ConsentError::Denied)?;
        self.verify_and_consume(command, &token)
    }
}

/// Read one token from a consent file, using the first non-empty line.
pub fn read_consent_file(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        return Ok(String::new());
    }
    let contents = fs::read_to_string(path)?;
    let metadata = fs::metadata(path)?;
    let is_standard_input = path == Path::new("/dev/stdin") || path == Path::new("/dev/fd/0");
    if !metadata.is_file() && !is_standard_input {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "consent file is not a regular file",
        ));
    }
    let mut permissions = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o600);
        let _ = fs::set_permissions(path, permissions);
    }
    let text = contents.trim();
    if text.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "consent file is empty",
        ));
    }
    let token = text.lines().next().unwrap_or_default().trim();
    if token.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "consent file is empty",
        ));
    }
    Ok(token.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::consent::ConsentStore;

    #[test]
    fn empty_consent_file_path_is_empty_input() {
        assert_eq!(read_consent_file("").unwrap(), "");
    }

    #[test]
    fn yes_bypasses_token_lookup_but_missing_consent_denies() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConsentStore::new(directory.path()).with_clock(|| 1000);
        assert_eq!(
            store.authorize(
                "delete",
                &ConsentOptions {
                    yes: true,
                    ..Default::default()
                }
            ),
            Ok(())
        );
        assert_eq!(
            store.authorize("delete", &ConsentOptions::default()),
            Err(ConsentError::Denied)
        );
    }

    #[test]
    fn empty_explicit_values_fall_through_to_denial() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConsentStore::new(directory.path());
        let options = ConsentOptions {
            consent_token: Some(String::new()),
            consent_file: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(
            store.authorize("delete", &options),
            Err(ConsentError::Denied)
        );
    }

    #[test]
    fn explicit_token_has_priority_and_is_consumed() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConsentStore::new(directory.path())
            .with_clock(|| 1000)
            .with_random_source(|length| Ok((0..length).map(|value| value as u8).collect()));
        let token = store.issue_token("delete", 60).unwrap();
        let options = ConsentOptions {
            consent_token: Some(token.clone()),
            ..Default::default()
        };
        assert_eq!(store.authorize("delete", &options), Ok(()));
        assert_eq!(
            store.verify_token("delete", &token),
            Err(ConsentError::NotFound)
        );
    }

    #[test]
    fn consent_file_reads_first_line_and_is_consumed() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConsentStore::new(directory.path())
            .with_clock(|| 1000)
            .with_random_source(|length| Ok((0..length).map(|value| value as u8).collect()));
        let token = store.issue_token("delete", 60).unwrap();
        let token_file = directory.path().join("input.token");
        fs::write(&token_file, format!("{token}\nignored\n")).unwrap();
        let options = ConsentOptions {
            consent_file: Some(token_file.to_string_lossy().into_owned()),
            ..Default::default()
        };
        assert_eq!(store.authorize("delete", &options), Ok(()));
        assert_eq!(
            store.verify_token("delete", &token),
            Err(ConsentError::NotFound)
        );
    }
}
