//! The HTTP MCP server must not outlive the process that supervises it (#1322).
//!
//! A short-lived `sh` plays the macOS app: it starts the real server as its
//! child and, when opted in, names itself in `SYMERASEME_MCP_PARENT_PID`. The
//! test kills the `sh` without letting it clean up (as a crash or SIGKILL
//! would) and observes only its own processes by PID: no global `pgrep`.
#![cfg(unix)]

use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SUPERVISOR: &str = r#"
if [ "$3" = 1 ]; then export SYMERASEME_MCP_PARENT_PID=$$; fi
"$0" mcp --host 127.0.0.1 --port "$1" &
echo $! > "$2.tmp"
mv "$2.tmp" "$2"
wait
"#;

const READY_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_TIMEOUT: Duration = Duration::from_secs(6);

struct Fixture {
    _root: tempfile::TempDir,
    port: u16,
    supervisor: Child,
    server_pid: u32,
}

impl Fixture {
    fn start(opt_in: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let port = unused_port();
        let pid_file = root.path().join("server.pid");
        let stderr = std::fs::File::create(root.path().join("stderr.log")).unwrap();
        let supervisor = Command::new("/bin/sh")
            .arg("-c")
            .arg(SUPERVISOR)
            .arg(env!("CARGO_BIN_EXE_symeraseme-rust"))
            .arg(port.to_string())
            .arg(&pid_file)
            .arg(if opt_in { "1" } else { "0" })
            .env("HOME", &home)
            .env("SYMERASEME_DATA_DIR", root.path().join("data"))
            .env_remove("SYMERASEME_MCP_PARENT_PID")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap();
        let mut fixture = Self {
            _root: root,
            port,
            supervisor,
            server_pid: 0,
        };
        fixture.server_pid = fixture.wait_ready(&pid_file);
        fixture
    }

    fn wait_ready(&mut self, pid_file: &Path) -> u32 {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            let pid = std::fs::read_to_string(pid_file)
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok());
            if let Some(pid) = pid
                && listening(self.port)
            {
                return pid;
            }
            assert!(
                self.supervisor.try_wait().unwrap().is_none(),
                "supervisor exited before the server was ready"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("server was not listening on port {} in time", self.port);
    }

    /// Simulate the app dying abruptly: no signal handler or cleanup can run.
    fn kill_supervisor(&mut self) {
        self.supervisor.kill().unwrap();
        self.supervisor.wait().unwrap();
    }

    fn wait_for_server_exit(&self, timeout: Duration) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if !alive(self.server_pid) {
                return Some(started.elapsed());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only processes this fixture started.
        if self.server_pid != 0 {
            let _ = Command::new("kill")
                .args(["-KILL", &self.server_pid.to_string()])
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.supervisor.kill();
        let _ = self.supervisor.wait();
    }
}

fn unused_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &(Ipv4Addr::LOCALHOST, port).into(),
        Duration::from_millis(200),
    )
    .is_ok()
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn wait_until_port_is_free(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !listening(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

#[test]
fn server_exits_and_frees_its_port_when_the_opted_in_parent_dies() {
    let mut fixture = Fixture::start(true);
    assert!(alive(fixture.server_pid));

    fixture.kill_supervisor();

    let elapsed = fixture
        .wait_for_server_exit(EXIT_TIMEOUT)
        .expect("server must exit after its supervising parent was killed");
    assert!(elapsed < EXIT_TIMEOUT, "exited after {elapsed:?}");
    assert!(
        wait_until_port_is_free(fixture.port, Duration::from_secs(2)),
        "port {} must be free after the server exits",
        fixture.port
    );
}

#[test]
fn server_without_the_opt_in_keeps_running_when_its_parent_dies() {
    let mut fixture = Fixture::start(false);

    fixture.kill_supervisor();

    // Longer than the watcher's poll interval plus its graceful drain, so an
    // accidental default-on watcher would already have stopped the server.
    assert!(
        fixture
            .wait_for_server_exit(Duration::from_secs(4))
            .is_none(),
        "plain CLI behaviour must be unchanged: the server keeps running"
    );
    assert!(listening(fixture.port));

    // Normal termination is still graceful: SIGTERM stops it and frees the port.
    let status = Command::new("kill")
        .args(["-TERM", &fixture.server_pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        fixture.wait_for_server_exit(EXIT_TIMEOUT).is_some(),
        "SIGTERM still stops the server"
    );
    assert!(wait_until_port_is_free(
        fixture.port,
        Duration::from_secs(2)
    ));
}
