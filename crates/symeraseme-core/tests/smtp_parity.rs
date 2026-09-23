use chrono::DateTime;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use symeraseme_core::email::{
    EmailMessage, SmtpTransport, build_mime_at, recipients, send_message_at,
};

const FIXTURE: &str = include_str!("../../../rust-tests/parity/oracle/email/smtp_cases.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("SMTP oracle fixture parses")
}

fn message(case: &Value) -> EmailMessage {
    let source = &case["input"]["message"];
    EmailMessage {
        to: source["To"].as_str().unwrap_or_default().to_owned(),
        subject: source["Subject"].as_str().unwrap_or_default().to_owned(),
        body: source["Body"].as_str().unwrap_or_default().to_owned(),
        cc: source["CC"].as_str().unwrap_or_default().to_owned(),
        bcc: source["BCC"].as_str().unwrap_or_default().to_owned(),
    }
}

fn actual(case: &Value) -> Value {
    let message = message(case);
    let now = DateTime::parse_from_rfc3339(case["input"]["date"].as_str().unwrap()).unwrap();
    let result = build_mime_at(
        &message,
        case["input"]["from"].as_str().unwrap(),
        now,
        case["input"]["message_id"].as_str().unwrap(),
    );
    match result {
        Ok((raw, message_id)) => json!({
            "name": case["name"],
            "input": case["input"],
            "message": String::from_utf8(raw).unwrap(),
            "message_id": message_id,
            "recipients": recipients(&message),
        }),
        Err(error) => json!({
            "name": case["name"],
            "input": case["input"],
            "recipients": recipients(&message),
            "error": error.to_string(),
        }),
    }
}

#[test]
fn source_bound_go_mime_bytes_recipients_and_errors_match() {
    let document = fixture();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (path, expected) in document["source_sha256"].as_object().unwrap() {
        let digest = hex::encode(Sha256::digest(fs::read(root.join(path)).unwrap()));
        assert_eq!(
            digest,
            expected.as_str().unwrap(),
            "Go source drift: {path}"
        );
    }

    let cases = document["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for case in cases {
        assert_eq!(actual(case), *case, "{}", case["name"]);
    }

    let first = &cases[0];
    let actual = actual(first);
    let mut corrupted = first.clone();
    let raw = first["message"].as_str().unwrap();
    corrupted["message"] = json!(raw.replacen("\r\n", "\n", 1));
    assert_ne!(actual, corrupted, "CRLF mutation must be detected");
}

#[derive(Default)]
struct RecordingTransport {
    recipients: RefCell<Option<Vec<String>>>,
    message: RefCell<Vec<u8>>,
    error: Option<String>,
}

impl SmtpTransport for RecordingTransport {
    fn send(&self, recipients: Option<&[String]>, message: &[u8]) -> Result<(), String> {
        *self.recipients.borrow_mut() = recipients.map(<[String]>::to_vec);
        *self.message.borrow_mut() = message.to_vec();
        self.error.clone().map_or(Ok(()), Err)
    }
}

#[test]
fn outbound_uses_only_the_fake_transport_and_preserves_go_error_text() {
    let document = fixture();
    let case = &document["cases"][0];
    let message = message(case);
    let now = DateTime::parse_from_rfc3339(case["input"]["date"].as_str().unwrap()).unwrap();
    let message_id = case["input"]["message_id"].as_str().unwrap();
    let expected_message = case["message"].as_str().unwrap().as_bytes();
    let expected_recipients = recipients(&message);

    let failing = RecordingTransport {
        error: Some("offline synthetic transport".to_owned()),
        ..RecordingTransport::default()
    };
    let error = send_message_at(
        &message,
        case["input"]["from"].as_str().unwrap(),
        now,
        message_id,
        &failing,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), document["send_error"].as_str().unwrap());
    assert_eq!(*failing.recipients.borrow(), expected_recipients);
    assert_eq!(failing.message.borrow().as_slice(), expected_message);

    let successful = RecordingTransport::default();
    assert_eq!(
        send_message_at(
            &message,
            case["input"]["from"].as_str().unwrap(),
            now,
            message_id,
            &successful,
        )
        .unwrap(),
        message_id
    );
    assert_eq!(*successful.recipients.borrow(), expected_recipients);
    assert_eq!(successful.message.borrow().as_slice(), expected_message);
}

#[test]
fn empty_message_id_uses_go_shaped_random_id_and_boundary() {
    let message = EmailMessage {
        to: "recipient@example.test".to_owned(),
        subject: "subject".to_owned(),
        body: "body".to_owned(),
        cc: String::new(),
        bcc: String::new(),
    };
    let now = DateTime::parse_from_rfc3339("2026-08-31T12:00:00Z").unwrap();
    let (raw, message_id) = build_mime_at(&message, "sender@example.test", now, "").unwrap();
    assert_eq!(message_id.len(), 37);
    assert!(message_id.starts_with('<') && message_id.ends_with("@symeraseme>"));
    assert!(
        message_id[1..25]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    let raw = String::from_utf8(raw).unwrap();
    assert!(raw.contains(&format!("Message-ID: {message_id}\r\n")));
    let expected_boundary = hex::encode(&Sha256::digest(message_id.as_bytes())[..8]);
    assert!(raw.contains(&format!("boundary=\"=_symeraseme_{expected_boundary}\"")));
}
