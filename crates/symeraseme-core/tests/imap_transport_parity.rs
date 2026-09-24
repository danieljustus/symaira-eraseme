//! Replays `rust-tests/parity/oracle/imap-transport/transcript_cases.json` — the
//! command transcript the Go `NetIMAPDialer` produced against the package's
//! scripted server — against the Rust plain-TCP transport.
//!
//! Compared per case: the normalised transcript (every client line with its
//! leading tag token replaced by `<tag>`), the selected UIDVALIDITY, the search
//! UID list, the fetched messages (uid, flags, header, body) and the exact
//! error string. Cases marked `replay: "go-only"` are skipped with an explicit
//! assertion that they carry a reason, so a skipped case can never be mistaken
//! for covered behaviour; the fixture currently marks every case byte-replayable,
//! including the two TLS ones, and the suite asserts that both TLS cases really
//! ran rather than being skipped somewhere else.
//!
//! The TLS cases record no certificate material: each side mints its own
//! localhost certificate for the run and trusts exactly that one, which is why
//! the transcript and the results stay comparable.

#[path = "support/imap_server.rs"]
mod imap_server;

use imap_server::{FolderState, MessageState, ScriptedImapServer, TlsMode};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{RootCertStore, ServerConfig};
use serde_json::{Value, json};
use std::process::Command;
use std::sync::Arc;
use symeraseme_core::email::session::{FetchedMessage, ImapDialer};
use symeraseme_core::email::types::{ImapConfig, OAuth2Token};

const FIXTURE: &str =
    include_str!("../../../rust-tests/parity/oracle/imap-transport/transcript_cases.json");

fn cases() -> Vec<Value> {
    let document: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
    document["cases"].as_array().expect("cases array").clone()
}

/// The oracle replaces the leading tag token with `<tag>` before recording, so
/// the comparison covers the command sequence and its arguments rather than the
/// per-connection tag numbering.
fn normalise(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line.split_once(' ') {
            Some((_, rest)) => format!("<tag> {rest}"),
            None => line.clone(),
        })
        .collect()
}

#[derive(Debug, Default)]
struct Observed {
    transcript: Vec<String>,
    uid_validity: Option<u32>,
    uids: Option<Vec<u32>>,
    fetched: Option<Vec<Value>>,
    error: Option<String>,
}

/// Mints a localhost certificate for the run and returns the server
/// configuration plus the root store that trusts exactly this certificate. No
/// key material ever reaches the repository or the fixture.
fn tls_material() -> (Arc<ServerConfig>, RootCertStore) {
    let certified = generate_simple_self_signed(vec!["127.0.0.1".to_string()])
        .expect("localhost certificate is minted");
    let certificate: CertificateDer<'static> = certified.cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(vec![certificate.clone()], key)
        .expect("server certificate is usable");
    let mut roots = RootCertStore::empty();
    roots
        .add(certificate)
        .expect("minted certificate is a trust anchor");
    (Arc::new(config), roots)
}

/// The server the case was recorded against, plus the roots the client has to
/// trust for it.
fn server_for(case: &Value) -> (ScriptedImapServer, Option<RootCertStore>) {
    let (server, roots) = match case["server_mode"].as_str().unwrap_or("plain") {
        "tls" => {
            let (config, roots) = tls_material();
            (
                ScriptedImapServer::new_tls(config, TlsMode::Implicit).expect("TLS server starts"),
                Some(roots),
            )
        }
        "starttls" => {
            let (config, roots) = tls_material();
            (
                ScriptedImapServer::new_tls(config, TlsMode::StartTls)
                    .expect("STARTTLS server starts"),
                Some(roots),
            )
        }
        _ => (
            ScriptedImapServer::new().expect("scripted server starts"),
            None,
        ),
    };
    let uid_validity = case["uid_validity"].as_u64().expect("uid_validity") as u32;
    let messages: Vec<MessageState> = case["messages"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| MessageState {
                    uid: entry["uid"].as_u64().expect("uid") as u32,
                    flags: entry["flags"]
                        .as_array()
                        .map(|flags| {
                            flags
                                .iter()
                                .map(|flag| flag.as_str().expect("flag").to_string())
                                .collect()
                        })
                        .unwrap_or_default(),
                    header: entry["header"].as_str().expect("header").to_string(),
                    body: entry["body"].as_str().expect("body").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    {
        let mut folders = server.folders.lock().expect("folders lock");
        folders.insert(
            "INBOX".to_string(),
            FolderState {
                uid_validity,
                messages: messages.clone(),
            },
        );
        // The Go oracle's server creates an unknown folder on demand and reports
        // its own default UIDVALIDITY (7).
        folders.insert(
            "Missing".to_string(),
            FolderState {
                uid_validity: 7,
                messages: messages.clone(),
            },
        );
        if case["name"] == "select_unicode_folder_uses_modified_utf7" {
            folders.insert(
                "&AMQ-rger &- Archiv".to_string(),
                FolderState {
                    uid_validity: 1,
                    messages,
                },
            );
        }
    }
    (server, roots)
}

fn config_for(case: &Value, server: &ScriptedImapServer) -> ImapConfig {
    let recorded = &case["config"];
    let oauth2 = recorded["oauth2_access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .map(|token| OAuth2Token {
            username: recorded["oauth2_username"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            access_token: token.to_string(),
        });
    ImapConfig {
        host: "127.0.0.1".to_string(),
        port: i64::from(server.port),
        username: recorded["username"].as_str().unwrap_or("").to_string(),
        password: recorded["password"].as_str().unwrap_or("").to_string(),
        use_tls: recorded["use_tls"].as_bool().unwrap_or(false),
        folder: "INBOX".to_string(),
        since_days: 0,
        max_messages: 0,
        oauth2,
        timeout_seconds: recorded["timeout_seconds"].as_f64().unwrap_or(5.0) as i64,
        allow_insecure_cleartext_auth: recorded["allow_insecure_cleartext_auth"]
            .as_bool()
            .unwrap_or(false),
    }
}

fn fetched_to_value(message: &FetchedMessage) -> Value {
    json!({
        "uid": message.uid,
        "flags": message.flags.clone().unwrap_or_default(),
        "header": String::from_utf8(message.header.clone()).expect("header is utf-8"),
        "body": String::from_utf8(message.body.clone()).expect("body is utf-8"),
        "internal_date_set": message.internal_date.is_some(),
    })
}

fn measure(case: &Value, server: &ScriptedImapServer, roots: Option<RootCertStore>) -> Observed {
    let dialer = match roots {
        Some(roots) => symeraseme_core::email::imap::ImapDialer::with_root_certificates(roots),
        None => symeraseme_core::email::imap::ImapDialer::new(),
    };
    measure_with(case, server, &dialer)
}

fn measure_with(case: &Value, server: &ScriptedImapServer, dialer: &impl ImapDialer) -> Observed {
    let mut observed = Observed::default();
    let config = config_for(case, server);
    let mut session = match dialer.dial(&config) {
        Ok(session) => session,
        Err(error) => {
            observed.error = Some(error);
            observed.transcript = normalise(&server.get_transcript());
            return observed;
        }
    };
    for op in case["ops"].as_array().expect("ops") {
        let op = op.as_str().expect("op");
        let (command, argument) = op.split_once(':').unwrap_or((op, ""));
        let result = match command {
            "select" => session.select(argument).map(|uid_validity| {
                observed.uid_validity = Some(uid_validity);
            }),
            "search" => session.search_uid(argument, None).map(|uids| {
                observed.uids = Some(uids);
            }),
            "fetch" => {
                let uids: Vec<u32> = argument
                    .split(',')
                    .filter_map(|value| value.trim().parse::<u32>().ok())
                    .collect();
                session.fetch(&uids).map(|messages| {
                    observed.fetched = Some(messages.iter().map(fetched_to_value).collect());
                })
            }
            "close" => {
                session.close();
                Ok(())
            }
            other => panic!("unknown op {other}"),
        };
        if let Err(error) = result {
            observed.error = Some(error);
            break;
        }
    }
    observed.transcript = normalise(&server.get_transcript());
    observed
}

#[test]
fn imap_transport_transcript_cases_match_the_go_oracle() {
    let mut replayed = 0;
    let mut tls_replayed = 0;
    for case in cases() {
        let name = case["name"].as_str().expect("case name").to_string();
        if case["replay"].as_str() == Some("go-only") {
            let reason = case["replay_reason"].as_str().unwrap_or("");
            assert!(
                !reason.is_empty(),
                "{name}: a skipped case must state why it cannot be replayed"
            );
            continue;
        }
        let (server, roots) = server_for(&case);
        // STARTTLS starts in cleartext, so `use_tls` alone would miss it; the
        // recorded server mode is what says a case exercises TLS.
        if case["server_mode"].as_str().unwrap_or("plain") != "plain" {
            tls_replayed += 1;
        }
        let observed = measure(&case, &server, roots);
        replayed += 1;

        let expected_transcript: Vec<String> = case["transcript"]
            .as_array()
            .expect("transcript")
            .iter()
            .map(|line| line.as_str().expect("line").to_string())
            .collect();
        assert_eq!(
            observed.transcript, expected_transcript,
            "{name}: the command transcript differs from the Go oracle"
        );

        if let Some(error) = case["error"].as_str() {
            assert_eq!(
                observed.error.as_deref(),
                Some(error),
                "{name}: the error text differs from Go"
            );
            continue;
        }
        assert_eq!(observed.error, None, "{name}: unexpected error");

        if let Some(uid_validity) = case["selected_uid_validity"].as_u64() {
            assert_eq!(
                observed.uid_validity.map(u64::from),
                Some(uid_validity),
                "{name}: selected UIDVALIDITY differs from Go"
            );
        }
        if let Some(uids) = case["search_uids"].as_array() {
            let expected: Vec<u32> = uids
                .iter()
                .map(|uid| uid.as_u64().expect("uid") as u32)
                .collect();
            assert_eq!(
                observed.uids.as_ref(),
                Some(&expected),
                "{name}: the search result differs from Go"
            );
        }
        if let Some(fetched) = case["fetched"].as_array() {
            let expected: Vec<Value> = fetched
                .iter()
                .map(|entry| {
                    json!({
                        "uid": entry["uid"],
                        "flags": entry["flags"].clone(),
                        "header": entry["header"],
                        "body": entry["body"],
                        "internal_date_set": entry["internal_date_set"],
                    })
                })
                .collect();
            assert_eq!(
                observed.fetched.as_ref(),
                Some(&expected),
                "{name}: the fetched messages differ from Go"
            );
        }
    }
    assert!(
        replayed >= 11,
        "expected all eleven byte-replayable cases, replayed {replayed}"
    );
    assert!(
        tls_replayed >= 2,
        "both TLS cases must really run rather than being skipped, replayed {tls_replayed}"
    );
}

#[test]
fn the_default_platform_root_store_rejects_an_untrusted_certificate() {
    let (config, _trusted_roots) = tls_material();
    let server = ScriptedImapServer::new_tls(config, TlsMode::Implicit).expect("TLS server starts");
    let case = tls_case();
    let observed = measure(&case, &server, None);
    let error = observed
        .error
        .expect("the platform roots must not trust a locally minted certificate");
    assert!(
        error.contains("UnknownIssuer") || error.contains("certificate"),
        "the rejection must be a certificate failure, got: {error}"
    );
}

#[test]
fn the_default_platform_root_store_trusts_a_configured_platform_root() {
    const CHILD_MODE: &str = "SYMERASEME_IMAP_TRUST_TEST_CHILD";
    if std::env::var_os(CHILD_MODE).is_some() {
        let cert_path = std::env::var("SYMERASEME_IMAP_TEST_CERT").expect("server cert path");
        let key_path = std::env::var("SYMERASEME_IMAP_TEST_KEY").expect("server key path");
        let cert = CertificateDer::from_pem_file(cert_path).expect("server cert parses");
        let key = PrivateKeyDer::from_pem_file(key_path).expect("server key parses");
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("server certificate is usable");
        let server = ScriptedImapServer::new_tls(Arc::new(config), TlsMode::Implicit)
            .expect("TLS server starts");
        let observed = measure(&tls_case(), &server, None);
        assert_eq!(
            observed.error, None,
            "the default TLS roots must trust the certificate from SSL_CERT_FILE"
        );
        return;
    }

    let certified = generate_simple_self_signed(vec!["127.0.0.1".to_string()])
        .expect("test certificate is minted");
    let temp = tempfile::tempdir().expect("temporary certificate directory");
    let cert_path = temp.path().join("server.pem");
    let key_path = temp.path().join("server-key.pem");
    let root_path = temp.path().join("roots.pem");
    std::fs::write(&cert_path, certified.cert.pem()).expect("server certificate is written");
    std::fs::write(&key_path, certified.signing_key.serialize_pem())
        .expect("server key is written");
    std::fs::write(&root_path, certified.cert.pem()).expect("trust root is written");

    let result = Command::new(std::env::current_exe().expect("test executable path"))
        .args([
            "--exact",
            "the_default_platform_root_store_trusts_a_configured_platform_root",
            "--nocapture",
        ])
        .env(CHILD_MODE, "1")
        .env("SSL_CERT_FILE", &root_path)
        .env("SSL_CERT_DIR", "")
        .env("SYMERASEME_IMAP_TEST_CERT", &cert_path)
        .env("SYMERASEME_IMAP_TEST_KEY", &key_path)
        .output()
        .expect("child trust test runs");
    assert!(
        result.status.success(),
        "default root trust child failed: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn the_injected_root_store_is_what_makes_that_certificate_trusted() {
    let (config, roots) = tls_material();
    let server = ScriptedImapServer::new_tls(config, TlsMode::Implicit).expect("TLS server starts");
    let case = tls_case();
    let observed = measure(&case, &server, Some(roots));
    assert_eq!(
        observed.error, None,
        "the injected root store must make the minted certificate trusted"
    );
    assert!(
        !observed.transcript.is_empty(),
        "a trusted TLS connection must speak IMAP"
    );
}

fn tls_case() -> Value {
    json!({
        "server_mode": "tls",
        "uid_validity": 100,
        "ops": ["close"],
        "config": {
            "use_tls": true,
            "username": "testuser",
            "password": "testpass",
            "timeout_seconds": 5.0,
            "allow_insecure_cleartext_auth": false
        }
    })
}
