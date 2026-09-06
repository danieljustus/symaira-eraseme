//! Subprocess isolation and raw byte capture.

use crate::case::{Case, FixtureFile, Program};
use crate::http::{HttpExchange, MockHttpServer};
use crate::mcp::{RawFrame, record_frames};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawStatus {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunResult {
    pub status: RawStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub http_exchanges: Vec<HttpExchange>,
    pub mcp_frames: Vec<RawFrame>,
    pub sandbox: PathBuf,
    pub cwd: PathBuf,
    pub home: PathBuf,
    pub xdg: PathBuf,
}

impl Drop for RunResult {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.sandbox);
    }
}

struct SandboxGuard {
    path: PathBuf,
    keep: bool,
}

impl Drop for SandboxGuard {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn unique_dir() -> std::io::Result<PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    loop {
        let path = std::env::temp_dir().join(format!(
            "symeraseme-parity-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => {
                set_mode(&path, 0o700)?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

pub fn validate_fixture_path(path: &Path) -> std::io::Result<()> {
    let raw = path.to_string_lossy();
    let windows_drive_prefix =
        raw.len() >= 2 && raw.as_bytes()[1] == b':' && raw.as_bytes()[0].is_ascii_alphabetic();
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || windows_drive_prefix
        || raw.starts_with(r"\\")
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "fixture path must be relative and sandbox-local",
        ));
    }
    for component in path.components() {
        if matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir | Component::CurDir
        ) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "fixture path escapes the sandbox",
            ));
        }
    }
    Ok(())
}

fn create_layout(root: &Path, files: &[FixtureFile]) -> std::io::Result<PathBuf> {
    let cwd = root.join("cwd");
    fs::create_dir(&cwd)?;
    set_mode(&cwd, 0o700)?;
    for fixture in files {
        validate_fixture_path(&fixture.relative_path)?;
        let path = cwd.join(&fixture.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            set_mode(parent, 0o700)?;
        }
        fs::write(&path, &fixture.contents)?;
        set_mode(&path, fixture.mode.unwrap_or(0o600))?;
    }
    Ok(cwd)
}

fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

fn reject_reserved_environment(case: &Case) -> std::io::Result<()> {
    const RESERVED: [&str; 6] = [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "TMPDIR",
        "TEMP",
    ];
    if let Some(name) = case
        .environment
        .values
        .keys()
        .find(|name| RESERVED.contains(&name.as_str()))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("case cannot override reserved environment variable {name}"),
        ));
    }
    Ok(())
}

/// Unix uses a dedicated process group so timeout cleanup includes descendants.
/// Other platforms return an explicit unsupported-capability error until a
/// Windows Job Object implementation is added; they never claim tree safety.
#[cfg(unix)]
fn configure_process_group(command: &mut Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    unsafe extern "C" {
        fn setpgid(pid: i32, pgid: i32) -> i32;
    }
    unsafe {
        command.pre_exec(|| {
            let result = setpgid(0, 0);
            if result == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "parity subprocess tree cleanup is unsupported on this platform; use Unix or add Windows Job Object support",
    ))
}

fn kill_process_group(child: &mut Child) {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        if let Ok(pid) = i32::try_from(child.id()) {
            const SIGKILL: i32 = 9;
            let _ = unsafe { kill(-pid, SIGKILL) };
        }
    }
    let _ = child.kill();
}

fn raw_status(status: &std::process::ExitStatus, timed_out: bool) -> RawStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        RawStatus {
            exit_code: status.code(),
            signal: status.signal(),
            timed_out,
        }
    }
    #[cfg(not(unix))]
    {
        RawStatus {
            exit_code: status.code(),
            signal: None,
            timed_out,
        }
    }
}

const WRITER_CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

fn finish_stdin_writer(
    writer: thread::JoinHandle<()>,
    result: Receiver<std::io::Result<()>>,
) -> std::io::Result<()> {
    let write_result = result
        .recv_timeout(WRITER_CLEANUP_TIMEOUT)
        .map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("stdin writer cleanup exceeded bounded timeout: {error}"),
            )
        })?;
    writer
        .join()
        .map_err(|_| std::io::Error::other("stdin writer panicked"))?;
    match write_result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error),
    }
}

const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;
const READER_CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

fn read_capped(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(8192);
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(count) > MAX_CAPTURE_BYTES {
            return Err(std::io::Error::other(
                "subprocess output exceeded the bounded capture limit",
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
}

fn finish_reader(
    reader: thread::JoinHandle<()>,
    result: Receiver<std::io::Result<Vec<u8>>>,
    captured: Option<std::io::Result<Vec<u8>>>,
    child: &mut Child,
    stream: &str,
) -> std::io::Result<Vec<u8>> {
    let value = match captured {
        Some(value) => value,
        None => result
            .recv_timeout(READER_CLEANUP_TIMEOUT)
            .map_err(|error| {
                kill_process_group(child);
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("{stream} reader cleanup exceeded bounded timeout: {error}"),
                )
            })?,
    };
    reader
        .join()
        .map_err(|_| std::io::Error::other(format!("{stream} reader panicked")))?;
    value
}

pub fn run_program(case: &Case, program: &Program) -> std::io::Result<RunResult> {
    reject_reserved_environment(case)?;
    let sandbox = unique_dir()?;
    let mut cleanup = SandboxGuard {
        path: sandbox.clone(),
        keep: false,
    };
    let cwd = create_layout(&sandbox, &case.cwd.files)?;
    let home = sandbox.join("home");
    let xdg = sandbox.join("xdg");
    let tmp = sandbox.join("tmp");
    fs::create_dir(&home)?;
    fs::create_dir(&xdg)?;
    fs::create_dir(&tmp)?;
    set_mode(&home, 0o700)?;
    set_mode(&xdg, 0o700)?;
    set_mode(&tmp, 0o700)?;

    let http_server = case
        .http
        .as_ref()
        .map(|fixture| MockHttpServer::start_with_timeout(&fixture.responses, case.timeout))
        .transpose()?;
    let mut command = Command::new(&program.executable);
    command
        .args(&program.argv)
        .current_dir(&cwd)
        .env_clear()
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg)
        .env("XDG_DATA_HOME", &xdg)
        .env("XDG_CACHE_HOME", &xdg)
        .env("TMPDIR", &tmp)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(server) = &http_server {
        command.env("PARITY_HTTP_URL", format!("http://{}", server.address()));
    }
    for (name, value) in &case.environment.values {
        command.env(name, value);
    }
    configure_process_group(&mut command)?;
    let mut child = command.spawn()?;

    let (stdin_done, stdin_result) = mpsc::channel();
    let stdin_thread = child.stdin.take().map(|mut stdin| {
        let input = case.stdin.clone();
        thread::spawn(move || {
            let result = stdin.write_all(&input);
            drop(stdin);
            let _ = stdin_done.send(result);
        })
    });
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let (stdout_done, stdout_result) = mpsc::channel();
    let stdout_thread = thread::spawn(move || {
        let result = read_capped(stdout);
        let _ = stdout_done.send(result);
    });
    let (stderr_done, stderr_result) = mpsc::channel();
    let stderr_thread = thread::spawn(move || {
        let result = read_capped(stderr);
        let _ = stderr_done.send(result);
    });

    let deadline = Instant::now() + case.timeout;
    let mut timed_out = false;
    let mut capture_error = None;
    let mut stdout_captured = None;
    let mut stderr_captured = None;
    let status = loop {
        if let Ok(result) = stdout_result.try_recv() {
            if let Err(error) = &result {
                capture_error = Some(std::io::Error::other(error.to_string()));
                kill_process_group(&mut child);
            }
            stdout_captured = Some(result);
            if capture_error.is_some() {
                break child.wait()?;
            }
        }
        if let Ok(result) = stderr_result.try_recv() {
            if let Err(error) = &result {
                capture_error = Some(std::io::Error::other(error.to_string()));
                kill_process_group(&mut child);
            }
            stderr_captured = Some(result);
            if capture_error.is_some() {
                break child.wait()?;
            }
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            kill_process_group(&mut child);
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(5));
    };
    // The child may have exited while a descendant still owns an inherited
    // pipe. Terminate the dedicated group before bounded reader cleanup.
    kill_process_group(&mut child);
    if let Some(writer) = stdin_thread {
        finish_stdin_writer(writer, stdin_result)?;
    }
    let stdout = finish_reader(
        stdout_thread,
        stdout_result,
        stdout_captured,
        &mut child,
        "stdout",
    )?;
    let stderr = finish_reader(
        stderr_thread,
        stderr_result,
        stderr_captured,
        &mut child,
        "stderr",
    )?;
    if let Some(error) = capture_error {
        return Err(error);
    }
    let http_exchanges = http_server
        .map(|server| server.finish())
        .transpose()?
        .unwrap_or_default();
    let mcp_frames = if case.capture_mcp {
        record_frames(&case.stdin, &stdout)
    } else {
        Vec::new()
    };
    cleanup.keep = true;
    Ok(RunResult {
        status: raw_status(&status, timed_out),
        stdout,
        stderr,
        http_exchanges,
        mcp_frames,
        sandbox,
        cwd,
        home,
        xdg,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::{Case, Program};

    fn dummy(output: &str) -> Program {
        Program {
            executable: PathBuf::from("/bin/sh"),
            argv: vec!["-c".into(), format!("printf '%s' {output}")],
        }
    }

    #[test]
    fn rejects_absolute_root_prefix_and_parent_fixture_paths() {
        let paths = [
            PathBuf::from("/absolute"),
            PathBuf::from("../escape"),
            PathBuf::from(r"C:\escape"),
            PathBuf::from(r"\\server\share"),
        ];
        for path in paths {
            assert!(validate_fixture_path(&path).is_err(), "{}", path.display());
        }
    }

    #[test]
    fn isolates_home_and_cwd_and_captures_raw_bytes() {
        let case = Case::new("isolation", dummy("same"), dummy("same"));
        let result = run_program(&case, &case.go).expect("dummy program runs");
        assert_eq!(result.stdout, b"same");
        assert_ne!(result.cwd, std::env::current_dir().unwrap());
        assert!(result.home.starts_with(&result.sandbox));
        assert_eq!(result.status.exit_code, Some(0));
    }

    #[test]
    fn stdin_is_written_concurrently_and_closed_after_large_input() {
        let mut case = Case::new("large-stdin", dummy("same"), dummy("same"));
        case.stdin = vec![b'x'; 2 * 1024 * 1024];
        case.go = Program {
            executable: PathBuf::from("/bin/sh"),
            argv: vec!["-c".into(), "wc -c >/dev/null".into()],
        };
        let result = run_program(&case, &case.go).expect("stdin writer must not deadlock");
        assert_eq!(result.status.exit_code, Some(0));
    }

    #[cfg(unix)]
    #[test]
    fn descendant_holding_inherited_stdin_is_cleaned_up_boundedly() {
        let mut case = Case::new("stdin-descendant", dummy("same"), dummy("same"));
        case.stdin = vec![b'x'; 2 * 1024 * 1024];
        case.go = Program {
            executable: PathBuf::from("/bin/sh"),
            argv: vec!["-c".into(), "sleep 10 & exit 0".into()],
        };
        let started = Instant::now();
        let result = run_program(&case, &case.go).expect("descendant cleanup must be bounded");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(result.status.exit_code, Some(0));
    }

    #[test]
    fn timeout_marks_and_kills_the_process_group() {
        let mut case = Case::new("timeout", dummy("same"), dummy("same"));
        case.timeout = Duration::from_millis(30);
        case.go = Program {
            executable: PathBuf::from("/bin/sh"),
            argv: vec!["-c".into(), "sleep 10".into()],
        };
        let result = run_program(&case, &case.go).expect("timeout is captured");
        assert!(result.status.timed_out);
    }

    #[cfg(not(unix))]
    #[test]
    fn non_unix_process_tree_capability_is_explicitly_unsupported() {
        let mut command = Command::new("does-not-run");
        let error = configure_process_group(&mut command).expect_err("capability must be explicit");
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    }
}
