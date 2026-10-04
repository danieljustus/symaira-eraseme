//! Source-bound SMTP protocol controls against the actual Go network sender.

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use symeraseme_core::email::OAuth2Token;
use symeraseme_core::email::smtp::{NetSmtpTransport, SmtpConfig, SmtpTransport};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn child(mut command: Command, root: &Path, input: Option<&Path>, budget: Duration) -> Vec<u8> {
    let out = root.join("child.stdout");
    let err = root.join("child.stderr");
    let input = match input {
        Some(path) => Stdio::from(fs::File::open(path).unwrap()),
        None => Stdio::null(),
    };
    let mut child = command
        .stdin(input)
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
            panic!("owned SMTP oracle child exceeded deadline");
        }
        thread::sleep(Duration::from_millis(20));
    };
    child.wait().unwrap();
    for path in [&out, &err] {
        assert!(fs::metadata(path).unwrap().len() <= 65536);
    }
    assert!(
        status.success(),
        "SMTP oracle: {}",
        String::from_utf8_lossy(&fs::read(err).unwrap())
    );
    fs::read(out).unwrap()
}
fn line<S: Read>(io: &mut BufReader<S>) -> String {
    let mut line = String::new();
    let n = Read::by_ref(io).take(65537).read_line(&mut line).unwrap();
    assert!(n <= 65536);
    line
}

#[test]
fn starttls_authenticates_only_after_verified_chain() {
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{ClientConfig, RootCertStore, ServerConfig, ServerConnection, StreamOwned};
    use std::sync::Arc;

    let root_key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "private SMTP test CA");
    let root = params.self_signed(&root_key).unwrap();
    let issuer = Issuer::from_params(&params, &root_key);
    let foreign_key = KeyPair::generate().unwrap();
    let foreign = Issuer::from_params(&params, &foreign_key);
    for case in ["trusted", "untrusted", "expired"] {
        let key = KeyPair::generate().unwrap();
        let mut leaf = CertificateParams::new(vec!["127.0.0.1".into()]).unwrap();
        if case == "expired" {
            leaf.not_before = rcgen::date_time_ymd(1990, 1, 1);
            leaf.not_after = rcgen::date_time_ymd(1991, 1, 1);
        }
        let cert = leaf
            .signed_by(
                &key,
                if case == "untrusted" {
                    &foreign
                } else {
                    &issuer
                },
            )
            .unwrap();
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(cert.der().to_vec())],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
                )
                .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let peer = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let socket = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("TLS SMTP accept: {e}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut plain = BufReader::new(socket);
            plain.get_mut().write_all(b"220 ready\r\n").unwrap();
            assert_eq!(line(&mut plain), "EHLO localhost\r\n");
            plain
                .get_mut()
                .write_all(b"250-localhost\r\n250 STARTTLS\r\n")
                .unwrap();
            assert_eq!(line(&mut plain), "STARTTLS\r\n");
            plain.get_mut().write_all(b"220 begin TLS\r\n").unwrap();
            assert!(plain.buffer().is_empty());
            let tls = StreamOwned::new(
                ServerConnection::new(Arc::new(config)).unwrap(),
                plain.into_inner(),
            );
            let mut tls = BufReader::new(tls);
            let mut first = String::new();
            let read = Read::by_ref(&mut tls).take(65537).read_line(&mut first);
            if case != "trusted" {
                assert!(
                    read.is_err() || first.is_empty(),
                    "untrusted SMTP peer received application bytes"
                );
                return false;
            }
            read.unwrap();
            assert_eq!(first, "EHLO localhost\r\n");
            tls.get_mut()
                .write_all(b"250-localhost\r\n250 AUTH XOAUTH2\r\n")
                .unwrap();
            assert_eq!(
                line(&mut tls),
                format!(
                    "AUTH XOAUTH2 {}\r\n",
                    STANDARD.encode("user=synthetic-user\x01auth=Bearer synthetic-token\x01\x01")
                )
            );
            tls.get_mut().write_all(b"235 authenticated\r\n").unwrap();
            assert_eq!(line(&mut tls), "MAIL FROM:<sender@example.invalid>\r\n");
            tls.get_mut().write_all(b"250 accepted\r\n").unwrap();
            assert_eq!(line(&mut tls), "RCPT TO:<to@example.invalid>\r\n");
            tls.get_mut().write_all(b"250 accepted\r\n").unwrap();
            assert_eq!(line(&mut tls), "DATA\r\n");
            tls.get_mut().write_all(b"354 data\r\n").unwrap();
            assert_eq!(line(&mut tls), "synthetic body\r\n");
            assert_eq!(line(&mut tls), ".\r\n");
            tls.get_mut().write_all(b"250 stored\r\n").unwrap();
            assert_eq!(line(&mut tls), "QUIT\r\n");
            tls.get_mut().write_all(b"221 closing\r\n").unwrap();
            true
        });
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(root.der().to_vec()))
            .unwrap();
        let tls =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth();
        let cfg = SmtpConfig {
            host: "127.0.0.1".into(),
            port: i64::from(port),
            from: "sender@example.invalid".into(),
            timeout: Duration::from_secs(2),
            oauth2: Some(OAuth2Token {
                username: "synthetic-user".into(),
                access_token: "synthetic-token".into(),
            }),
            ..SmtpConfig::default()
        };
        let result = NetSmtpTransport::new(cfg)
            .with_tls_config(Arc::new(tls))
            .send(Some(&["to@example.invalid".into()]), b"synthetic body\n");
        assert_eq!(result.is_ok(), case == "trusted", "{case}");
        if let Err(error) = result {
            assert!(!error.contains("synthetic-token"));
        }
        assert_eq!(peer.join().unwrap(), case == "trusted");
        eprintln!("real Rust STARTTLS {case}: authenticated only after verified trusted chain");
    }
}
fn server(case: &'static str) -> (u16, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let socket = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline);
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("SMTP accept: {e}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut io = BufReader::new(socket);
        let mut seen = Vec::new();
        if case == "greeting-rejected" {
            io.get_mut().write_all(b"421 unavailable\r\n").unwrap();
            assert!(line(&mut io).is_empty());
            return seen;
        }
        io.get_mut().write_all(b"220 localhost ready\r\n").unwrap();
        let greeting = line(&mut io);
        assert_eq!(greeting, "EHLO localhost\r\n");
        seen.push(greeting);
        if case == "helo-fallback" {
            io.get_mut().write_all(b"502 EHLO unsupported\r\n").unwrap();
            let helo = line(&mut io);
            assert_eq!(helo, "HELO localhost\r\n");
            seen.push(helo);
            io.get_mut().write_all(b"250 localhost\r\n").unwrap();
        } else {
            io.get_mut()
                .write_all(b"250-localhost\r\n250-AUTH PLAIN XOAUTH2\r\n250 8BITMIME\r\n")
                .unwrap();
        }
        if case == "missing-starttls" {
            assert!(line(&mut io).is_empty());
            return seen;
        }
        if matches!(
            case,
            "plain" | "oauth" | "auth-rejected" | "auth-challenge" | "credential-echo"
        ) {
            let auth = line(&mut io);
            let expected = if case == "oauth" {
                format!(
                    "AUTH XOAUTH2 {}\r\n",
                    STANDARD.encode("user=synthetic-user\x01auth=Bearer synthetic-token\x01\x01")
                )
            } else {
                format!(
                    "AUTH PLAIN {}\r\n",
                    STANDARD.encode(if case == "credential-echo" {
                        "\0synthetic-user\0\tsynthetic-password"
                    } else {
                        "\0synthetic-user\0synthetic-password"
                    })
                )
            };
            assert_eq!(auth, expected);
            seen.push(auth);
            if matches!(case, "auth-rejected" | "auth-challenge" | "credential-echo") {
                if case == "credential-echo" {
                    let echo = format!(
                        "535 \tsynthetic-password {}\r\n",
                        STANDARD.encode(if case == "credential-echo" {
                            "\0synthetic-user\0\tsynthetic-password"
                        } else {
                            "\0synthetic-user\0synthetic-password"
                        })
                    );
                    io.get_mut().write_all(echo.as_bytes()).unwrap();
                } else {
                    io.get_mut()
                        .write_all(if case == "auth-rejected" {
                            b"535 authentication rejected\r\n"
                        } else {
                            b"334 Y2hhbGxlbmdl\r\n"
                        })
                        .unwrap();
                }
                let abort = line(&mut io);
                assert_eq!(abort, "*\r\n");
                seen.push(abort);
                io.get_mut().write_all(b"501 aborted\r\n").unwrap();
                let quit = line(&mut io);
                assert_eq!(quit, "QUIT\r\n");
                seen.push(quit);
                io.get_mut().write_all(b"221 closing\r\n").unwrap();
                return seen;
            }
            io.get_mut().write_all(b"235 authenticated\r\n").unwrap();
        }
        let mail = line(&mut io);
        assert_eq!(
            mail,
            if case == "helo-fallback" {
                "MAIL FROM:<sender@example.invalid>\r\n"
            } else {
                "MAIL FROM:<sender@example.invalid> BODY=8BITMIME\r\n"
            }
        );
        seen.push(mail);
        io.get_mut().write_all(b"250 sender accepted\r\n").unwrap();
        for recipient in [
            "to@example.invalid",
            "cc@example.invalid",
            "bcc@example.invalid",
        ] {
            let rcpt = line(&mut io);
            assert_eq!(rcpt, format!("RCPT TO:<{recipient}>\r\n"));
            seen.push(rcpt);
            io.get_mut()
                .write_all(b"250 recipient accepted\r\n")
                .unwrap();
        }
        let data = line(&mut io);
        assert_eq!(data, "DATA\r\n");
        seen.push(data);
        io.get_mut().write_all(b"354 send data\r\n").unwrap();
        loop {
            let data = line(&mut io);
            assert!(!data.is_empty());
            seen.push(data.clone());
            if data == ".\r\n" {
                break;
            }
        }
        if case == "data-rejected" {
            io.get_mut()
                .write_all(b"451 temporary data failure\r\n")
                .unwrap();
            assert!(line(&mut io).is_empty());
            return seen;
        }
        io.get_mut().write_all(b"250 stored\r\n").unwrap();
        let quit = line(&mut io);
        assert_eq!(quit, "QUIT\r\n");
        seen.push(quit);
        io.get_mut().write_all(b"221 closing\r\n").unwrap();
        seen
    });
    (port, thread)
}

#[test]
fn live_go_smtp_envelope_auth_dot_framing_and_failures_match() {
    let root = tempfile::tempdir().unwrap();
    let helper = root.path().join(if cfg!(windows) {
        "smtp-go.exe"
    } else {
        "smtp-go"
    });
    let mut build = Command::new("go");
    build
        .args(["build", "-o"])
        .arg(&helper)
        .arg("./rust-tests/parity/oracle/campaign-smtp")
        .current_dir(repo())
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off");
    child(build, root.path(), None, Duration::from_secs(180));
    let recipients = vec![
        "to@example.invalid".into(),
        "cc@example.invalid".into(),
        "bcc@example.invalid".into(),
    ];
    for case in [
        "plain",
        "oauth",
        "auth-rejected",
        "auth-challenge",
        "credential-echo",
        "missing-starttls",
        "data-rejected",
        "greeting-rejected",
        "helo-fallback",
    ] {
        let (go_port, go_server) = server(case);
        let auth = matches!(
            case,
            "plain" | "auth-rejected" | "auth-challenge" | "credential-echo"
        );
        let oauth = case == "oauth";
        let raw=b"From: sender@example.invalid\r\nTo: to@example.invalid\r\n\r\n.first\n..second\r\nlast\r";
        let request = json!({"Config":{"Host":"127.0.0.1","Port":go_port,"From":"sender@example.invalid","UseTLS":case=="missing-starttls","Username":if auth{"synthetic-user"}else{""},"Password":if auth{if case == "credential-echo" {"\tsynthetic-password"} else {"synthetic-password"}}else{""},"OAuth2":if oauth{json!({"Username":"synthetic-user","AccessToken":"synthetic-token"})}else{Value::Null},"Timeout":2_000_000_000u64},"Recipients":recipients,"Raw":STANDARD.encode(raw)});
        let input = root.path().join("input.json");
        fs::write(&input, serde_json::to_vec(&request).unwrap()).unwrap();
        let mut command = Command::new(&helper);
        command
            .arg("--transport")
            .env_clear()
            .env("HOME", root.path())
            .env("USERPROFILE", root.path())
            .env("TEMP", root.path())
            .env("TMP", root.path());
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let go: Value = serde_json::from_slice(&child(
            command,
            root.path(),
            Some(&input),
            Duration::from_secs(10),
        ))
        .unwrap();
        let (rust_port, rust_server) = server(case);
        let config = SmtpConfig {
            host: "127.0.0.1".into(),
            port: i64::from(rust_port),
            from: "sender@example.invalid".into(),
            use_tls: case == "missing-starttls",
            username: if auth {
                "synthetic-user".into()
            } else {
                String::new()
            },
            password: if auth {
                if case == "credential-echo" {
                    "\tsynthetic-password".into()
                } else {
                    "synthetic-password".into()
                }
            } else {
                String::new()
            },
            oauth2: oauth.then(|| OAuth2Token {
                username: "synthetic-user".into(),
                access_token: "synthetic-token".into(),
            }),
            timeout: Duration::from_secs(2),
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("synthetic-password"));
        assert!(!debug.contains("synthetic-token"));
        let rust = NetSmtpTransport::new(config).send(Some(&recipients), raw);
        if case == "credential-echo" {
            let error = rust.unwrap_err();
            let encoded = STANDARD.encode(if case == "credential-echo" {
                "\0synthetic-user\0\tsynthetic-password"
            } else {
                "\0synthetic-user\0synthetic-password"
            });
            assert!(
                go["error"].as_str().unwrap().contains("synthetic-password"),
                "Go negative control must observe the actual echo"
            );
            assert!(go["error"].as_str().unwrap().contains(&encoded));
            assert!(!error.contains("synthetic-password"));
            assert!(!error.contains(&encoded));
            assert!(error.contains("[REDACTED]"));
        } else {
            assert_eq!(
                json!({"ok":rust.is_ok(),"error":rust.err().unwrap_or_default()}),
                go,
                "{case}"
            );
        }
        assert_eq!(
            rust_server.join().unwrap(),
            go_server.join().unwrap(),
            "{case}"
        );
        if case == "credential-echo" {
            eprintln!(
                "real SMTP credential-echo: matching authentication/abort wire; Rust removes the raw and encoded synthetic credentials observed in Go's error"
            );
        } else {
            eprintln!("real Go/Rust SMTP case {case}: exact transaction, result and error matched");
        }
    }
}
