//! Read-only Go identity profile discovery and authenticated loading.
//!
//! This is the identity JSON-header/AES-GCM envelope, NOT the event-store
//! encryption format. The caller supplies paths and an existing-key resolver;
//! loading never initializes a key, creates directories, or rewrites a profile.

use super::{KeyringBackend, MasterKeyError, MasterKeyResolver};
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// The complete Go profile fields, including normalized empty collections.
/// Deliberately has no `Debug` implementation: profiles contain personal data.
#[derive(Clone, Default, Eq, PartialEq, Serialize)]
pub struct Profile {
    pub full_name: String,
    #[serde(serialize_with = "serialize_go_cloned_slice")]
    pub name_variants: Vec<String>,
    pub date_of_birth: Option<String>,
    pub addresses: Vec<ProfileAddress>,
    #[serde(serialize_with = "serialize_go_cloned_slice")]
    pub email_addresses: Vec<String>,
    #[serde(serialize_with = "serialize_go_cloned_slice")]
    pub phone_numbers: Vec<String>,
    #[serde(serialize_with = "serialize_go_cloned_slice")]
    pub jurisdictions: Vec<String>,
}

// LoadProfile normalizes slices and then clone() uses append([]string(nil),
// values...), returning nil for empty string slices. Addresses use make and
// remain []. Preserve the measured public loader result, not normalize alone.
fn serialize_go_cloned_slice<S: serde::Serializer>(
    values: &[String],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if values.is_empty() {
        serializer.serialize_none()
    } else {
        values.serialize(serializer)
    }
}

/// Address fields retained by the Go identity model, including optional dates.
#[derive(Clone, Default, Eq, PartialEq, Serialize)]
pub struct ProfileAddress {
    pub street: String,
    pub city: String,
    pub postal_code: String,
    pub country: String,
    pub state: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

// Go encoding/json matches tagged names case-insensitively, processes duplicate
// fields in input order, ignores unknown fields, and accepts null zero values.
// Ordinary serde derives reject duplicates/null and are not equivalent.
macro_rules! read_field {
    ($map:ident, $target:expr, scalar) => {
        if let Some(value) = $map.next_value::<Option<String>>()? {
            $target = value;
        }
    };
    ($map:ident, $target:expr, optional) => {
        $target = $map.next_value::<Option<String>>()?;
    };
    ($map:ident, $target:expr, strings) => {
        $target = $map
            .next_value::<Option<Vec<Option<String>>>>()?
            .unwrap_or_default()
            .into_iter()
            .map(Option::unwrap_or_default)
            .collect();
    };
    ($map:ident, $target:expr, addresses) => {
        $target = $map
            .next_value::<Option<Vec<ProfileAddress>>>()?
            .unwrap_or_default();
    };
    ($map:ident, $target:expr, integer) => {
        if let Some(value) = $map.next_value::<Option<i64>>()? {
            $target = value;
        }
    };
}

macro_rules! go_record {
    ($record:ident { $($field:ident: $kind:ident),* $(,)? }) => {
        impl<'de> Deserialize<'de> for $record {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct RecordVisitor;
                impl<'de> Visitor<'de> for RecordVisitor {
                    type Value = $record;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str("an identity object or null")
                    }
                    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                        Ok($record::default())
                    }
                    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                        let mut record = $record::default();
                        while let Some(name) = map.next_key::<String>()? {
                            match name.to_lowercase().replace('ſ', "s").as_str() {
                                $(stringify!($field) => { read_field!(map, record.$field, $kind); })*
                                _ => { map.next_value::<IgnoredAny>()?; }
                            }
                        }
                        Ok(record)
                    }
                }
                deserializer.deserialize_any(RecordVisitor)
            }
        }
    };
}

go_record!(Profile {
    full_name: scalar,
    name_variants: strings,
    date_of_birth: optional,
    addresses: addresses,
    email_addresses: strings,
    phone_numbers: strings,
    jurisdictions: strings,
});
go_record!(ProfileAddress {
    street: scalar,
    city: scalar,
    postal_code: scalar,
    country: scalar,
    state: optional,
    valid_from: optional,
    valid_to: optional,
});

#[derive(Default)]
struct Envelope {
    version: i64,
    nonce: String,
    algorithm: String,
}
go_record!(Envelope {
    version: integer,
    nonce: scalar,
    algorithm: scalar
});

/// Opaque errors: never retain filenames, JSON excerpts, keys or plaintext.
#[derive(Debug, Eq, PartialEq)]
pub enum ProfileError {
    NotFound,
    Stat,
    Read,
    NoSeparator,
    Header,
    LegacyV0,
    Key(MasterKeyError),
    Nonce,
    Authentication,
    Json,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFound => "identity: profile not found",
            Self::Stat => "identity: stat profile failed",
            Self::Read => "identity: read profile failed",
            Self::NoSeparator => "identity: profile corrupt: no header separator",
            Self::Header => "identity: profile corrupt: header",
            Self::LegacyV0 => "identity: legacy v0 profile (no AAD) is no longer supported",
            Self::Key(error) => return error.fmt(f),
            Self::Nonce => "identity: profile corrupt: nonce",
            Self::Authentication => {
                "identity: profile corrupt: cipher: message authentication failed"
            }
            Self::Json => "identity: profile corrupt",
        })
    }
}
impl std::error::Error for ProfileError {}

/// Explicit environment/home snapshot for profile discovery. No filesystem
/// operations occur during construction. Tests need not mutate process globals.
#[derive(Clone, Default)]
pub struct ProfilePaths {
    home: Option<PathBuf>,
    environment: BTreeMap<String, String>,
}

impl ProfilePaths {
    pub fn new(home: Option<PathBuf>, environment: BTreeMap<String, String>) -> Self {
        Self { home, environment }
    }

    /// Process adapter, equivalent to Go's `os.UserHomeDir` and path overrides.
    pub fn from_process() -> Self {
        let home_variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let home = std::env::var_os(home_variable)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let environment = [
            "SYMERASEME_IDENTITY_PATH",
            "SYMERASEME_DATA_DIR",
            "SYMERASEME_CONFIG_DIR",
        ]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok().map(|v| (name.to_owned(), v)))
        .collect();
        Self::new(home, environment)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.environment
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    fn expand(&self, path: &Path) -> PathBuf {
        if let Some(home) = &self.home {
            if path == Path::new("~") {
                return home.clone();
            }
            if let Some(value) = path.to_str().and_then(|v| v.strip_prefix("~/")) {
                return home.join(value);
            }
        }
        path.to_owned()
    }

    /// Explicit path, identity override, data override, then config directory.
    /// Only the two historical identity basenames are eligible for fallback.
    pub fn resolve(&self, path: &Path) -> PathBuf {
        let target = if !path.as_os_str().is_empty() {
            self.expand(path)
        } else if let Some(value) = self.value("SYMERASEME_IDENTITY_PATH") {
            self.expand(Path::new(value))
        } else {
            let directory = self
                .value("SYMERASEME_DATA_DIR")
                .or_else(|| self.value("SYMERASEME_CONFIG_DIR"))
                .unwrap_or("~/.config/symeraseme");
            self.expand(Path::new(directory)).join("identity.encrypted")
        };
        // Do not fallback from permission or other I/O failures to a different
        // identity. This fail-closed distinction is intentional.
        if let Err(error) = std::fs::metadata(&target)
            && error.kind() == std::io::ErrorKind::NotFound
        {
            let alternative = match target.file_name().and_then(|v| v.to_str()) {
                Some("identity.encrypted") => Some("identity.enc"),
                Some("identity.enc") => Some("identity.encrypted"),
                _ => None,
            };
            if let Some(name) = alternative {
                let alternate = target.with_file_name(name);
                if std::fs::metadata(&alternate).is_ok() {
                    return alternate;
                }
            }
        }
        target
    }
}

/// Go-compatible existence probe (directories also exist); does not resolve keys.
pub fn profile_exists(path: &Path, paths: &ProfilePaths) -> bool {
    std::fs::metadata(paths.resolve(path)).is_ok()
}

/// Read and authenticate a profile with existing key sources only.
///
/// Empty `path` selects discovery. This read primitive deliberately re-reads on
/// each call; callers retaining a profile own its lifetime instead of a global
/// plaintext cache. Parser/I/O errors are redacted, not raw Go JSON/OS excerpts.
pub fn load_profile<K: KeyringBackend>(
    path: &Path,
    paths: &ProfilePaths,
    keys: &mut MasterKeyResolver<K>,
) -> Result<Profile, ProfileError> {
    let target = paths.resolve(path);
    let metadata = std::fs::metadata(&target).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProfileError::NotFound
        } else {
            ProfileError::Stat
        }
    })?;
    // Avoid opening blocking special files. Directories report the read error
    // they produce in Go without ever attempting key resolution.
    if !metadata.is_file() {
        return Err(ProfileError::Read);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A regular file swapped for a FIFO between stat/open must not block.
        options.custom_flags(nix::libc::O_NONBLOCK);
    }
    let mut file = options.open(target).map_err(|_| ProfileError::Read)?;
    if !file.metadata().map_err(|_| ProfileError::Read)?.is_file() {
        return Err(ProfileError::Read);
    }
    let mut raw = Vec::new();
    file.read_to_end(&mut raw).map_err(|_| ProfileError::Read)?;
    let separator = raw
        .iter()
        .position(|&v| v == b'\n')
        .ok_or(ProfileError::NoSeparator)?;
    let header_bytes = &raw[..separator];
    let header: Envelope =
        serde_json::from_slice(header_bytes).map_err(|_| ProfileError::Header)?;
    if header.version == 0 {
        return Err(ProfileError::LegacyV0);
    }
    // Go intentionally does not dispatch on algorithm or reject nonzero
    // versions: the original header bytes are authenticated without rewriting.
    let key = keys.resolve_existing().map_err(ProfileError::Key)?;
    let nonce = hex::decode(header.nonce).map_err(|_| ProfileError::Nonce)?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| ProfileError::Nonce)?;
    let cipher =
        Aes256Gcm::new_from_slice(key.as_bytes()).map_err(|_| ProfileError::Authentication)?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: &raw[separator + 1..],
                    aad: header_bytes,
                },
            )
            .map_err(|_| ProfileError::Authentication)?,
    );
    serde_json::from_slice(&plain).map_err(|_| ProfileError::Json)
}
