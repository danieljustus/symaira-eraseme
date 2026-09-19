//! Replays `rust-tests/parity/oracle/imap-transport/transcript_cases.json` — the
//! command transcript the Go `NetIMAPDialer` produced against the package's
//! scripted server — against the Rust plain-TCP transport.
//!
//! Compared per case: the normalised transcript (every client line with its
//! leading tag token replaced by `<tag>`), the selected UIDVALIDITY, the search
//! UID list, the fetched messages (uid, flags, header, body) and the exact
//! error string. Cases marked `replay: "go-only"` are skipped with an explicit
//! assertion that they carry a reason, so a skipped case can never be mistaken
//! for covered behaviour.

#[path = "support/imap_server.rs"]
mod imap_server;

use imap_server::{FolderState, MessageState, ScriptedImapServer};
use serde_json::{Value, json};
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

fn server_for(case: &Value) -> ScriptedImapServer {
    let server = ScriptedImapServer::new().expect("scripted server starts");
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
                messages,
            },
        );
    }
    server
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

fn measure(case: &Value, server: &ScriptedImapServer) -> Observed {
    measure_with(
        case,
        server,
        &symeraseme_core::email::imap::ImapDialer::new(),
    )
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
        let server = server_for(&case);
        let observed = measure(&case, &server);
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
        replayed >= 9,
        "expected at least the nine byte-replayable cases, replayed {replayed}"
    );
}

#[test]
fn tls_transport_is_refused_and_says_so() {
    let server = ScriptedImapServer::new().expect("scripted server starts");
    let case = json!({
        "server_mode": "tls",
        "uid_validity": 100,
        "config": {"use_tls": true, "username": "testuser", "password": "testpass", "timeout_seconds": 5.0}
    });
    let observed = measure(&case, &server);
    let error = observed.error.expect("implicit TLS must be refused");
    assert!(
        error.contains("not ported"),
        "the refusal must name the unported transport, got: {error}"
    );
    assert!(
        observed.transcript.is_empty(),
        "a refused TLS connection must not speak IMAP, got: {:?}",
        observed.transcript
    );
}
