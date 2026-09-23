//! Replays every case recorded by `rust-tests/parity/oracle/email` — the Go
//! `internal/email` policy oracle — against the Rust port.
//!
//! The comparisons are byte-exact: the recorded answers are JSON text, and the
//! order of the fields is part of the contract, so the fixture text is compared
//! with insignificant whitespace removed instead of being parsed into a map.

use chrono::{DateTime, FixedOffset, Utc};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use symeraseme_core::email::config::{ImapConfigOptions, load_imap_config_with};
use symeraseme_core::email::hwm::{HwmStore, MemoryHwmStore};
use symeraseme_core::email::policy::{parse_email_body, poll_folders, poll_inbox};
use symeraseme_core::email::service::{InboxService, ReplyStore};
use symeraseme_core::email::session::{FetchedMessage, ImapDialer, ImapSession};
use symeraseme_core::email::types::{
    ImapConfig, MatchedMessage, Message, OAuth2Token, RemovalRequest,
};
use symeraseme_core::email::wire::{matched_messages_json, optional_messages_json};
use symeraseme_core::email::{
    match_reply_to_request, normalize_subject, parse_fetched_message, subject_matches,
};

const FIXTURE: &str = include_str!("../../../rust-tests/parity/oracle/email/email_cases.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("oracle fixture parses")
}

/// Removes insignificant whitespace while keeping string literals untouched, so
/// two JSON texts can be compared including their field order.
fn compact(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut in_string = false;
    let mut escaped = false;
    for character in json.chars() {
        if in_string {
            out.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => {
                in_string = true;
                out.push(character);
            }
            ' ' | '\t' | '\n' | '\r' => {}
            _ => out.push(character),
        }
    }
    out
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

// ---------------------------------------------------------------------------
// A scripted session. It reports what the policy asked for and answers from the
// recorded script, exactly like the Go oracle's fake.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct FetchedSpec {
    uid: u32,
    header: Vec<u8>,
    body: Vec<u8>,
    flags: Option<Vec<String>>,
    internal_date: Option<DateTime<Utc>>,
}

#[derive(Clone, Default)]
struct FolderScript {
    uid_validity: u32,
    uids: Vec<u32>,
    fetched: Vec<FetchedSpec>,
    omit: Vec<u32>,
    select_error: Option<String>,
    search_error: Option<String>,
    fetch_error: Option<String>,
}

#[derive(Default)]
struct Recorder {
    searches: Vec<(String, String, bool)>,
    fetches: Vec<Vec<u32>>,
}

struct ScriptedDialer {
    folders: HashMap<String, FolderScript>,
    recorder: Arc<Mutex<Recorder>>,
}

impl ImapDialer for ScriptedDialer {
    fn dial(&self, config: &ImapConfig) -> Result<Box<dyn ImapSession>, String> {
        let Some(script) = self.folders.get(&config.folder) else {
            return Err(format!(
                "scripted dialer: no script for folder {:?}",
                config.folder
            ));
        };
        Ok(Box::new(ScriptedSession {
            script: script.clone(),
            folder: config.folder.clone(),
            recorder: self.recorder.clone(),
        }))
    }
}

struct ScriptedSession {
    script: FolderScript,
    folder: String,
    recorder: Arc<Mutex<Recorder>>,
}

impl ImapSession for ScriptedSession {
    fn select(&mut self, _folder: &str) -> Result<u32, String> {
        match &self.script.select_error {
            Some(error) => Err(error.clone()),
            None => Ok(self.script.uid_validity),
        }
    }

    fn search_uid(
        &mut self,
        uid_range: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<u32>, String> {
        self.recorder.lock().expect("recorder lock").searches.push((
            self.folder.clone(),
            uid_range.to_string(),
            since.is_some(),
        ));
        match &self.script.search_error {
            Some(error) => Err(error.clone()),
            None => Ok(self.script.uids.clone()),
        }
    }

    fn fetch(&mut self, uids: &[u32]) -> Result<Vec<FetchedMessage>, String> {
        self.recorder
            .lock()
            .expect("recorder lock")
            .fetches
            .push(uids.to_vec());
        if let Some(error) = &self.script.fetch_error {
            return Err(error.clone());
        }
        Ok(self
            .script
            .fetched
            .iter()
            .filter(|message| !self.script.omit.contains(&message.uid))
            .map(|message| FetchedMessage {
                uid: message.uid,
                header: message.header.clone(),
                body: message.body.clone(),
                flags: message.flags.clone(),
                internal_date: message.internal_date,
            })
            .collect())
    }

    fn close(&mut self) {}
}

fn build_script(value: &Value) -> HashMap<String, FolderScript> {
    let mut folders = HashMap::new();
    let Some(entries) = value.as_object() else {
        return folders;
    };
    for (folder, script) in entries {
        let mut parsed = FolderScript {
            uid_validity: script["uid_validity"].as_u64().unwrap_or(0) as u32,
            uids: script["uids"]
                .as_array()
                .map(|uids| {
                    uids.iter()
                        .filter_map(|uid| uid.as_u64().map(|uid| uid as u32))
                        .collect()
                })
                .unwrap_or_default(),
            omit: script["omit"]
                .as_array()
                .map(|uids| {
                    uids.iter()
                        .filter_map(|uid| uid.as_u64().map(|uid| uid as u32))
                        .collect()
                })
                .unwrap_or_default(),
            select_error: text(&script["select_error"]),
            search_error: text(&script["search_error"]),
            fetch_error: text(&script["fetch_error"]),
            fetched: Vec::new(),
        };
        if let Some(fetched) = script["fetched"].as_array() {
            parsed.fetched = fetched
                .iter()
                .map(|message| FetchedSpec {
                    uid: message["uid"].as_u64().unwrap_or(0) as u32,
                    header: text(&message["header"]).unwrap_or_default().into_bytes(),
                    body: text(&message["body"]).unwrap_or_default().into_bytes(),
                    flags: message["flags"].as_array().map(|flags| {
                        flags
                            .iter()
                            .filter_map(|flag| flag.as_str().map(str::to_string))
                            .collect()
                    }),
                    internal_date: text(&message["internal_date"])
                        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
                        .map(|value| value.with_timezone(&Utc)),
                })
                .collect();
        }
        folders.insert(folder.clone(), parsed);
    }
    folders
}

fn seed_hwm(store: &MemoryHwmStore, entries: &Value) {
    if let Some(entries) = entries.as_array() {
        for entry in entries {
            let (Some(validity), Some(last)) =
                (entry["uid_validity"].as_u64(), entry["last_uid"].as_u64())
            else {
                continue;
            };
            store
                .set(
                    entry["host"].as_str().unwrap_or_default(),
                    entry["folder"].as_str().unwrap_or_default(),
                    validity as u32,
                    last as u32,
                )
                .expect("seed hwm");
        }
    }
}

fn hwm_snapshot(store: &MemoryHwmStore, host: &str, folders: &[String]) -> Value {
    let mut out = Vec::new();
    for folder in folders {
        let (validity, last) = store.get(host, folder).expect("read hwm");
        if validity.is_none() && last.is_none() {
            continue;
        }
        out.push(serde_json::json!({
            "host": host,
            "folder": folder,
            "uid_validity": validity,
            "last_uid": last,
        }));
    }
    out.sort_by(|left, right| left["folder"].as_str().cmp(&right["folder"].as_str()));
    Value::Array(out)
}

fn folders_of(case: &Value) -> Vec<String> {
    case["folders"]
        .as_array()
        .map(|folders| {
            folders
                .iter()
                .filter_map(|folder| folder.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| vec!["INBOX".to_string()])
}

#[test]
fn recorded_normalized_subjects_match() {
    for case in fixture()["normalize_subject"].as_array().expect("cases") {
        let input = case["input"].as_str().expect("input");
        assert_eq!(
            normalize_subject(input),
            case["output"].as_str().unwrap(),
            "case {}",
            case["name"]
        );
    }
}

#[test]
fn recorded_email_bodies_match() {
    for case in fixture()["parse_email_body"].as_array().expect("cases") {
        let max = case["max"].as_i64().unwrap_or(0).max(0) as usize;
        assert_eq!(
            parse_email_body(case["body"].as_str().expect("body"), max),
            case["output"].as_str().unwrap(),
            "case {}",
            case["name"]
        );
    }
}

#[test]
fn recorded_subject_matches_agree() {
    for case in fixture()["subject_matches"].as_array().expect("cases") {
        assert_eq!(
            subject_matches(
                case["base"].as_str().expect("base"),
                case["reply"].as_str().expect("reply")
            ),
            case["matches"].as_bool().unwrap(),
            "case {}",
            case["name"]
        );
    }
}

#[test]
fn recorded_reply_matches_agree() {
    for case in fixture()["match_reply_to_request"]
        .as_array()
        .expect("cases")
    {
        let messages: Vec<Message> = case["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|message| Message {
                id: message["ID"].as_str().unwrap_or_default().to_string(),
                subject: message["Subject"].as_str().unwrap_or_default().to_string(),
                from: String::new(),
                to: String::new(),
                date: None,
                body: String::new(),
                flags: None,
                message_id: message["MessageID"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                thread_id: message["ThreadID"].as_str().unwrap_or_default().to_string(),
                imap_uid: message["IMAPUID"].as_u64().unwrap_or(0) as u32,
            })
            .collect();
        let requests: Vec<RemovalRequest> = case["requests"]
            .as_array()
            .map(|requests| {
                requests
                    .iter()
                    .map(|request| RemovalRequest {
                        id: request["ID"].as_i64().unwrap_or(0),
                        broker_id: request["BrokerID"].as_str().unwrap_or_default().to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let thread_map: HashMap<String, i64> = case["thread_map"]
            .as_object()
            .map(|map| {
                map.iter()
                    .map(|(key, value)| (key.clone(), value.as_i64().unwrap_or(0)))
                    .collect()
            })
            .unwrap_or_default();
        let matched = match_reply_to_request(&messages, &requests, &thread_map);
        assert_eq!(
            compact(&matched_messages_json(&matched)),
            compact(case["output"].as_str().expect("output")),
            "case {}",
            case["name"]
        );
    }
}

#[test]
fn recorded_parsed_messages_match() {
    for case in fixture()["parse_fetched_message"]
        .as_array()
        .expect("cases")
    {
        let fetched = FetchedMessage {
            uid: 1,
            header: case["header"]
                .as_str()
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
            body: case["body"]
                .as_str()
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
            flags: case["flags"].as_array().map(|flags| {
                flags
                    .iter()
                    .filter_map(|flag| flag.as_str().map(str::to_string))
                    .collect()
            }),
            internal_date: text(&case["internal_date"])
                .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
                .map(|value| value.with_timezone(&Utc)),
        };
        // The fixture records the UID as a running index over the case list.
        let index = fixture()["parse_fetched_message"]
            .as_array()
            .expect("cases")
            .iter()
            .position(|entry| entry["name"] == case["name"])
            .expect("case index");
        let fetched = FetchedMessage {
            uid: index as u32 + 1,
            ..fetched
        };

        match parse_fetched_message(&fetched) {
            Ok(message) => assert_eq!(
                compact(&symeraseme_core::email::wire::message_json(&message)),
                compact(case["output"].as_str().expect("output")),
                "case {}",
                case["name"]
            ),
            Err(error) => assert_eq!(
                error,
                case["error"].as_str().unwrap_or_default(),
                "case {}",
                case["name"]
            ),
        }
    }
}

#[test]
fn recorded_polls_agree() {
    for case in fixture()["poll_inbox"].as_array().expect("cases") {
        let host = case["host"].as_str().expect("host");
        let folders = folders_of(case);
        let config = ImapConfig {
            host: host.to_string(),
            folder: folders[0].clone(),
            max_messages: case["max_messages"].as_i64().unwrap_or(0),
            since_days: case["since_days"].as_i64().unwrap_or(0),
            password: case["password"].as_str().unwrap_or_default().to_string(),
            oauth2: case["oauth2_access_token"]
                .as_str()
                .map(|token| OAuth2Token {
                    username: case["oauth2_username"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    access_token: token.to_string(),
                }),
            ..ImapConfig::default()
        };
        let state = MemoryHwmStore::new();
        seed_hwm(&state, &case["initial_hwm"]);

        for (index, recorded) in case["calls"].as_array().expect("calls").iter().enumerate() {
            let recorder = Arc::new(Mutex::new(Recorder::default()));
            let dialer = ScriptedDialer {
                folders: build_script(&case["script"]),
                recorder: recorder.clone(),
            };
            let result: Result<Option<Vec<Message>>, _> = if folders.len() > 1 {
                poll_folders(&config, &folders, &dialer, &state)
            } else {
                poll_inbox(&config, &dialer, &state)
            };
            let name = format!("{} call {index}", case["name"]);
            match result {
                Ok(messages) => {
                    assert_eq!(
                        compact(&optional_messages_json(messages.as_deref())),
                        compact(recorded["messages"].as_str().expect("messages")),
                        "{name}"
                    );
                }
                Err(error) => {
                    assert_eq!(
                        error.message(),
                        recorded["error"].as_str().unwrap_or_default(),
                        "{name}"
                    );
                }
            }

            let recorder = recorder.lock().expect("recorder lock");
            let searches: Vec<Value> = recorder
                .searches
                .iter()
                .map(|(folder, uid_range, since)| {
                    serde_json::json!({"folder": folder, "uid_range": uid_range, "since_set": since})
                })
                .collect();
            assert_eq!(
                Value::Array(searches),
                recorded["searches"],
                "{name} searches"
            );
            let fetches: Vec<Value> = recorder
                .fetches
                .iter()
                .map(|uids| serde_json::json!(uids))
                .collect();
            assert_eq!(Value::Array(fetches), recorded["fetches"], "{name} fetches");
        }

        assert_eq!(
            hwm_snapshot(&state, host, &folders),
            case["final_hwm"],
            "{} final hwm",
            case["name"]
        );
    }
}

struct RecordingStore {
    inserts: RefCell<Vec<Value>>,
}

impl ReplyStore for RecordingStore {
    fn insert(&self, reply: &MatchedMessage, snippet: &str) -> Result<(), String> {
        self.inserts.borrow_mut().push(serde_json::json!({
            "request_id": reply.request_id,
            "message_id": if reply.message.message_id.is_empty() { reply.message.id.clone() } else { reply.message.message_id.clone() },
            "thread_id": reply.message.thread_id,
            "from": reply.message.from,
            "subject": reply.message.subject,
            "snippet": snippet,
        }));
        Ok(())
    }
}

struct FailingStore;

impl ReplyStore for FailingStore {
    fn insert(&self, _reply: &MatchedMessage, _snippet: &str) -> Result<(), String> {
        Err("reply insert failed".to_string())
    }
}

#[test]
fn recorded_service_runs_agree() {
    for case in fixture()["poll_and_match"].as_array().expect("cases") {
        let host = case["host"].as_str().expect("host");
        let folders = folders_of(case);
        let requests: Vec<RemovalRequest> = case["requests"]
            .as_array()
            .expect("requests")
            .iter()
            .map(|request| RemovalRequest {
                id: request["ID"].as_i64().unwrap_or(0),
                broker_id: request["BrokerID"].as_str().unwrap_or_default().to_string(),
            })
            .collect();
        let thread_map: HashMap<String, i64> = case["thread_map"]
            .as_object()
            .map(|map| {
                map.iter()
                    .map(|(key, value)| (key.clone(), value.as_i64().unwrap_or(0)))
                    .collect()
            })
            .unwrap_or_default();
        let config = ImapConfig {
            host: host.to_string(),
            folder: folders[0].clone(),
            ..ImapConfig::default()
        };
        let state = MemoryHwmStore::new();
        let recorder = Arc::new(Mutex::new(Recorder::default()));
        let dialer = ScriptedDialer {
            folders: build_script(&case["script"]),
            recorder,
        };
        let service = InboxService::new(&dialer, Some(&state));
        let store = RecordingStore {
            inserts: RefCell::new(Vec::new()),
        };
        let failing = FailingStore;
        let insert_error = case["error"]
            .as_str()
            .is_some_and(|error| error.contains("persist inbox reply"));
        let reply_store: &dyn ReplyStore = if insert_error { &failing } else { &store };

        let result =
            service.poll_and_match(&config, &folders, &requests, &thread_map, Some(reply_store));
        match result {
            Ok(matched) => assert_eq!(
                compact(&matched_messages_json(&matched)),
                compact(case["output"].as_str().expect("output")),
                "{} output",
                case["name"]
            ),
            Err(error) => assert_eq!(
                error.message(),
                case["error"].as_str().unwrap_or_default(),
                "{} error",
                case["name"]
            ),
        }
        assert_eq!(
            Value::Array(store.inserts.borrow().clone()),
            case["inserts"],
            "{} inserts",
            case["name"]
        );
        assert_eq!(
            hwm_snapshot(&state, host, &folders),
            case["final_hwm"],
            "{} hwm",
            case["name"]
        );
    }
}

#[test]
fn recorded_imap_configs_agree() {
    for case in fixture()["imap_config"].as_array().expect("cases") {
        let environment: BTreeMap<String, String> = case["env"]
            .as_object()
            .map(|env| {
                env.iter()
                    .map(|(key, value)| {
                        (key.clone(), value.as_str().unwrap_or_default().to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let options = ImapConfigOptions {
            oauth2_access_token: text(&case["access_token"]),
            oauth2_username: text(&case["oauth2_username"]),
        };
        match load_imap_config_with(options, &environment) {
            Ok(config) => {
                let expected: Value =
                    serde_json::from_str(case["config"].as_str().expect("config"))
                        .expect("config json");
                let actual = serde_json::json!({
                    "host": config.host,
                    "port": config.port,
                    "username": config.username,
                    "use_tls": config.use_tls,
                    "folder": config.folder,
                    "since_days": config.since_days,
                    "max_messages": config.max_messages,
                    "password_set": !config.password.is_empty(),
                    "oauth2": config.oauth2.as_ref().map(|oauth2| serde_json::json!({
                        "username": oauth2.username,
                        "access_token_set": !oauth2.access_token.is_empty(),
                    })),
                });
                assert_eq!(actual, expected, "{} config", case["name"]);
            }
            Err(error) => assert_eq!(
                error,
                case["error"].as_str().unwrap_or_default(),
                "{}",
                case["name"]
            ),
        }
    }
}

#[test]
fn fixed_offset_dates_render_like_go() {
    let date = DateTime::parse_from_rfc3339("2026-07-21T10:00:00+02:00").unwrap();
    assert_eq!(
        symeraseme_core::email::wire::format_go_timestamp(&date),
        "2026-07-21T10:00:00+02:00"
    );
    let utc: DateTime<FixedOffset> = DateTime::parse_from_rfc3339("2026-07-22T09:30:00Z").unwrap();
    assert_eq!(
        symeraseme_core::email::wire::format_go_timestamp(&utc),
        "2026-07-22T09:30:00Z"
    );
}
