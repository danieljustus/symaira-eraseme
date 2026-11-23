//! Real network SMTP campaign differential; no external mailbox or credentials.

use chrono::{DateTime, FixedOffset};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use symeraseme_core::campaign::{ExecuteOpts, execute_campaign, get_plan};
use symeraseme_core::email::smtp::{EmailMessage, NetSmtpTransport, SmtpConfig, send_message_at};
use symeraseme_core::identity::Profile;
use symeraseme_core::storage::{EventType, Repository, Source, Store};

#[path = "support/capture_smtp.rs"]
mod capture_smtp;
#[path = "support/frozen_smtp.rs"]
mod frozen_smtp;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn capture(mut command: Command, root: &Path, label: &str, budget: Duration) -> Output {
    let out = root.join(format!("{label}.stdout"));
    let err = root.join(format!("{label}.stderr"));
    let mut child = command
        .stdin(Stdio::null())
        .stdout(fs::File::create(&out).unwrap())
        .stderr(fs::File::create(&err).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + budget;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{label} exceeded its owned deadline");
        }
        thread::sleep(Duration::from_millis(20));
    };
    child.wait().unwrap();
    for path in [&out, &err] {
        assert!(fs::metadata(path).unwrap().len() <= 1024 * 1024);
    }
    Output {
        status,
        stdout: fs::read(out).unwrap(),
        stderr: fs::read(err).unwrap(),
    }
}

fn server() -> (u16, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let thread = thread::spawn(move || {
        let mut transcript = Vec::new();
        for _ in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(8);
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "SMTP client did not connect");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("SMTP accept: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut io = BufReader::new(socket);
            io.get_mut()
                .write_all(b"220 synthetic SMTP ready\r\n")
                .unwrap();
            assert_eq!(line(&mut io), "EHLO localhost\r\n");
            transcript.push("EHLO localhost".into());
            io.get_mut()
                .write_all(b"250-localhost\r\n250-8BITMIME\r\n250 SMTPUTF8\r\n")
                .unwrap();
            let mail = line(&mut io);
            assert_eq!(
                mail,
                "MAIL FROM:<sender@example.invalid> BODY=8BITMIME SMTPUTF8\r\n"
            );
            transcript.push(mail);
            io.get_mut().write_all(b"250 sender accepted\r\n").unwrap();
            let rcpt = line(&mut io);
            transcript.push(rcpt.clone());
            if rcpt == "RCPT TO:<failure@example.invalid>\r\n" {
                io.get_mut()
                    .write_all(b"550 synthetic recipient rejected\r\n")
                    .unwrap();
                assert!(line(&mut io).is_empty(), "rejected transaction must close");
                continue;
            }
            assert_eq!(rcpt, "RCPT TO:<success@example.invalid>\r\n");
            io.get_mut()
                .write_all(b"251 recipient forwarded\r\n")
                .unwrap();
            assert_eq!(line(&mut io), "DATA\r\n");
            transcript.push("DATA".into());
            io.get_mut().write_all(b"354 send data\r\n").unwrap();
            loop {
                let data = line(&mut io);
                assert!(!data.is_empty(), "SMTP DATA truncated");
                if data == ".\r\n" {
                    break;
                }
                // The production Go sender uses time.Now for this header.
                // Fold only that volatile value; every other MIME byte matches.
                transcript.push(if data.starts_with("Date: ") {
                    "Date: <CLOCK>\r\n".into()
                } else {
                    data
                });
            }
            io.get_mut().write_all(b"250 message accepted\r\n").unwrap();
            assert_eq!(line(&mut io), "QUIT\r\n");
            transcript.push("QUIT".into());
            io.get_mut().write_all(b"221 closing\r\n").unwrap();
        }
        transcript
    });
    (port, thread)
}

fn line(io: &mut BufReader<TcpStream>) -> String {
    let mut line = String::new();
    let count = Read::by_ref(io).take(65537).read_line(&mut line).unwrap();
    assert!(count <= 65536);
    line
}

fn pin(store: &Store, ids: &[i64]) {
    for id in ids {
        store
            .connection()
            .execute(
                "UPDATE removal_requests SET created_at='2026-02-03 04:05:06' WHERE id=?1",
                [id],
            )
            .unwrap();
        store.connection().execute("UPDATE request_state SET last_event_at='2026-02-04 05:06:07',sent_at='2026-02-05 06:07:08',acknowledged_at='2026-02-06 07:08:09',resolved_at='2026-02-07 08:09:10',deadline_at='2026-02-08 09:10:11',next_action_at='2026-02-09 10:11:12' WHERE request_id=?1",[id]).unwrap();
    }
}

#[test]
fn real_smtp_campaign_bytes_events_and_projections_match_go() {
    let root = tempfile::tempdir().unwrap();
    let frozen = frozen_smtp::observations("campaign");
    let helper = root.path().join(if cfg!(windows) {
        "smtp-go.exe"
    } else {
        "smtp-go"
    });
    let (output, go_transcript) = if let Some(corpus) = &frozen {
        corpus.observation("campaign", &[])
    } else {
        let mut build = Command::new("go");
        build
            .args(["build", "-o"])
            .arg(&helper)
            .arg("./rust-tests/parity/oracle/campaign-smtp")
            .current_dir(repo())
            .env("GOTOOLCHAIN", "go1.26.6")
            .env("GOPROXY", "off")
            .env("GOSUMDB", "off");
        let built = capture(build, root.path(), "go-build", Duration::from_secs(180));
        assert!(
            built.status.success(),
            "Go build: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let go_root = root.path().join("go");
        fs::create_dir(&go_root).unwrap();
        let (port, go_server) = server();
        let mut go = Command::new(&helper);
        go.env_clear()
            .current_dir(&go_root)
            .args([go_root.to_str().unwrap(), &port.to_string()])
            .env("HOME", &go_root)
            .env("USERPROFILE", &go_root)
            .env("TMPDIR", &go_root)
            .env("TEMP", &go_root)
            .env("TMP", &go_root)
            .env("TZ", "UTC")
            .env("SYMERASEME_ORACLE_SOURCE_ROOT", repo());
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(v) = std::env::var_os(key) {
                go.env(key, v);
            }
        }
        for key in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "SYMERASEME_DATA_DIR",
        ] {
            go.env(key, &go_root);
        }
        let output = capture(go, root.path(), "go-campaign", Duration::from_secs(20));
        assert!(
            output.status.success(),
            "Go campaign: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let transcript = go_server.join().unwrap();
        capture_smtp::record(
            "campaign",
            &helper,
            &[capture_smtp::case("campaign", &output, &[], &transcript)],
        );
        (output, transcript)
    };
    let expected: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(expected["schema"], "symeraseme.go-oracle.campaign-smtp.v1");
    assert_eq!(expected["go_version"], "go1.26.6");
    let sources = expected["sources_sha256"].as_object().unwrap();
    assert_eq!(sources.len(), 6);
    for path in [
        "go.mod",
        "internal/campaign/execution.go",
        "internal/email/smtp.go",
        "internal/email/types.go",
        "internal/eventstore/projection.go",
        "rust-tests/parity/oracle/campaign-smtp/main.go",
    ] {
        assert_eq!(
            sources[path],
            hex::encode(Sha256::digest(fs::read(repo().join(path)).unwrap()))
        );
    }
    let (port, rust_server) = server();
    let transport = NetSmtpTransport::new(SmtpConfig {
        host: "127.0.0.1".into(),
        port: i64::from(port),
        from: "sender@example.invalid".into(),
        use_tls: false,
        timeout: Duration::from_secs(3),
        ..SmtpConfig::default()
    });
    let now = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap();
    let store = Store::open(root.path().join("rust.sqlite")).unwrap();
    let repository = Repository::new(&store);
    repository
        .create_campaign("smtp-campaign", "initial", "")
        .unwrap();
    let mut ids = Vec::new();
    for broker in ["failure", "success"] {
        let id = repository
            .create_removal_request(broker, "email", "smtp-campaign", "DE", "", "")
            .unwrap();
        ids.push(id);
        store
            .append_and_project(
                id,
                &EventType::Planned,
                json!({"broker_name":broker,"endpoint":format!("{broker}@example.invalid")})
                    .as_object()
                    .unwrap(),
                &Source::System,
                now.to_utc(),
            )
            .unwrap();
    }
    let profile = Profile {
        full_name: "Oracle Person".into(),
        email_addresses: vec!["oracle@example.invalid".into()],
        ..Profile::default()
    };
    let sender = |to: &str, subject: &str, body: &str| {
        let message = EmailMessage {
            to: to.into(),
            subject: subject.into(),
            body: body.into(),
            cc: String::new(),
            bcc: String::new(),
        };
        send_message_at(
            &message,
            "sender@example.invalid",
            now,
            FixedOffset::east_opt(0).unwrap(),
            "<070707070707070707070707@symeraseme>",
            &transport,
        )
        .map(|id| json!({"message_id":id}).as_object().unwrap().clone())
        .map_err(|e| e.to_string())
    };
    let result = execute_campaign(
        &store,
        "smtp-campaign",
        &ExecuteOpts {
            email_sender: Some(&sender),
            ..ExecuteOpts::default()
        },
        Ok(Some(&profile)),
        5,
        now.to_utc(),
    )
    .unwrap();
    let mut events = Vec::new();
    for id in &ids {
        for event in repository.get_events(*id, 0).unwrap() {
            events.push(json!({"type":event.event_type.as_str(),"request_id":event.request_id,"payload":event.payload,"source":event.source.as_str()}));
        }
    }
    pin(&store, &ids);
    assert_eq!(Value::Object(result), expected["result"]);
    assert_eq!(json!(events), expected["events"]);
    assert_eq!(
        Value::Object(get_plan(&store, "smtp-campaign", "").unwrap()),
        expected["plan"]
    );
    assert_eq!(rust_server.join().unwrap(), go_transcript);
    eprintln!(
        "real SMTP campaign: rejected recipient then success; exact normalized wire, results, events and complete projected plan match real Go"
    );
}
