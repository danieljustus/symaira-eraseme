use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Child, Command, Stdio};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use symeraseme_core::config::{
    Config, ConfigContext, ConfigError, Storage, default_encrypted_temp_dir, defaults, load,
    resolve_storage,
};

const GO_FIXTURE: &str = include_str!("../../../rust-tests/parity/oracle/config/config_cases.json");

struct TestTree {
    root: PathBuf,
}

impl TestTree {
    fn new(case: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("symeraseme-config-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test tree");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn context(&self, environment: BTreeMap<String, String>) -> ConfigContext {
        fs::create_dir_all(self.home()).expect("create home");
        fs::create_dir_all(self.project()).expect("create project");
        ConfigContext::new(self.home(), self.project(), environment)
    }

    fn write(&self, path: impl AsRef<Path>, contents: &str) {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, contents).expect("write fixture");
    }
}

impl Drop for TestTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture(case: &str) -> Value {
    let document: Value = serde_json::from_str(GO_FIXTURE).expect("valid Go config fixture");
    document.get(case).cloned().expect("fixture case")
}

#[cfg(unix)]
#[derive(Debug)]
struct OracleCommandOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(unix)]
const ORACLE_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(unix)]
const ORACLE_MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
#[cfg(unix)]
const ORACLE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(unix)]
fn run_go_config_oracle() -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let temp_root = std::env::temp_dir().join(format!(
        "symeraseme-config-oracle-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));
    fs::create_dir(&temp_root).expect("create isolated oracle build directory");
    let _cleanup = TempRootGuard(temp_root.clone());
    let executable = temp_root.join(if cfg!(windows) {
        "config-oracle.exe"
    } else {
        "config-oracle"
    });

    let mut build = Command::new("go");
    build
        .args(["build", "-o"])
        .arg(&executable)
        .arg("./rust-tests/parity/oracle/config")
        .current_dir(&root)
        .env("GOWORK", "off");
    let build_output = run_file_backed(
        &mut build,
        &temp_root.join("build.stdout"),
        &temp_root.join("build.stderr"),
        ORACLE_TIMEOUT,
    )
    .expect("Go must be available for the committed config oracle");
    assert!(
        build_output.status.success(),
        "Go config oracle build failed"
    );

    let mut oracle = Command::new(&executable);
    oracle.current_dir(&root).env_clear();
    let output = run_file_backed(
        &mut oracle,
        &temp_root.join("oracle.stdout"),
        &temp_root.join("oracle.stderr"),
        ORACLE_TIMEOUT,
    )
    .expect("Go config oracle execution must complete within its bounded timeout");
    assert!(
        output.status.success(),
        "Go config oracle exited unsuccessfully"
    );
    serde_json::from_slice(&output.stdout).expect("Go config oracle must emit valid JSON")
}

#[cfg(unix)]
struct TempRootGuard(PathBuf);

#[cfg(unix)]
impl Drop for TempRootGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn bounded_reap(child: &mut Child, timeout: Duration) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(_) => return Ok(()),
            None if Instant::now() >= deadline => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "oracle subprocess cleanup exceeded its bounded timeout",
                ));
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    }
}

#[cfg(unix)]
fn append_error(slot: &mut Option<String>, label: &str, error: std::io::Error) {
    if slot.is_none() {
        *slot = Some(format!("{label}: {error}"));
    }
}

#[cfg(unix)]
fn cleanup_error(errors: &[Option<String>], timed_out: bool) -> Option<std::io::Error> {
    let messages: Vec<&str> = errors.iter().filter_map(Option::as_deref).collect();
    if messages.is_empty() && !timed_out {
        return None;
    }
    let message = if timed_out {
        format!(
            "oracle subprocess cleanup exceeded its bounded timeout{}",
            if messages.is_empty() {
                String::new()
            } else {
                format!("; {}", messages.join("; "))
            }
        )
    } else {
        messages.join("; ")
    };
    Some(std::io::Error::new(
        if timed_out {
            std::io::ErrorKind::TimedOut
        } else {
            std::io::ErrorKind::Other
        },
        message,
    ))
}

#[cfg(unix)]
fn benign_kill_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
    ) || error.raw_os_error() == Some(3)
}

#[cfg(unix)]
struct OracleGroupKeeper {
    child: Child,
    oracle_child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
    pgid: i32,
    oracle_reaped: bool,
    keeper_reaped: bool,
    keeper_wait_uncertain: bool,
    cleanup_finished: bool,
    cleanup_deadline: Option<Instant>,
}

#[cfg(unix)]
impl OracleGroupKeeper {
    fn spawn() -> std::io::Result<Self> {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "read _"])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn()?;
        let pid = child.id();
        let pgid = match i32::try_from(pid) {
            Ok(pgid) if pgid > 1 => pgid,
            Ok(_) => {
                drop(child.stdin.take());
                let kill_error = child.kill().err();
                let reap_error = bounded_reap(&mut child, ORACLE_CLEANUP_TIMEOUT).err();
                let mut errors = [None, None];
                if let Some(error) = kill_error.filter(|error| !benign_kill_error(error)) {
                    append_error(&mut errors[0], "keeper fallback kill", error);
                }
                if let Some(error) = reap_error {
                    append_error(&mut errors[1], "keeper reap", error);
                }
                let detail = cleanup_error(&errors, false)
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default();
                return Err(std::io::Error::other(format!(
                    "invalid keeper process group id{detail}"
                )));
            }
            Err(error) => {
                drop(child.stdin.take());
                let kill_error = child.kill().err();
                let reap_error = bounded_reap(&mut child, ORACLE_CLEANUP_TIMEOUT).err();
                let mut errors = [None, None, None];
                append_error(
                    &mut errors[0],
                    "invalid keeper pid",
                    std::io::Error::other(error),
                );
                if let Some(error) = kill_error.filter(|error| !benign_kill_error(error)) {
                    append_error(&mut errors[1], "keeper fallback kill", error);
                }
                if let Some(error) = reap_error {
                    append_error(&mut errors[2], "keeper reap", error);
                }
                return Err(cleanup_error(&errors, false)
                    .expect("invalid keeper pid must produce an error"));
            }
        };
        let stdin = child.stdin.take();
        Ok(Self {
            child,
            oracle_child: None,
            stdin,
            pgid,
            oracle_reaped: false,
            keeper_reaped: false,
            keeper_wait_uncertain: false,
            cleanup_finished: false,
            cleanup_deadline: None,
        })
    }

    fn attach_oracle_child(&mut self, child: Child) {
        debug_assert!(self.oracle_child.is_none());
        self.oracle_child = Some(child);
    }

    fn configure_command(&self, command: &mut Command) {
        use std::os::unix::process::CommandExt;
        command.process_group(self.pgid);
    }

    fn cleanup(&mut self, timeout: Duration) -> std::io::Result<()> {
        if self.cleanup_finished {
            return Ok(());
        }

        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        const SIGKILL: i32 = 9;
        // One deadline covers the group signal, fallback, and both waits.
        let deadline = *self
            .cleanup_deadline
            .get_or_insert_with(|| Instant::now() + timeout);
        let mut group_error = None;
        if !self.keeper_reaped
            && !self.keeper_wait_uncertain
            && unsafe { kill(-self.pgid, SIGKILL) } != 0
        {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(3) {
                append_error(
                    &mut group_error,
                    "oracle process-tree cleanup failed",
                    error,
                );
            }
        }

        // A failed group signal must not prevent a direct-child fallback, but
        // the original group error remains part of the returned error.
        let mut direct_error = None;
        let group_signal_failed = group_error.is_some();
        if let Some(error) = self
            .oracle_child
            .as_mut()
            .filter(|_| group_signal_failed)
            .and_then(|child| child.kill().err())
            .filter(|error| !benign_kill_error(error))
        {
            append_error(
                &mut direct_error,
                "oracle direct-child cleanup failed",
                error,
            );
        }
        drop(self.stdin.take());

        let mut timed_out = false;
        loop {
            if !self.oracle_reaped {
                match self.oracle_child.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(_)) => self.oracle_reaped = true,
                        Ok(None) => {}
                        Err(error) => append_error(
                            &mut direct_error,
                            "oracle direct-child wait failed",
                            error,
                        ),
                    },
                    None => self.oracle_reaped = true,
                }
            }
            if !self.keeper_reaped {
                match self.child.try_wait() {
                    Ok(Some(_)) => self.keeper_reaped = true,
                    Ok(None) => {
                        if Instant::now() >= deadline {
                            timed_out = true;
                            break;
                        }
                    }
                    Err(error) => {
                        self.keeper_wait_uncertain = true;
                        append_error(&mut group_error, "keeper wait failed", error);
                    }
                }
            }
            if self.oracle_reaped && self.keeper_reaped {
                break;
            }
            if Instant::now() >= deadline {
                timed_out = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        if self.oracle_reaped && self.keeper_reaped {
            // This is the only terminal-state transition. Drop must never send
            // a numeric-PGID signal after the keeper has been reaped.
            self.cleanup_finished = true;
        }
        let errors = [group_error, direct_error];
        cleanup_error(&errors, timed_out).map_or(Ok(()), Err)
    }
}

#[cfg(unix)]
impl Drop for OracleGroupKeeper {
    fn drop(&mut self) {
        if self.cleanup_finished {
            return;
        }
        let deadline = *self
            .cleanup_deadline
            .get_or_insert_with(|| Instant::now() + ORACLE_CLEANUP_TIMEOUT);
        drop(self.stdin.take());

        // The keeper reserves the PGID. Never signal it once waitpid has
        // reaped the keeper, because that numeric PGID may already be reused.
        if !self.keeper_reaped && !self.keeper_wait_uncertain {
            unsafe extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            const SIGKILL: i32 = 9;
            let _ = unsafe { kill(-self.pgid, SIGKILL) };
        }
        let oracle_reaped = self.oracle_reaped;
        if let Some(child) = self.oracle_child.as_mut().filter(|_| !oracle_reaped) {
            let _ = child.kill();
        }
        while !self.oracle_reaped || !self.keeper_reaped {
            if Instant::now() >= deadline {
                break;
            }
            if !self.oracle_reaped {
                if let Some(child) = self.oracle_child.as_mut() {
                    if matches!(child.try_wait(), Ok(Some(_))) {
                        self.oracle_reaped = true;
                    }
                } else {
                    self.oracle_reaped = true;
                }
            }
            if !self.keeper_reaped && matches!(self.child.try_wait(), Ok(Some(_))) {
                self.keeper_reaped = true;
            }
            if !self.oracle_reaped || !self.keeper_reaped {
                thread::sleep(Duration::from_millis(5));
            }
        }
        self.cleanup_finished = self.oracle_reaped && self.keeper_reaped;
    }
}

#[cfg(unix)]
fn run_file_backed(
    command: &mut Command,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
) -> std::io::Result<OracleCommandOutput> {
    run_file_backed_with_limit(
        command,
        stdout_path,
        stderr_path,
        timeout,
        ORACLE_MAX_OUTPUT_BYTES,
    )
}

#[cfg(unix)]
fn run_file_backed_with_limit(
    command: &mut Command,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
    output_limit: u64,
) -> std::io::Result<OracleCommandOutput> {
    let stdout = fs::File::create(stdout_path)?;
    let stderr = fs::File::create(stderr_path)?;
    // Clone the output files before creating the keeper. A clone failure must
    // be observable, not deferred to an unreportable Drop path.
    let child_stdout = stdout.try_clone()?;
    let child_stderr = stderr.try_clone()?;

    let mut keeper = OracleGroupKeeper::spawn()?;
    keeper.configure_command(command);
    command.stdout(Stdio::from(child_stdout));
    command.stderr(Stdio::from(child_stderr));

    if let Err(spawn_error) = command
        .spawn()
        .map(|child| keeper.attach_oracle_child(child))
    {
        let cleanup_error = keeper.cleanup(ORACLE_CLEANUP_TIMEOUT);
        return match cleanup_error {
            Ok(()) => Err(spawn_error),
            Err(cleanup_error) => Err(std::io::Error::new(
                spawn_error.kind(),
                format!("{spawn_error}; {cleanup_error}"),
            )),
        };
    }

    let deadline = Instant::now() + timeout;
    let runner_result: std::io::Result<std::process::ExitStatus> = loop {
        match output_exceeded(stdout_path, stderr_path, output_limit) {
            Ok(true) => {
                break Err(std::io::Error::other(
                    "oracle subprocess output exceeded its bounded limit",
                ));
            }
            Ok(false) => {}
            Err(err) => {
                break Err(err);
            }
        }
        match keeper
            .oracle_child
            .as_mut()
            .expect("oracle child attached")
            .try_wait()
        {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(err) => break Err(err),
        }
        if Instant::now() >= deadline {
            break Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle subprocess exceeded its bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };

    let cleanup_error = keeper.cleanup(ORACLE_CLEANUP_TIMEOUT).err();
    let status = match (runner_result, cleanup_error) {
        (Ok(status), None) => status,
        (Err(error), None) => return Err(error),
        (Ok(_), Some(cleanup_error)) => return Err(cleanup_error),
        (Err(error), Some(cleanup_error)) => {
            return Err(std::io::Error::new(
                error.kind(),
                format!("{error}; {cleanup_error}"),
            ));
        }
    };

    drop(stdout);
    drop(stderr);
    let stdout = read_capped_file(stdout_path, output_limit)?;
    let stderr = read_capped_file(stderr_path, output_limit)?;
    Ok(OracleCommandOutput {
        status,
        stdout,
        stderr,
    })
}

#[cfg(unix)]
fn output_exceeded(stdout_path: &Path, stderr_path: &Path, limit: u64) -> std::io::Result<bool> {
    for path in [stdout_path, stderr_path] {
        match fs::metadata(path) {
            Ok(metadata) if metadata.len() > limit => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

#[cfg(unix)]
fn read_capped_file(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut output = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut output)?;
    if output.len() as u64 > limit {
        return Err(std::io::Error::other(
            "oracle subprocess output exceeded its bounded limit",
        ));
    }
    Ok(output)
}

#[cfg(unix)]
fn wait_until<F>(timeout: Duration, mut predicate: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    false
}

#[cfg(unix)]
#[derive(Debug)]
struct ProcessObservation {
    pid: u32,
    ppid: u32,
    pgid: u32,
    uid: u32,
    stat: String,
}

#[cfg(unix)]
fn parse_process_observation(
    output: &OracleCommandOutput,
    requested_pid: u32,
) -> std::io::Result<Option<ProcessObservation>> {
    let line = String::from_utf8_lossy(&output.stdout);
    if !output.stderr.is_empty() {
        return Err(std::io::Error::other(format!(
            "ps observer wrote to stderr: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if line.trim().is_empty() {
        if output.status.code() == Some(1) && output.stdout.is_empty() {
            return Ok(None);
        }
        return Err(std::io::Error::other(format!(
            "ps observer failed with {}",
            output.status
        )));
    }

    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "ps observer failed with {}",
            output.status
        )));
    }

    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(std::io::Error::other(format!(
            "ps observer returned malformed output: {line:?}"
        )));
    }
    let parse = |field: &str, name: &str| {
        field.parse::<u32>().map_err(|error| {
            std::io::Error::other(format!("ps observer returned invalid {name}: {error}"))
        })
    };
    let observation = ProcessObservation {
        pid: parse(fields[0], "pid")?,
        ppid: parse(fields[1], "ppid")?,
        pgid: parse(fields[2], "pgid")?,
        uid: parse(fields[3], "uid")?,
        stat: fields[4].to_owned(),
    };
    if observation.pid != requested_pid {
        return Err(std::io::Error::other(format!(
            "ps observer returned pid {}, expected {requested_pid}",
            observation.pid
        )));
    }
    Ok(Some(observation))
}

#[cfg(unix)]
fn observe_process(pid: u32) -> std::io::Result<Option<ProcessObservation>> {
    let temp_root = tempfile::tempdir()?;
    let mut command = Command::new("/bin/ps");
    command.args(["-o", "pid=,ppid=,pgid=,uid=,stat=", "-p", &pid.to_string()]);
    let output = run_file_backed_with_limit(
        &mut command,
        &temp_root.path().join("stdout"),
        &temp_root.path().join("stderr"),
        ORACLE_CLEANUP_TIMEOUT,
        16 * 1024,
    )?;
    parse_process_observation(&output, pid)
}

#[cfg(unix)]
#[test]
fn process_observation_parser_rejects_genuine_errors_and_unexpected_rows() {
    let output = |status: i32, stdout: &str, stderr: &str| OracleCommandOutput {
        status: std::process::ExitStatus::from_raw(status << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    };

    assert!(
        parse_process_observation(&output(1, "", ""), 1234)
            .expect("ordinary missing PID is absence")
            .is_none()
    );
    assert!(parse_process_observation(&output(0, " \n", ""), 1234).is_err());
    assert!(parse_process_observation(&output(1, " \n", ""), 1234).is_err());
    assert!(parse_process_observation(&output(1, "", "EPERM"), 1234).is_err());
    assert!(parse_process_observation(&output(23, "", ""), 1234).is_err());
    assert!(parse_process_observation(&output(1, "1234 2 3 4 R\n", ""), 1234).is_err());
    assert!(parse_process_observation(&output(0, "1234 2 3 4\n", ""), 1234).is_err());
    assert!(parse_process_observation(&output(0, "5678 2 3 4 R\n", ""), 1234).is_err());
    assert!(
        parse_process_observation(&output(0, "1234 2 3 4 R\n1234 2 3 4 R\n", ""), 1234,).is_err()
    );
}

#[cfg(unix)]
fn own_uid() -> u32 {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

#[cfg(unix)]
fn assert_owned_running_with_parent(pid: u32, pgid: i32, ppid: u32) {
    let observation = observe_process(pid)
        .expect("ps observer must provide a trustworthy pre-cleanup observation")
        .expect("owned process must exist before cleanup");
    assert_eq!(observation.pid, pid);
    assert_eq!(observation.ppid, ppid);
    assert_eq!(observation.pgid, pgid as u32);
    assert_eq!(observation.uid, own_uid());
    assert!(!observation.stat.starts_with('Z'));
}

#[cfg(unix)]
fn assert_owned_running(pid: u32, pgid: i32) {
    assert_owned_running_with_parent(pid, pgid, std::process::id());
}

#[cfg(unix)]
fn wait_for_process_state(pid: u32, state: char, timeout: Duration) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match observe_process(pid)? {
            Some(observation) if observation.stat.starts_with(state) => return Ok(()),
            Some(_) if Instant::now() >= deadline => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("process {pid} did not reach state {state}"),
                ));
            }
            Some(_) => {}
            None => {
                return Err(std::io::Error::other(format!(
                    "process {pid} disappeared before reaching state {state}"
                )));
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
fn wait_for_process_death(pid: u32, timeout: Duration) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if observe_process(pid)?.is_none() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("process {pid} remained observable after cleanup"),
            ));
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
struct OwnedChildGuard(Child);

#[cfg(unix)]
impl Drop for OwnedChildGuard {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        let _ = self.0.kill();
        let _ = bounded_reap(&mut self.0, ORACLE_CLEANUP_TIMEOUT);
    }
}

#[cfg(unix)]
fn helper_command(test_name: &str, marker: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("current test executable"));
    command
        .args(["--exact", test_name, "--nocapture"])
        .env(marker, "1");
    command
}

#[cfg(unix)]
#[test]
fn oracle_runner_output_helper() {
    if std::env::var_os("SYMERASEME_CONFIG_ORACLE_OUTPUT_HELPER").is_none() {
        return;
    }
    let chunk = [b'x'; 4096];
    let mut stdout = std::io::stdout().lock();
    for _ in 0..1024 {
        stdout.write_all(&chunk).expect("write helper output");
    }
    stdout.flush().expect("flush helper output");
}

#[cfg(unix)]
#[test]
fn oracle_runner_timeout_helper() {
    if std::env::var_os("SYMERASEME_CONFIG_ORACLE_TIMEOUT_HELPER").is_some() {
        thread::sleep(Duration::from_secs(30));
    }
}

// These probes require tree-safe termination and therefore remain Unix-only
// until the documented Windows Job Object capability is implemented.
#[cfg(unix)]
#[test]
fn run_file_backed_enforces_live_output_limit() {
    let tree = TestTree::new("runner-output-limit");
    let mut command = helper_command(
        "oracle_runner_output_helper",
        "SYMERASEME_CONFIG_ORACLE_OUTPUT_HELPER",
    );
    let error = run_file_backed_with_limit(
        &mut command,
        &tree.root.join("stdout"),
        &tree.root.join("stderr"),
        Duration::from_secs(5),
        1024,
    )
    .expect_err("oversized output must fail closed");
    assert!(
        error.to_string().contains("bounded limit"),
        "unexpected runner error: {error}"
    );
}

#[cfg(unix)]
#[test]
fn run_file_backed_timeout_cleanup_is_bounded() {
    let tree = TestTree::new("runner-timeout");
    let mut command = helper_command(
        "oracle_runner_timeout_helper",
        "SYMERASEME_CONFIG_ORACLE_TIMEOUT_HELPER",
    );
    let started = Instant::now();
    let error = run_file_backed_with_limit(
        &mut command,
        &tree.root.join("stdout"),
        &tree.root.join("stderr"),
        Duration::from_millis(50),
        1024 * 1024,
    )
    .expect_err("timed-out subprocess must fail closed");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3));
}

// This oracle and the process-cleanup probes require Unix process groups.
// Windows parity remains explicitly capability-gated until Job Object support
// exists; the native config cases still run with semantic path normalization.
#[cfg(unix)]
#[test]
fn go_config_oracle_provenance_fixture_and_rust_results_match() {
    let fixture: Value = serde_json::from_str(GO_FIXTURE).expect("valid Go config fixture");
    let oracle = run_go_config_oracle();
    assert_eq!(
        oracle["provenance"],
        json!({
            "source_revision": "119ee9f84fe7c9e1485d25ab10aac8582e98395c",
            "source_path": "internal/config/config.go",
            "source_sha256": "d197afc83776a85880428994e32b0c1585c245ed52d86f9a31fe51889cbce32c",
            "schema": "symaira-eraseme.config-parity.v1"
        })
    );
    assert_eq!(
        oracle["cases"], fixture,
        "normalized Go config oracle output differs from the committed fixture"
    );
}

#[cfg(unix)]
#[test]
fn regression_oracle_worker_exits_before_cleanup_zombie_succeeds() {
    let tree = TestTree::new("native-zombie");
    let pid_file = tree.root.join("descendant.pid");
    let ready_file = tree.root.join("worker.ready");
    let mut keeper = OracleGroupKeeper::spawn().expect("spawn keeper");
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "/bin/sleep 30 & echo $! > \"$1\" && touch \"$2\" && read _",
            "oracle",
        ])
        .arg(&pid_file)
        .arg(&ready_file)
        .env_clear()
        .stdin(Stdio::piped());
    keeper.configure_command(&mut command);
    keeper.attach_oracle_child(command.spawn().expect("spawn worker"));
    let worker_pid = keeper.oracle_child.as_ref().expect("worker attached").id();

    assert!(
        wait_until(Duration::from_secs(2), || ready_file.exists()),
        "worker must create ready file"
    );
    assert_owned_running(worker_pid, keeper.pgid);
    let descendant_pid: u32 = fs::read_to_string(&pid_file)
        .expect("descendant pid file must exist")
        .trim()
        .parse()
        .expect("valid descendant pid");
    assert!(descendant_pid > 1);
    assert_owned_running_with_parent(descendant_pid, keeper.pgid, worker_pid);

    drop(
        keeper
            .oracle_child
            .as_mut()
            .expect("worker attached")
            .stdin
            .take(),
    );
    wait_for_process_state(worker_pid, 'Z', Duration::from_secs(2))
        .expect("worker must reach zombie state without being reaped");

    let cleanup_result = keeper.cleanup(ORACLE_CLEANUP_TIMEOUT);
    assert!(
        cleanup_result.is_ok(),
        "cleanup with exited worker must succeed: {cleanup_result:?}"
    );
    assert!(keeper.oracle_reaped, "worker child must be reaped");
    wait_for_process_death(worker_pid, Duration::from_secs(2))
        .expect("worker must be absent after cleanup");
    wait_for_process_death(descendant_pid, Duration::from_secs(2))
        .expect("owned descendant must be absent after cleanup");
}

#[cfg(unix)]
#[test]
fn regression_oracle_live_worker_and_descendant_cleaned_on_timeout() {
    let tree = TestTree::new("timeout-descendant");
    let pid_file = tree.root.join("descendant.pid");
    let ready_file = tree.root.join("ready");
    let mut command = Command::new("/bin/sh");
    let script = "/bin/sleep 30 & echo $! > \"$1\" && touch \"$2\" && /bin/sleep 30";
    command
        .args(["-c", script, "oracle"])
        .arg(&pid_file)
        .arg(&ready_file)
        .env_clear();

    let started = Instant::now();
    let error = run_file_backed_with_limit(
        &mut command,
        &tree.root.join("stdout"),
        &tree.root.join("stderr"),
        Duration::from_millis(500),
        ORACLE_MAX_OUTPUT_BYTES,
    )
    .expect_err("timed-out command must fail closed");

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(ready_file.exists(), "worker must reach the ready handshake");

    let pid_str = fs::read_to_string(&pid_file).expect("descendant pid file must exist");
    let descendant_pid: u32 = pid_str.trim().parse().expect("valid descendant pid");
    assert!(descendant_pid > 1);

    wait_for_process_death(descendant_pid, Duration::from_secs(2))
        .expect("descendant process must be absent after timeout cleanup");
}

#[cfg(unix)]
#[test]
fn regression_oracle_live_worker_and_descendant_cleaned_on_output_limit() {
    let tree = TestTree::new("output-limit-descendant");
    let pid_file = tree.root.join("descendant.pid");
    let ready_file = tree.root.join("ready");
    let mut command = Command::new("/bin/sh");
    let script = "/bin/sleep 30 & echo $! > \"$1\" && touch \"$2\" && while true; do echo 'overflow-bytes-1234567890'; done";
    command
        .args(["-c", script, "oracle"])
        .arg(&pid_file)
        .arg(&ready_file)
        .env_clear();

    let error = run_file_backed_with_limit(
        &mut command,
        &tree.root.join("stdout"),
        &tree.root.join("stderr"),
        Duration::from_secs(5),
        1024,
    )
    .expect_err("oversized output must fail closed");

    assert!(
        error.to_string().contains("bounded limit"),
        "unexpected runner error: {error}"
    );
    assert!(ready_file.exists(), "worker must reach the ready handshake");

    let pid_str = fs::read_to_string(&pid_file).expect("descendant pid file must exist");
    let descendant_pid: u32 = pid_str.trim().parse().expect("valid descendant pid");
    assert!(descendant_pid > 1);

    wait_for_process_death(descendant_pid, Duration::from_secs(2))
        .expect("descendant process must be absent after output cleanup");
}

#[cfg(unix)]
#[test]
fn regression_oracle_exited_worker_with_live_descendant_cleaned_on_normal_return() {
    let tree = TestTree::new("normal-return-descendant");
    let pid_file = tree.root.join("descendant.pid");
    let ready_file = tree.root.join("ready");
    let mut command = Command::new("/bin/sh");
    let script =
        "/bin/sleep 30 & echo $! > \"$1\" && touch \"$2\" && echo 'normal-complete' && exit 0";
    command
        .args(["-c", script, "oracle"])
        .arg(&pid_file)
        .arg(&ready_file)
        .env_clear();

    let output = run_file_backed(
        &mut command,
        &tree.root.join("stdout"),
        &tree.root.join("stderr"),
        Duration::from_secs(5),
    )
    .expect("command must complete normally");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("normal-complete"));
    assert!(ready_file.exists(), "worker must reach the ready handshake");

    let pid_str = fs::read_to_string(&pid_file).expect("descendant pid file must exist");
    let descendant_pid: u32 = pid_str.trim().parse().expect("valid descendant pid");
    assert!(descendant_pid > 1);

    wait_for_process_death(descendant_pid, Duration::from_secs(2))
        .expect("descendant process must be absent after normal cleanup");
}

#[cfg(unix)]
#[test]
fn regression_oracle_successful_ordinary_execution() {
    let tree = TestTree::new("ordinary-exec");
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "echo 'oracle-test-payload'"])
        .env_clear();

    let output = run_file_backed(
        &mut command,
        &tree.root.join("stdout"),
        &tree.root.join("stderr"),
        Duration::from_secs(5),
    )
    .expect("ordinary command execution must succeed");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "oracle-test-payload"
    );
}

#[cfg(unix)]
#[test]
fn regression_outside_separately_owned_group_survivor_unaffected() {
    use std::os::unix::process::CommandExt;
    let tree = TestTree::new("outside-survivor");

    let mut outside_command = Command::new("/bin/sh");
    outside_command
        .args(["-c", "read _"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut outside_child = OwnedChildGuard(outside_command.spawn().expect("spawn outside child"));
    let outside_pid = outside_child.0.id();
    assert!(outside_pid > 1);
    assert_owned_running(outside_pid, outside_pid as i32);

    let mut oracle_cmd = Command::new("/bin/sh");
    oracle_cmd.args(["-c", "/bin/sleep 30"]).env_clear();

    let err = run_file_backed_with_limit(
        &mut oracle_cmd,
        &tree.root.join("stdout"),
        &tree.root.join("stderr"),
        Duration::from_millis(50),
        ORACLE_MAX_OUTPUT_BYTES,
    )
    .expect_err("timed out command must error");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    assert_owned_running(outside_pid, outside_pid as i32);

    drop(outside_child.0.stdin.take());
    outside_child.0.kill().expect("outside sentinel kill");
    bounded_reap(&mut outside_child.0, ORACLE_CLEANUP_TIMEOUT)
        .expect("outside sentinel reap must be bounded");
    wait_for_process_death(outside_pid, Duration::from_secs(2))
        .expect("outside sentinel must be absent after its own cleanup");
}

#[cfg(unix)]
#[test]
fn regression_genuine_group_error_propagation_on_dead_keeper() {
    let mut keeper = OracleGroupKeeper::spawn().expect("spawn keeper");
    let keeper_pid = keeper.child.id();

    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    const SIGKILL: i32 = 9;
    unsafe {
        kill(keeper_pid as i32, SIGKILL);
    }

    wait_for_process_state(keeper_pid, 'Z', Duration::from_secs(2))
        .expect("keeper must reach zombie state");

    let result = keeper.cleanup(ORACLE_CLEANUP_TIMEOUT);
    #[cfg(target_os = "macos")]
    {
        let error = result.expect_err("unreaped dead keeper on Darwin must propagate EPERM");
        let error_msg = error.to_string();
        assert!(
            error_msg.contains("oracle process-tree cleanup failed")
                && (error_msg.contains("Operation not permitted")
                    || error_msg.contains("os error 1")),
            "unexpected error message: {error_msg}"
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        assert!(
            result.is_ok(),
            "non-Darwin control expects the documented ESRCH race to be benign: {result:?}"
        );
    }
    assert!(keeper.keeper_reaped, "keeper must be reaped after cleanup");
    wait_for_process_death(keeper_pid, Duration::from_secs(2))
        .expect("keeper must be absent after cleanup");
}

fn normalized_result(root: &Path, config: &Config, storage: &Storage) -> Value {
    let cache_root = storage
        .temp_dir
        .parent()
        .and_then(Path::parent)
        .expect("encrypted temp dir has cache/tool/database shape");
    json!({
        "config": config,
        "storage": {
            "data_dir": normalize_path(root, "$ROOT", &storage.data_dir),
            "db_dir": normalize_path(root, "$ROOT", &storage.db_dir),
            "db_path": normalize_path(root, "$ROOT", &storage.db_path),
            "temp_dir": normalize_path(cache_root, "$CACHE", &storage.temp_dir),
            "encrypt_db": storage.encrypt_db,
        },
    })
}

fn normalize_path(root: &Path, placeholder: &str, value: &Path) -> String {
    let root = root.to_string_lossy().replace('\\', "/");
    let value = value.to_string_lossy().replace('\\', "/");
    let root = root.trim_end_matches('/');
    if value == root {
        return placeholder.to_owned();
    }
    if let Some(suffix) = value
        .strip_prefix(root)
        .filter(|suffix| suffix.starts_with('/'))
    {
        return format!("{placeholder}{suffix}");
    }
    value
}

fn assert_field(error: ConfigError, field: &str) {
    assert_eq!(error.field(), Some(field), "error = {error}");
    assert!(error.to_string().contains(field), "error = {error}");
}

#[test]
fn cfg_001_defaults_and_persistent_storage_match_go_fixture() {
    let tree = TestTree::new("cfg-001");
    let context = tree.context(BTreeMap::new());

    let config = defaults();
    assert_eq!(config.port, 8000);
    assert!(!config.encrypt_db);
    assert!(!config.allow_remote);

    let storage = resolve_storage(&context).expect("default storage");
    assert_eq!(
        normalized_result(&tree.root, &config, &storage),
        fixture("CFG-001")
    );
    assert!(
        !storage
            .db_path
            .starts_with(std::env::temp_dir().join("symeraseme"))
    );
}

#[test]
fn cfg_002_precedence_is_defaults_global_project_then_environment() {
    let tree = TestTree::new("cfg-002");
    let xdg = tree.root.join("xdg");
    let global = xdg.join("symeraseme/config.toml");
    tree.write(
        &global,
        "data_dir = \"global-data\"\ndb_dir = \"global-db\"\nencrypt_db = true\nport = 8100\nallow_remote = false\n",
    );
    tree.write(
        tree.project().join(".symeraseme.toml"),
        "data_dir = \"project-data\"\nencrypt_db = false\n",
    );
    let environment = BTreeMap::from([
        ("XDG_CONFIG_HOME".into(), xdg.to_string_lossy().into_owned()),
        ("SYMERASEME_DATA_DIR".into(), "~/env-data".into()),
        ("SYMERASEME_DB_DIR".into(), "env-db".into()),
        ("SYMERASEME_ENCRYPT_DB".into(), "true".into()),
        ("SYMERASEME_PORT".into(), "8123".into()),
        ("SYMERASEME_ALLOW_REMOTE".into(), "yes".into()),
    ]);
    let context = tree.context(environment);

    let config = load(&context).expect("precedence config");
    let storage = resolve_storage(&context).expect("precedence storage");
    assert_eq!(
        normalized_result(&tree.root, &config, &storage),
        fixture("CFG-002")
    );
}

#[test]
fn cfg_003_relative_and_tilde_paths_are_absolute_from_context() {
    let tree = TestTree::new("cfg-003");
    tree.write(
        tree.project().join(".symeraseme.toml"),
        "data_dir = \"~/custom/data\"\ndb_dir = \"relative-db\"\n",
    );
    let context = tree.context(BTreeMap::new());

    let storage = resolve_storage(&context).expect("path storage");
    assert_eq!(storage.data_dir, tree.home().join("custom/data"));
    assert_eq!(storage.db_dir, tree.project().join("relative-db"));
    assert!(storage.data_dir.is_absolute());
    assert!(storage.db_dir.is_absolute());
    assert!(storage.db_path.is_absolute());
    assert_eq!(
        normalized_result(&tree.root, &load(&context).unwrap(), &storage),
        fixture("CFG-003")
    );
}

#[test]
fn cfg_004_bool_and_integer_coercion_ignore_unknown_nested_fields() {
    let tree = TestTree::new("cfg-004");
    let xdg = tree.root.join("xdg");
    tree.write(
        xdg.join("symeraseme/config.toml"),
        "encrypt_db = 1\nallow_remote = \" oN \"\nport = \"8124\"\nunknown = true\n[legacy.server]\nport = 1\n",
    );
    let environment =
        BTreeMap::from([("XDG_CONFIG_HOME".into(), xdg.to_string_lossy().into_owned())]);
    let context = tree.context(environment);

    let config = load(&context).expect("coercion config");
    assert!(config.encrypt_db);
    assert!(config.allow_remote);
    assert_eq!(config.port, 8124);
    let storage = resolve_storage(&context).expect("coercion storage");
    assert_eq!(
        normalized_result(&tree.root, &config, &storage),
        fixture("CFG-004")
    );
}

#[test]
fn cfg_005_malformed_supported_values_are_classified_by_field() {
    let tree = TestTree::new("cfg-005");
    let mut environment = BTreeMap::from([("SYMERASEME_ENCRYPT_DB".into(), "maybe".into())]);
    let context = tree.context(environment.clone());
    assert_field(load(&context).expect_err("invalid bool"), "encrypt_db");

    environment.insert("SYMERASEME_ENCRYPT_DB".into(), "false".into());
    environment.insert("SYMERASEME_PORT".into(), "65536".into());
    assert_field(
        load(&tree.context(environment.clone())).expect_err("invalid port"),
        "port",
    );

    environment.remove("SYMERASEME_PORT");
    environment.insert("SYMERASEME_DATA_DIR".into(), "".into());
    assert!(
        load(&tree.context(environment)).is_ok(),
        "empty env is ignored"
    );

    tree.write(
        tree.project().join(".symeraseme.toml"),
        "db_dir = \"   \"\n",
    );
    assert_field(
        load(&tree.context(BTreeMap::new())).expect_err("empty path"),
        "db_dir",
    );
    tree.write(
        tree.project().join(".symeraseme.toml"),
        "db_dir = \"bad\\u0000path\"\n",
    );
    assert_field(
        load(&tree.context(BTreeMap::new())).expect_err("NUL path"),
        "db_dir",
    );

    let fixture = fixture("CFG-005");
    assert_eq!(
        fixture["errors"],
        json!(["encrypt_db", "port", "db_dir", "db_dir"])
    );
}

#[test]
fn cfg_006_relative_xdg_falls_back_and_temp_dir_is_user_scoped() {
    let tree = TestTree::new("cfg-006");
    tree.write(
        tree.home().join(".config/symeraseme/config.toml"),
        "data_dir = \"default-global\"\nencrypt_db = 1\nport = 9000\n",
    );
    let environment = BTreeMap::from([("XDG_CONFIG_HOME".into(), "relative-xdg".into())]);
    let context = tree.context(environment);

    let config = load(&context).expect("fallback config");
    let storage = resolve_storage(&context).expect("fallback storage");
    assert!(storage.encrypt_db);
    assert_eq!(config.port, 9000);
    assert_eq!(storage.data_dir, tree.project().join("default-global"));
    assert_eq!(
        storage.temp_dir,
        default_encrypted_temp_dir(&context).expect("temp dir")
    );
    assert!(storage.temp_dir.starts_with(tree.home()));
    assert!(
        storage.temp_dir != std::env::temp_dir().join("symeraseme/database"),
        "encrypted temp dir must not be the shared temp location"
    );
    assert_eq!(
        normalized_result(&tree.root, &config, &storage),
        fixture("CFG-006")
    );
}
