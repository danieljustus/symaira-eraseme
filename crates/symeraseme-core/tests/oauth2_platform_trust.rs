//! Exercise the production OAuth2 transport with private TLS roots and no live provider.
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
#[cfg(any(target_os = "linux", target_vendor = "apple"))]
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use symeraseme_core::email::oauth2::{TokenTransport, UreqTokenTransport};
use symeraseme_core::email::{ImapConfig, ImapDialer};

#[path = "support/imap_server.rs"]
mod imap_server;

const TEST: &str = "oauth2_transport_uses_configured_platform_roots_and_rejects_bad_chains";
const CHILD: &str = "SYMERASEME_OAUTH_TRUST_CASE";

#[cfg(windows)]
fn inspect_windows_root() {
    use schannel::cert_context::ValidUses;
    use schannel::cert_store::CertStore;
    let expected =
        CertificateDer::from_pem_file(std::env::var("SYMERASEME_OAUTH_TEST_ROOT").unwrap())
            .unwrap();
    assert!(std::env::var_os("SSL_CERT_FILE").is_none());
    assert!(std::env::var_os("SSL_CERT_DIR").is_none());
    let store = CertStore::open_local_machine("ROOT").unwrap();
    let cert = store
        .certs()
        .find(|cert| cert.to_der() == expected.as_ref())
        .expect("the exact owned CA must be present in Windows LocalMachine ROOT");
    eprintln!(
        "native owned CA time_valid={:?} valid_uses={:?}",
        cert.is_time_valid(),
        cert.valid_uses().map(|uses| match uses {
            ValidUses::All => vec!["all".to_owned()],
            ValidUses::Oids(oids) => oids,
        })
    );
    let mut parsed = rustls::RootCertStore::empty();
    eprintln!("native owned CA rustls_parse={:?}", parsed.add(expected));
}

fn check_imap(case: &str) {
    let cert_path = std::env::var("SYMERASEME_OAUTH_TEST_CERT").unwrap();
    let key_path = std::env::var("SYMERASEME_OAUTH_TEST_KEY").unwrap();
    for mode in [
        imap_server::TlsMode::Implicit,
        imap_server::TlsMode::StartTls,
    ] {
        let server = imap_server::ScriptedImapServer::new_tls(
            Arc::new(server_config(Path::new(&cert_path), Path::new(&key_path))),
            mode,
        )
        .unwrap();
        let config = ImapConfig {
            host: "127.0.0.1".into(),
            port: i64::from(server.port),
            username: "testuser".into(),
            password: "testpass".into(),
            use_tls: mode == imap_server::TlsMode::Implicit,
            folder: "INBOX".into(),
            since_days: 0,
            max_messages: 0,
            oauth2: None,
            timeout_seconds: 3,
            allow_insecure_cleartext_auth: false,
        };
        let result = symeraseme_core::email::imap::ImapDialer::new().dial(&config);
        if case == "trusted" {
            result
                .expect("native platform root must permit IMAP authentication")
                .close();
            assert!(
                server
                    .get_transcript()
                    .iter()
                    .any(|line| line.contains("LOGIN"))
            );
        } else {
            assert!(result.is_err(), "{case} IMAP chain must be rejected");
            assert!(
                !server
                    .get_transcript()
                    .iter()
                    .any(|line| line.contains("LOGIN")),
                "credentials must not reach an untrusted IMAP peer"
            );
        }
    }
}

struct InstalledRoot {
    #[cfg(not(target_os = "linux"))]
    name: String,
    #[cfg(target_os = "linux")]
    path: PathBuf,
    #[cfg(target_vendor = "apple")]
    source: PathBuf,
    installed: bool,
}

fn checked(command: &mut Command) -> Result<(), String> {
    checked_with_budget(command, Duration::from_secs(30))
}

fn checked_with_budget(command: &mut Command, budget: Duration) -> Result<(), String> {
    command_output_with_budget(command, budget, Stdio::null()).map(|_| ())
}

fn command_output_with_budget(
    command: &mut Command,
    budget: Duration,
    input: Stdio,
) -> Result<Vec<u8>, String> {
    // A native security tool may leave a helper holding a pipe after its own
    // exit. Capture regular files and bound the actual command's lifetime.
    let logs = tempfile::tempdir().map_err(|error| error.to_string())?;
    let stdout = logs.path().join("stdout");
    let stderr = logs.path().join("stderr");
    let label = format!(
        "{:?} {:?}",
        command.get_program(),
        command.get_args().take(3).collect::<Vec<_>>()
    );
    eprintln!("native CA command start: {label}");
    let mut child = command
        .stdin(input)
        .stdout(std::fs::File::create(&stdout).map_err(|error| error.to_string())?)
        .stderr(std::fs::File::create(&stderr).map_err(|error| error.to_string())?)
        .spawn()
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() >= budget {
            child.kill().map_err(|error| {
                format!(
                    "native CA command exceeded {}ms; terminate failed: {label}: {error}",
                    budget.as_millis()
                )
            })?;
            child.wait().map_err(|error| error.to_string())?;
            return Err(format!(
                "native CA command exceeded {}ms: {label}",
                budget.as_millis()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    child.wait().map_err(|error| error.to_string())?;
    eprintln!(
        "native CA command finished: {label}, status={status}, elapsed_ms={}",
        started.elapsed().as_millis()
    );
    if status.success() {
        if std::fs::metadata(&stdout)
            .map_err(|error| error.to_string())?
            .len()
            > 64 * 1024
        {
            return Err("native CA command exceeded its output limit".into());
        }
        std::fs::read(&stdout).map_err(|error| error.to_string())
    } else {
        let size = std::fs::metadata(&stderr)
            .map_err(|error| error.to_string())?
            .len();
        if size > 64 * 1024 {
            return Err("native CA command exceeded its diagnostic limit".into());
        }
        let error = std::fs::read(&stderr).map_err(|error| error.to_string())?;
        Err(format!(
            "native CA command failed: {}",
            String::from_utf8_lossy(&error)
        ))
    }
}

#[test]
fn native_command_control_bounds_children_and_reports_real_exit_failure() {
    const MODE: &str = "SYMERASEME_CA_COMMAND_CONTROL";
    if let Ok(mode) = std::env::var(MODE) {
        match mode.as_str() {
            "success" => std::process::exit(0),
            "failure" => {
                eprintln!("synthetic native command failure");
                std::process::exit(23);
            }
            "stall" => {
                thread::sleep(Duration::from_secs(10));
                std::process::exit(0);
            }
            _ => panic!("unexpected native command control"),
        }
    }
    let command = |mode: &str| {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "native_command_control_bounds_children_and_reports_real_exit_failure",
                "--nocapture",
            ])
            .env(MODE, mode);
        command
    };
    checked(&mut command("success")).unwrap();
    assert!(
        checked(&mut command("failure"))
            .unwrap_err()
            .contains("synthetic native command failure")
    );
    let started = Instant::now();
    assert!(
        checked_with_budget(&mut command("stall"), Duration::from_millis(100))
            .unwrap_err()
            .contains("exceeded 100ms")
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "stalled command must be terminated and reaped"
    );
}

impl InstalledRoot {
    fn install(source: &Path, name: String) -> Self {
        let mut root = Self {
            #[cfg(not(target_os = "linux"))]
            name: name.clone(),
            #[cfg(target_os = "linux")]
            path: PathBuf::from(format!("/usr/local/share/ca-certificates/{name}.crt")),
            #[cfg(target_vendor = "apple")]
            source: source.to_path_buf(),
            installed: false,
        };
        #[cfg(target_os = "linux")]
        {
            checked(
                Command::new("sudo")
                    .args(["-n", "install", "-m", "644"])
                    .arg(source)
                    .arg(&root.path),
            )
            .unwrap();
            root.installed = true;
            checked(Command::new("sudo").args(["-n", "update-ca-certificates"])).unwrap();
        }
        #[cfg(target_vendor = "apple")]
        {
            checked(
                Command::new("sudo")
                    .args([
                        "-n",
                        "security",
                        "add-trusted-cert",
                        "-d",
                        "-r",
                        "trustRoot",
                        "-k",
                        "/Library/Keychains/System.keychain",
                    ])
                    .arg(source),
            )
            .unwrap();
            root.installed = true;
        }
        #[cfg(windows)]
        {
            checked(
                Command::new("certutil")
                    .args(["-addstore", "Root"])
                    .arg(source),
            )
            .unwrap();
            root.installed = true;
        }
        root
    }

    fn remove(&mut self) -> Result<(), String> {
        if !self.installed {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            checked(
                Command::new("sudo")
                    .args(["-n", "rm", "--"])
                    .arg(&self.path),
            )?;
            checked(Command::new("sudo").args(["-n", "update-ca-certificates"]))?;
        }
        #[cfg(target_vendor = "apple")]
        {
            // Both login/root remove-trusted-cert stall on hosted macOS.
            // Edit only this certificate's entry through the external trust
            // representation API, then prove every unrelated entry survived.
            let files = tempfile::tempdir().map_err(|error| error.to_string())?;
            let before = files.path().join("before.plist");
            let removed = files.path().join("removed.plist");
            let after = files.path().join("after.plist");
            checked(
                Command::new("security")
                    .args(["trust-settings-export", "-d"])
                    .arg(&before),
            )?;
            checked(Command::new("python3").args([
                "-c",
                "import hashlib,plistlib,ssl,sys; source,before,out=sys.argv[1:]; digest=hashlib.sha1(ssl.PEM_cert_to_DER_cert(open(source).read())).hexdigest().upper(); data=plistlib.load(open(before,'rb')); entries=data['trustList']; owned=[k for k in entries if k.upper()==digest]; assert len(owned)==1, 'owned CI CA trust entry missing or ambiguous'; del entries[owned[0]]; plistlib.dump(data,open(out,'wb')); print('removed exactly one owned CI CA trust entry')",
            ]).arg(&self.source).arg(&before).arg(&removed))?;
            checked(
                Command::new("sudo")
                    .args(["-n", "security", "trust-settings-import", "-d"])
                    .arg(&removed),
            )?;
            checked(
                Command::new("security")
                    .args(["trust-settings-export", "-d"])
                    .arg(&after),
            )?;
            checked(Command::new("python3").args([
                "-c",
                "import plistlib,sys; expected,actual=[plistlib.load(open(p,'rb')) for p in sys.argv[1:]]; assert expected==actual, 'CI CA trust removal changed unrelated settings'; print('owned CA trust entry absent; unrelated trust settings unchanged')",
            ]).arg(&removed).arg(&after))?;
            checked(Command::new("sudo").args([
                "-n",
                "security",
                "delete-certificate",
                "-c",
                &self.name,
                "/Library/Keychains/System.keychain",
            ]))?;
        }
        #[cfg(windows)]
        checked(Command::new("certutil").args(["-delstore", "Root", &self.name]))?;
        self.installed = false;
        Ok(())
    }
}

impl Drop for InstalledRoot {
    fn drop(&mut self) {
        if let Err(error) = self.remove() {
            eprintln!("owned CI CA cleanup failed: {error}");
        }
    }
}

fn serve(cert: &Path, key: &Path) -> (String, thread::JoinHandle<Vec<u8>>) {
    let config = server_config(cert, key);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("https://{}/token", listener.local_addr().unwrap());
    let handle = thread::spawn(move || serve_connection(listener, config));
    (endpoint, handle)
}

fn server_config(cert: &Path, key: &Path) -> ServerConfig {
    let cert = CertificateDer::from_pem_file(cert).unwrap();
    let key = PrivateKeyDer::from_pem_file(key).unwrap();
    ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap()
}

fn serve_connection(listener: TcpListener, config: ServerConfig) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let socket = loop {
        match listener.accept() {
            Ok((socket, _)) => break socket,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "TLS client did not connect");
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("TLS accept: {error}"),
        }
    };
    // Windows accepted sockets inherit the listener's nonblocking mode.
    // Use bounded blocking TLS I/O after the separately bounded accept loop.
    socket.set_nonblocking(false).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let connection = ServerConnection::new(Arc::new(config)).unwrap();
    let mut stream = StreamOwned::new(connection, socket);
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) => return request,
            Err(error) => {
                eprintln!("synthetic TLS peer handshake/read error: {error}");
                return request;
            }
            Ok(_) => request.push(byte[0]),
        }
        assert!(request.len() <= 64 * 1024, "bounded request headers");
    }
    let headers = std::str::from_utf8(&request).unwrap();
    let length: usize = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .unwrap()
        .1
        .trim()
        .parse()
        .unwrap();
    assert!(length <= 1024, "bounded synthetic form");
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    request.extend_from_slice(&body);
    let body = br#"{"access_token":"synthetic","token_type":"Bearer"}"#;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
    request
}

#[test]
fn oauth2_transport_uses_configured_platform_roots_and_rejects_bad_chains() {
    if let Ok(case) = std::env::var(CHILD) {
        #[cfg(windows)]
        if std::env::var_os("SYMERASEME_NATIVE_TRUST_CHILD").is_some() && case == "trusted" {
            inspect_windows_root();
        }
        let (endpoint, server) = serve(
            Path::new(&std::env::var("SYMERASEME_OAUTH_TEST_CERT").unwrap()),
            Path::new(&std::env::var("SYMERASEME_OAUTH_TEST_KEY").unwrap()),
        );
        let result =
            UreqTokenTransport.post_form(&endpoint, "code=synthetic", Duration::from_secs(3));
        let request = server.join().unwrap();
        if case == "trusted" {
            let reply = result.expect("the configured platform CA must be trusted by OAuth2");
            assert_eq!(reply.status, 200);
            assert_eq!(
                reply.body,
                r#"{"access_token":"synthetic","token_type":"Bearer"}"#
            );
            assert!(request.starts_with(b"POST /token HTTP/1.1\r\n"));
            assert!(request.ends_with(b"code=synthetic"));
        } else {
            assert!(result.is_err(), "{case} TLS chain must be rejected");
            assert!(
                request.is_empty(),
                "OAuth2 form must not reach an untrusted peer"
            );
        }
        if std::env::var_os("SYMERASEME_NATIVE_TRUST_CHILD").is_some() {
            check_imap(&case);
            eprintln!("native trust {case}: OAuth2 HTTPS, IMAP TLS and STARTTLS checked");
        }
        return;
    }
    exercise_certificates(false);
}

#[test]
#[ignore = "changes trust only on disposable GitHub-hosted native CI runners"]
fn native_platform_roots_accept_ca_and_reject_foreign_expired_chains() {
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    exercise_certificates(true);
}

fn exercise_certificates(native: bool) {
    let directory = tempfile::tempdir().unwrap();
    let name = format!(
        "symeraseme-ci-{}",
        directory.path().file_name().unwrap().to_str().unwrap()
    );
    let root_key = KeyPair::generate().unwrap();
    let mut root_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    root_params
        .distinguished_name
        .push(DnType::CommonName, &name);
    let root = root_params.self_signed(&root_key).unwrap();
    let issuer = Issuer::from_params(&root_params, &root_key);
    let unrelated_key = KeyPair::generate().unwrap();
    let unrelated = root_params.self_signed(&unrelated_key).unwrap();
    let roots = directory.path().join("roots.pem");
    let unrelated_roots = directory.path().join("unrelated.pem");
    std::fs::write(&roots, root.pem()).unwrap();
    std::fs::write(&unrelated_roots, unrelated.pem()).unwrap();
    let mut installation = native.then(|| InstalledRoot::install(&roots, name));

    let mut outcomes = Vec::new();
    for case in ["trusted", "untrusted", "expired"] {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec!["127.0.0.1".into()]).unwrap();
        if case == "expired" {
            params.not_before = rcgen::date_time_ymd(1990, 1, 1);
            params.not_after = rcgen::date_time_ymd(1991, 1, 1);
        }
        let untrusted_issuer = Issuer::from_params(&root_params, &unrelated_key);
        let cert = params
            .signed_by(
                &key,
                if native && case == "untrusted" {
                    &untrusted_issuer
                } else {
                    &issuer
                },
            )
            .unwrap();
        let cert_path = directory.path().join(format!("{case}.pem"));
        let key_path = directory.path().join(format!("{case}-key.pem"));
        std::fs::write(&cert_path, cert.pem()).unwrap();
        std::fs::write(&key_path, key.serialize_pem()).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", TEST, "--nocapture"])
            .env(CHILD, case)
            .env("SYMERASEME_OAUTH_TEST_CERT", cert_path)
            .env("SYMERASEME_OAUTH_TEST_KEY", key_path);
        if native {
            command
                .env_remove("SSL_CERT_FILE")
                .env_remove("SSL_CERT_DIR")
                .env("SYMERASEME_NATIVE_TRUST_CHILD", "1")
                .env("SYMERASEME_OAUTH_TEST_ROOT", &roots);
        } else {
            command
                .env(
                    "SSL_CERT_FILE",
                    if case == "untrusted" {
                        &unrelated_roots
                    } else {
                        &roots
                    },
                )
                .env("SSL_CERT_DIR", "");
        }
        let output = command.output().unwrap();
        if native {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }
        outcomes.push((case, output));
    }
    if let Some(installation) = &mut installation {
        installation.remove().unwrap();
    }
    for (case, output) in outcomes {
        assert!(
            output.status.success(),
            "{case}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
