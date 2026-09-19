//! The inbox policy: UID handling, the bounded fetch window, reply matching and
//! the text helpers. Ported from `internal/email` (imap.go, inbox.go).

use crate::email::hwm::HwmStore;
use crate::email::parse::parse_fetched_message;
use crate::email::session::{FetchedMessage, ImapDialer};
use crate::email::types::{
    ImapConfig, ImapError, MatchMethod, MatchedMessage, Message, RemovalRequest,
};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// `time.Now().UTC()`. The policy has no clock seam — Go does not have one
/// either — so a since window is only observable by its presence, never by the
/// instant it was computed from.
fn now_utc() -> DateTime<Utc> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    DateTime::<Utc>::from_timestamp(elapsed.as_secs() as i64, elapsed.subsec_nanos())
        .unwrap_or_default()
}

/// The prefix every IMAP failure carries, mirroring Go's `ErrIMAP`.
pub const ERR_IMAP: &str = "email: imap error";

fn error(stage: &str, cause: &str) -> ImapError {
    ImapError::new(format!("{ERR_IMAP}: {stage}: {cause}"))
}

fn error_without_cause(stage: &str) -> ImapError {
    ImapError::new(format!("{ERR_IMAP}: {stage}"))
}

/// `email.imapError`: the stage, then the cause with every credential (raw and
/// base64 encoded) replaced by `[REDACTED]`.
fn imap_error(stage: &str, cause: &str, secrets: &[String]) -> ImapError {
    error(stage, &redact_error(cause, secrets))
}

/// `email.RedactError`: a secret leaks in raw or base64 form, so all five
/// spellings are replaced.
pub fn redact_error(message: &str, secrets: &[String]) -> String {
    let mut redacted = message.to_string();
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        let encoded = [
            secret.clone(),
            base64::engine::general_purpose::STANDARD.encode(secret),
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(secret),
            base64::engine::general_purpose::URL_SAFE.encode(secret),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret),
        ];
        for spelling in encoded {
            redacted = redacted.replace(&spelling, "[REDACTED]");
        }
    }
    redacted
}

/// One UID-based poll of a single folder.
///
/// `Ok(None)` reproduces Go's nil slice — an empty mailbox is reported as
/// `null`, not `[]`, because the caller never received a list at all.
pub fn poll_inbox(
    config: &ImapConfig,
    dialer: &dyn ImapDialer,
    state: &dyn HwmStore,
) -> Result<Option<Vec<Message>>, ImapError> {
    if config.host.is_empty() {
        return Err(error_without_cause("host is empty"));
    }
    poll_inbox_with_defaults(config, dialer, state)
}

/// `email.pollInbox`. The caller has already applied the defaults.
pub fn poll_inbox_with_defaults(
    config: &ImapConfig,
    dialer: &dyn ImapDialer,
    state: &dyn HwmStore,
) -> Result<Option<Vec<Message>>, ImapError> {
    let folder = if config.folder.is_empty() {
        "INBOX".to_string()
    } else {
        config.folder.clone()
    };
    let max_messages = if config.max_messages <= 0 {
        50
    } else {
        config.max_messages as usize
    };
    let secrets = config.secrets();

    let mut session = dialer
        .dial(config)
        .map_err(|cause| imap_error("connect/login failed", &cause, &secrets))?;
    let uid_validity = session
        .select(&folder)
        .map_err(|cause| imap_error("folder select failed", &cause, &secrets))?;
    let (stored_validity, last_uid) = state
        .get(&config.host, &folder)
        .map_err(|cause| imap_error("read high-water mark failed", &cause, &secrets))?;

    let mut start = 1u32;
    if let (Some(validity), Some(last)) = (stored_validity, last_uid)
        && validity == uid_validity
    {
        if last == u32::MAX {
            return Err(error("UID range exhausted", "last UID is UINT32_MAX"));
        }
        start = last + 1;
    }
    let uid_range = format!("{start}:*");
    let since = if config.since_days > 0 {
        Some(now_utc() - Duration::days(config.since_days))
    } else {
        None
    };

    let mut uids = match since {
        Some(since) => session.search_uid(&uid_range, Some(since)),
        None => session.search_uid(&uid_range, None),
    }
    .map_err(|cause| imap_error("UID search failed", &cause, &secrets))?;

    if uids.is_empty() {
        state
            .set(&config.host, &folder, uid_validity, last_uid.unwrap_or(0))
            .map_err(|cause| imap_error("write high-water mark failed", &cause, &secrets))?;
        return Ok(None);
    }

    uids.sort_unstable();
    if uids.len() > max_messages {
        // Consume the oldest pending window first. Keeping the highest UIDs here
        // would advance the HWM past messages that were never fetched.
        uids.truncate(max_messages);
    }

    let fetched = session
        .fetch(&uids)
        .map_err(|cause| imap_error("UID fetch failed", &cause, &secrets))?;
    let by_uid: HashMap<u32, FetchedMessage> = fetched
        .into_iter()
        .map(|message| (message.uid, message))
        .collect();

    let mut parsed = Vec::with_capacity(by_uid.len());
    let mut last_handled = 0u32;
    for uid in uids {
        let Some(message) = by_uid.get(&uid) else {
            return Err(error(
                "UID fetch omitted requested message",
                &format!("UID {uid} missing from FETCH response"),
            ));
        };
        let parsed_message = parse_fetched_message(message).map_err(|cause| {
            error(
                "parse fetched message failed",
                &format!("UID {uid}: {cause}"),
            )
        })?;
        last_handled = uid;
        if let Some(since) = since {
            match parsed_message.date {
                Some(date) if date >= since.fixed_offset() => {}
                _ => continue,
            }
        }
        parsed.push(parsed_message);
    }

    state
        .set(&config.host, &folder, uid_validity, last_handled)
        .map_err(|cause| imap_error("write high-water mark failed", &cause, &secrets))?;
    Ok(Some(parsed))
}

/// `email.PollFolders`: polls each folder and deduplicates by RFC Message-ID.
pub fn poll_folders(
    config: &ImapConfig,
    folders: &[String],
    dialer: &dyn ImapDialer,
    state: &dyn HwmStore,
) -> Result<Option<Vec<Message>>, ImapError> {
    let owned: Vec<String>;
    let folders = if folders.is_empty() {
        owned = vec!["INBOX".to_string()];
        owned.as_slice()
    } else {
        folders
    };
    let mut seen: Vec<String> = Vec::new();
    let mut all: Vec<Message> = Vec::new();
    for folder in folders {
        let mut folder_config = config.clone();
        folder_config.folder = folder.clone();
        let messages = poll_inbox_with_defaults(&folder_config, dialer, state)?;
        for message in messages.unwrap_or_default() {
            let key = if message.message_id.is_empty() {
                message.id.clone()
            } else {
                message.message_id.clone()
            };
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            all.push(message);
        }
    }
    if all.is_empty() {
        return Ok(None);
    }
    Ok(Some(all))
}

/// `email.MatchReplyToRequest`: a recorded thread Message-ID wins over the
/// normalized request subject, and every message keeps an explicit decision.
pub fn match_reply_to_request(
    messages: &[Message],
    requests: &[RemovalRequest],
    thread_map: &HashMap<String, i64>,
) -> Vec<MatchedMessage> {
    let mut subject_index: HashMap<String, i64> = HashMap::new();
    for request in requests {
        let subject = format!("Data Deletion Request — {}", request.broker_id);
        subject_index.insert(normalize_subject(&subject).to_lowercase(), request.id);
    }
    let mut out = Vec::with_capacity(messages.len());
    for message in messages {
        let mut matched = MatchedMessage {
            message: message.clone(),
            request_id: None,
            match_method: MatchMethod::Unmatched,
        };
        let thread_match = if !message.thread_id.is_empty() {
            thread_map.get(&message.thread_id).copied()
        } else {
            None
        };
        if let Some(request_id) = thread_match {
            matched.request_id = Some(request_id);
            matched.match_method = MatchMethod::Thread;
        } else if let Some(request_id) =
            subject_index.get(&normalize_subject(&message.subject).to_lowercase())
        {
            matched.request_id = Some(*request_id);
            matched.match_method = MatchMethod::Subject;
        }
        out.push(matched);
    }
    out
}

/// `email.NormalizeSubject`: strips the reply/forward prefixes common mail
/// clients use, repeatedly, case-insensitively, exactly as Go does.
pub fn normalize_subject(subject: &str) -> String {
    let mut cleaned = subject.trim().to_string();
    loop {
        let lowered = cleaned.to_lowercase();
        let mut matched = false;
        for prefix in ["re", "fwd", "aw", "antwort", "réf", "sv", "vs", "wg", "ref"] {
            let needle = format!("{prefix}:");
            if lowered.starts_with(&needle) {
                cleaned = cleaned[needle.len()..].trim().to_string();
                matched = true;
                break;
            }
        }
        if !matched {
            return cleaned;
        }
    }
}

pub fn subject_matches(base_subject: &str, reply_subject: &str) -> bool {
    normalize_subject(base_subject).to_lowercase()
        == normalize_subject(reply_subject).to_lowercase()
}

/// `email.ParseEmailBody`: trims, then bounds by *bytes* — Go slices the string
/// and lets the JSON encoder repair a cut character.
pub fn parse_email_body(body: &str, max_length: usize) -> String {
    let trimmed = body.trim();
    let max_length = if max_length == 0 { 500 } else { max_length };
    let bytes = trimmed.as_bytes();
    if bytes.len() > max_length {
        let mut out = crate::email::parse::to_valid_utf8_lossy_per_byte(&bytes[..max_length]);
        out.push_str("...");
        return out;
    }
    trimmed.to_string()
}
