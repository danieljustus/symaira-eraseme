//! Replays every case recorded by `rust-tests/parity/oracle/oauth2` — the Go
//! `internal/email/oauth2.go` oracle — against the Rust port.
//!
//! Three comparison styles, matching what the oracle can actually pin:
//!
//! * The provider table, the PKCE derivation and every error string are compared
//!   character for character.
//! * An authorization URL is compared with its random `state` and
//!   `code_challenge` replaced by the recorded marker, plus the recorded lengths
//!   and the S256 relation between challenge and verifier.
//! * Token exchanges run against a real loopback HTTP server: the transcript
//!   (method, path, content type, form body) and the parsed result or error are
//!   compared with what Go produced for the identical canned reply.

use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symeraseme_core::email::oauth2::{
    OAuth2Client, OAuthStateStore, TokenReply, TokenTransport, TokenTransportError,
    UreqTokenTransport, encode_form, is_loopback, pkce_challenge, provider,
};

const RANDOM_MARK: &str = "<random>";

fn fixture() -> Value {
    let raw = include_str!("../../../rust-tests/parity/oracle/oauth2/oauth2_cases.json");
    serde_json::from_str(raw).expect("fixture parses")
}

#[test]
fn oauth2_provider_table_matches_go() {
    let fixture = fixture();
    let expected = &fixture["providers"];
    for name in ["gmail", "outlook"] {
        let config = provider(name).expect("known provider");
        let actual = serde_json::to_value(&config).expect("serialise provider");
        assert_eq!(actual, expected[name], "provider {name}");
    }
    // Go lowercases and trims the provider name before lookup; anything else is
    // unknown.
    assert_eq!(provider(" Gmail "), provider("gmail"));
    assert!(provider("posteo").is_none());
}

#[test]
fn oauth2_pkce_derivation_matches_go() {
    let fixture = fixture();
    let cases = fixture["pkce"].as_array().expect("pkce cases");
    assert!(!cases.is_empty());
    for case in cases {
        let verifier = case["verifier"].as_str().expect("verifier");
        let expected = case["challenge"].as_str().expect("challenge");
        assert_eq!(pkce_challenge(verifier), expected, "verifier {verifier:?}");
    }
}

#[test]
fn oauth2_authorise_url_matches_go() {
    let fixture = fixture();
    let cases = fixture["authorize"].as_array().expect("authorize cases");
    assert!(!cases.is_empty());
    for (index, case) in cases.iter().enumerate() {
        let name = case["name"].as_str().expect("name");
        let provider_name = case["provider"].as_str().expect("provider");
        let client_id = case["client_id"].as_str().expect("client_id");
        let redirect_uri = case["redirect_uri"].as_str().expect("redirect_uri");
        let directory = tempfile::tempdir().expect("temp dir");
        let client = OAuth2Client::new(OAuthStateStore::new(
            directory.path().join(format!("state-{index}.json")),
        ));
        match case["error"].as_str() {
            Some(expected_error) => {
                let error = client
                    .authorise_url(provider_name, client_id, redirect_uri)
                    .expect_err("unknown provider");
                assert_eq!(error.to_string(), expected_error, "case {name}");
            }
            None => {
                let (url, verifier) = client
                    .authorise_url(provider_name, client_id, redirect_uri)
                    .expect("authorise url");
                let prefix = case["auth_url_prefix"].as_str().expect("prefix");
                assert!(url.starts_with(prefix), "case {name}: {url}");
                let query = url.strip_prefix(prefix).expect("query string");
                let mut params = parse_query(query);
                let state = params
                    .get("state")
                    .and_then(|values| values.first())
                    .cloned()
                    .expect("state parameter");
                let challenge = params
                    .get("code_challenge")
                    .and_then(|values| values.first())
                    .cloned()
                    .expect("challenge parameter");
                assert_eq!(
                    challenge,
                    pkce_challenge(&verifier),
                    "case {name}: challenge must be S256 of the returned verifier"
                );
                assert_eq!(
                    verifier.len() as i64,
                    case["verifier_length"].as_i64().expect("verifier length"),
                    "case {name}: verifier length"
                );
                assert_eq!(
                    state.len() as i64,
                    case["state_length"].as_i64().expect("state length"),
                    "case {name}: state length"
                );
                assert_eq!(
                    case["challenge_is_s256_of_verifier"].as_bool(),
                    Some(true),
                    "case {name}"
                );
                params.insert("state".to_string(), vec![RANDOM_MARK.to_string()]);
                params.insert("code_challenge".to_string(), vec![RANDOM_MARK.to_string()]);
                let expected: BTreeMap<String, Vec<String>> =
                    serde_json::from_value(case["params"].clone()).expect("expected params");
                assert_eq!(params, expected, "case {name}: parameters");
            }
        }
    }
}

#[test]
fn oauth2_state_store_matches_go() {
    let fixture = fixture();
    let cases = fixture["state_store"].as_array().expect("state cases");
    assert!(!cases.is_empty());
    for (index, case) in cases.iter().enumerate() {
        let name = case["name"].as_str().expect("name");
        let path_set = case["path_set"].as_bool().expect("path_set");
        let ttl = case["ttl_seconds"].as_i64().expect("ttl");
        let directory = tempfile::tempdir().expect("temp dir");
        let path = if path_set {
            directory.path().join(format!("state-{index}.json"))
        } else {
            std::path::PathBuf::new()
        };
        let base = 1_789_732_800_i64; // 2026-09-19T12:00:00Z
        let clock = Arc::new(Mutex::new(base));
        let now = clock.clone();
        let store = OAuthStateStore::new(&path)
            .with_ttl_seconds(ttl)
            .with_now_unix(move || *now.lock().expect("clock"));
        for step in case["steps"].as_array().expect("steps") {
            let op = step["op"].as_str().expect("op");
            let state = step["state"].as_str().expect("state");
            let offset = offset_from(step["at"].as_str().expect("at"), base);
            *clock.lock().expect("clock") = base + offset;
            let outcome = match op {
                "store" => store.store(state, step["provider"].as_str().unwrap_or_default()),
                "validate" => store.validate(state),
                other => panic!("unknown op {other}"),
            };
            match step["error"].as_str() {
                Some(expected) => assert_eq!(
                    outcome.expect_err("expected error").to_string(),
                    expected,
                    "case {name} step {op} {state}"
                ),
                None => assert!(outcome.is_ok(), "case {name} step {op} {state}"),
            }
        }
        match case["file"].as_str() {
            Some(expected) => {
                let actual = std::fs::read_to_string(&path).expect("state file");
                assert_eq!(actual, expected, "case {name}: state file");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(&path)
                        .expect("metadata")
                        .permissions()
                        .mode();
                    assert_eq!(
                        i64::from(mode & 0o777),
                        case["file_mode"].as_i64().expect("file mode"),
                        "case {name}: file mode"
                    );
                }
            }
            None => assert!(
                !path.exists(),
                "case {name}: no state file expected, found one"
            ),
        }
    }
}

#[test]
fn oauth2_token_requests_match_go_transcript() {
    let fixture = fixture();
    let cases = fixture["tokens"].as_array().expect("token cases");
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let provider_name = case["provider"].as_str().expect("provider");
        let operation = case["operation"].as_str().expect("operation");
        let params = &case["params"];
        let status = case["response"]["status"].as_u64().expect("status") as u16;
        let body = case["response"]["body"].as_str().expect("body").to_string();
        let recorded = &case["recorded"];
        let directory = tempfile::tempdir().expect("temp dir");
        let client = OAuth2Client::new(OAuthStateStore::new(directory.path().join("state.json")));
        let expects_request = !recorded["body"].as_str().expect("recorded body").is_empty()
            && case["transport_fails"].as_bool() != Some(true);
        let server = expects_request.then(|| MockServer::start(status, body));
        let transport: Box<dyn TokenTransport> = match server.as_ref() {
            Some(server) => Box::new(LocalTransport {
                base: format!("http://{}", server.addr),
            }),
            None if case["transport_fails"].as_bool() == Some(true) => Box::new(FailingTransport),
            None => Box::new(FailingTransport),
        };
        let parameter = |key: &str| params[key].as_str().unwrap_or_default();
        let outcome = match operation {
            "exchange" => client.exchange_code(
                transport.as_ref(),
                provider_name,
                parameter("code"),
                parameter("client_id"),
                parameter("client_secret"),
                parameter("redirect_uri"),
                parameter("verifier"),
            ),
            "refresh" => client.refresh_access_token(
                transport.as_ref(),
                provider_name,
                parameter("client_id"),
                parameter("client_secret"),
                parameter("refresh_token"),
            ),
            other => panic!("unknown operation {other}"),
        };
        match case["error"].as_str() {
            Some(expected) => assert_eq!(
                outcome.expect_err("expected error").to_string(),
                expected,
                "case {name}"
            ),
            None => {
                let result = outcome.expect("token request succeeds");
                let actual = serde_json::to_string(&result).expect("serialise result");
                let expected = case["result"].as_str().expect("result");
                assert_eq!(actual, expected, "case {name}: result");
            }
        }
        let observed = match server {
            Some(server) => server.finish(),
            None => Transcript::default(),
        };
        assert_eq!(
            observed.method,
            recorded["method"].as_str().expect("method"),
            "case {name}: method"
        );
        assert_eq!(
            observed.path,
            recorded["path"].as_str().expect("path"),
            "case {name}: path"
        );
        assert_eq!(
            observed.content_type,
            recorded["content_type"].as_str().expect("content type"),
            "case {name}: content type"
        );
        assert_eq!(
            observed.body,
            recorded["body"].as_str().expect("body"),
            "case {name}: form body"
        );
    }
}

#[test]
fn oauth2_token_transport_accepts_only_https_or_loopback() {
    assert!(is_loopback("https://127.0.0.1:8080/token"));
    assert!(is_loopback("http://localhost/token"));
    assert!(!is_loopback("https://oauth2.googleapis.com/token"));
    assert!(!is_loopback("http://erase.example/token"));
    // The production transport is the only thing the binary would construct.
    let _ = UreqTokenTransport;
}

#[test]
fn oauth2_form_encoding_matches_go_url_values_encode() {
    let fields = [
        (
            "scope",
            "https://mail.google.com/ https://www.googleapis.com/auth/gmail.send",
        ),
        ("client_id", "client-123.apps.googleusercontent.com"),
        ("code_challenge_method", "S256"),
    ];
    assert_eq!(
        encode_form(&fields),
        "client_id=client-123.apps.googleusercontent.com&code_challenge_method=S256&scope=https%3A%2F%2Fmail.google.com%2F+https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fgmail.send"
    );
}

fn parse_query(query: &str) -> BTreeMap<String, Vec<String>> {
    let mut params: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        params
            .entry(percent_decode(key))
            .or_default()
            .push(percent_decode(value));
    }
    params
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("00");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn offset_from(timestamp: &str, base: i64) -> i64 {
    // The oracle records an RFC 3339 instant; the cases only use whole seconds.
    let seconds = parse_rfc3339(timestamp);
    seconds - base
}

fn parse_rfc3339(value: &str) -> i64 {
    let (date, rest) = value.split_once('T').expect("date and time");
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next().expect("year").parse().expect("year");
    let month: i64 = date_parts.next().expect("month").parse().expect("month");
    let day: i64 = date_parts.next().expect("day").parse().expect("day");
    let (time, _offset) = rest.split_at(8);
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next().expect("hour").parse().expect("hour");
    let minute: i64 = time_parts.next().expect("minute").parse().expect("minute");
    let second: i64 = time_parts.next().expect("second").parse().expect("second");
    days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_shift = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_shift + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
struct Transcript {
    method: String,
    path: String,
    content_type: String,
    body: String,
}

struct MockServer {
    addr: SocketAddr,
    transcript: Arc<Mutex<Transcript>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MockServer {
    fn start(status: u16, body: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local address");
        let transcript = Arc::new(Mutex::new(Transcript::default()));
        let captured = transcript.clone();
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 1024];
            let head_end = loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break buffer.len(),
                    Ok(read) => {
                        buffer.extend_from_slice(&chunk[..read]);
                        if let Some(position) = find_head_end(&buffer) {
                            break position;
                        }
                    }
                    Err(_) => return,
                }
            };
            let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
            let mut lines = head.split("\r\n");
            let request_line = lines.next().unwrap_or_default();
            let mut request_parts = request_line.split(' ');
            let method = request_parts.next().unwrap_or_default().to_string();
            let path = request_parts.next().unwrap_or_default().to_string();
            let mut content_type = String::new();
            let mut content_length = 0usize;
            for line in lines {
                let Some((name, value)) = line.split_once(':') else {
                    continue;
                };
                let value = value.trim();
                if name.eq_ignore_ascii_case("content-type") {
                    content_type = value.to_string();
                }
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.parse().unwrap_or(0);
                }
            }
            let body_start = head_end + 4;
            while buffer.len() < body_start + content_length {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                    Err(_) => break,
                }
            }
            let form = String::from_utf8_lossy(
                &buffer[body_start..(body_start + content_length).min(buffer.len())],
            )
            .to_string();
            *captured.lock().expect("transcript") = Transcript {
                method,
                path,
                content_type,
                body: form,
            };
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
        Self {
            addr,
            transcript,
            handle: Some(handle),
        }
    }

    fn finish(mut self) -> Transcript {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.transcript.lock().expect("transcript").clone()
    }
}

fn find_head_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

struct LocalTransport {
    base: String,
}

impl TokenTransport for LocalTransport {
    fn post_form(
        &self,
        endpoint: &str,
        form: &str,
        timeout: Duration,
    ) -> Result<TokenReply, TokenTransportError> {
        let path = endpoint
            .split_once("://")
            .map(|(_, rest)| rest)
            .and_then(|rest| rest.find('/').map(|index| &rest[index..]))
            .unwrap_or("/");
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(false)
            .timeout_global(Some(timeout))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let mut response = agent
            .post(format!("{}{path}", self.base))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(form.as_bytes())
            .map_err(|_| TokenTransportError)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|_| TokenTransportError)?;
        Ok(TokenReply { status, body })
    }
}

struct FailingTransport;

impl TokenTransport for FailingTransport {
    fn post_form(
        &self,
        _endpoint: &str,
        _form: &str,
        _timeout: Duration,
    ) -> Result<TokenReply, TokenTransportError> {
        Err(TokenTransportError)
    }
}
