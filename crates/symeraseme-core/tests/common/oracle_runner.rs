use std::fs;
use std::io::{Read, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

/// Run a subprocess with file-backed, bounded output and owned-tree cleanup.
pub(crate) fn run(
    command: &mut Command,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
    output_limit: u64,
) -> std::io::Result<Output> {
    run_inner(command, stdout_path, stderr_path, timeout, output_limit)
}

pub(crate) fn configure_go_environment(command: &mut Command, temp_root: &Path) {
    command
        .env_clear()
        .env(
            "PATH",
            std::env::var_os("PATH").expect("PATH is configured"),
        )
        .env("HOME", temp_root)
        .env("TMP", temp_root)
        .env("TMPDIR", temp_root)
        .env("TEMP", temp_root)
        .env("GOCACHE", temp_root.join("go-cache"))
        .env("GOWORK", "off");
    for name in ["SystemRoot", "SYSTEMROOT", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

/// Run a subprocess with stdin backed by a private temporary file.
#[allow(dead_code)]
pub(crate) fn run_with_stdin(
    command: &mut Command,
    input: &[u8],
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
    output_limit: u64,
) -> std::io::Result<Output> {
    validate_stdin_len(input.len() as u64)?;
    let mut stdin_file =
        tempfile::NamedTempFile::new_in(stdout_path.parent().unwrap_or_else(|| Path::new(".")))?;
    stdin_file.write_all(input)?;
    stdin_file.as_file().sync_all()?;
    command.stdin(Stdio::from(stdin_file.reopen()?));
    run_inner(command, stdout_path, stderr_path, timeout, output_limit)
}

fn run_inner(
    command: &mut Command,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
    output_limit: u64,
) -> std::io::Result<Output> {
    configure_process_group(command)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut process = OwnedProcess::spawn(command)?;
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("oracle subprocess stdout pipe unavailable"))?;
    let stderr = process
        .child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("oracle subprocess stderr pipe unavailable"))?;
    let stdout_reader = CappedReader::spawn(stdout, output_limit);
    let stderr_reader = CappedReader::spawn(stderr, output_limit);
    let deadline = Instant::now() + timeout;

    let status = loop {
        if stdout_reader.exceeded() || stderr_reader.exceeded() {
            process.cleanup(Duration::from_secs(2))?;
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(std::io::Error::other(
                "oracle subprocess output exceeded its bounded limit",
            ));
        }
        match process.child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let cleanup = process.cleanup(Duration::from_secs(2));
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(with_cleanup_error(error, cleanup));
            }
        }
        if Instant::now() >= deadline {
            process.cleanup(Duration::from_secs(2))?;
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle subprocess exceeded its bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };

    // Always clean the owned tree before joining readers: a trusted descendant
    // may retain a pipe after the leader exits. This avoids an EOF hang.
    process.cleanup(Duration::from_secs(2))?;
    let stdout_bytes = stdout_reader.join()?;
    let stderr_bytes = stderr_reader.join()?;
    if stdout_bytes.1 || stderr_bytes.1 {
        return Err(std::io::Error::other(
            "oracle subprocess output exceeded its bounded limit",
        ));
    }
    write_bounded_file(stdout_path, &stdout_bytes.0)?;
    write_bounded_file(stderr_path, &stderr_bytes.0)?;
    Ok(Output {
        status,
        stdout: stdout_bytes.0,
        stderr: stderr_bytes.0,
    })
}

fn validate_stdin_len(length: u64) -> std::io::Result<()> {
    if length > 64 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "oracle subprocess stdin exceeds its bounded request limit",
        ));
    }
    Ok(())
}

struct CappedReader {
    handle: thread::JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
    exceeded: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CappedReader {
    fn spawn(mut pipe: impl Read + Send + 'static, limit: u64) -> Self {
        let exceeded_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = exceeded_flag.clone();
        Self {
            handle: thread::spawn(move || {
                let mut bytes = Vec::new();
                let mut exceeded = false;
                let mut buffer = [0_u8; 8192];
                loop {
                    let count = pipe.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    if !exceeded {
                        let remaining = limit.saturating_sub(bytes.len() as u64);
                        if count as u64 <= remaining {
                            bytes.extend_from_slice(&buffer[..count]);
                        } else {
                            let keep = remaining as usize;
                            bytes.extend_from_slice(&buffer[..keep]);
                            exceeded = true;
                            flag.store(true, std::sync::atomic::Ordering::Release);
                        }
                    }
                }
                Ok((bytes, exceeded))
            }),
            exceeded: exceeded_flag,
        }
    }

    fn exceeded(&self) -> bool {
        self.exceeded.load(std::sync::atomic::Ordering::Acquire)
    }

    fn join(self) -> std::io::Result<(Vec<u8>, bool)> {
        self.handle
            .join()
            .map_err(|_| std::io::Error::other("oracle output reader panicked"))?
    }
}

fn write_bounded_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)
}

fn with_cleanup_error(original: std::io::Error, cleanup: std::io::Result<()>) -> std::io::Error {
    match cleanup {
        Ok(()) => original,
        Err(error) => std::io::Error::other(format!("{original}; cleanup failed: {error}")),
    }
}

fn configure_process_group(command: &mut Command) -> std::io::Result<()> {
    // Unix cleanup covers trusted descendants that retain this process group.
    // It is not a sandbox: a deliberate setsid/escape descendant is outside scope.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe extern "C" {
            fn setpgid(pid: i32, pgid: i32) -> i32;
        }
        unsafe {
            command.pre_exec(|| {
                if setpgid(0, 0) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_SUSPENDED);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = command;
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "oracle process-tree cleanup unsupported on this platform",
        ));
    }
    Ok(())
}

struct OwnedProcess {
    child: Child,
    #[cfg(windows)]
    job: WindowsJob,
}

impl OwnedProcess {
    fn spawn(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(windows)]
        let mut child = command.spawn()?;
        #[cfg(not(windows))]
        let child = command.spawn()?;
        #[cfg(windows)]
        {
            let job = match WindowsJob::assign(&child) {
                Ok(job) => job,
                Err(error) => {
                    terminate_unassigned(&mut child)?;
                    return Err(std::io::Error::other(format!(
                        "oracle Windows Job Object assignment failed: {error}"
                    )));
                }
            };
            if let Err(error) = job.resume(child.id()) {
                terminate_unassigned(&mut child)?;
                return Err(std::io::Error::other(format!(
                    "oracle Windows suspended-process resume failed: {error}"
                )));
            }
            return Ok(Self { child, job });
        }
        #[cfg(not(windows))]
        Ok(Self { child })
    }

    fn cleanup(&mut self, timeout: Duration) -> std::io::Result<()> {
        #[cfg(windows)]
        let tree_result = self.job.terminate_tree();
        #[cfg(unix)]
        let tree_result = kill_process_group(&self.child);
        #[cfg(not(any(unix, windows)))]
        let tree_result = Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "oracle process-tree cleanup unsupported on this platform",
        ));
        // Closing the Windows Job Object can reap the leader before the direct
        // kill below. Do not signal an already-reaped child (or a recycled PID).
        let direct_result = match self.child.try_wait()? {
            Some(_) => Ok(()),
            None => self.child.kill(),
        };
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait()? {
                Some(_) => break,
                None if Instant::now() >= deadline => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "oracle subprocess cleanup exceeded its bounded timeout",
                    ));
                }
                None => thread::sleep(Duration::from_millis(10)),
            }
        }
        tree_result?;
        if let Err(error) = direct_result {
            let already_gone = matches!(
                error.kind(),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
            ) || error.raw_os_error() == Some(3);
            if !already_gone {
                return Err(std::io::Error::other(format!(
                    "oracle direct-child cleanup failed: {error}"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn kill_process_group(child: &Child) -> std::io::Result<()> {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let pid = i32::try_from(child.id()).map_err(|_| std::io::Error::other("invalid child pid"))?;
    if unsafe { kill(-pid, 9) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(3) {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(windows)]
const CREATE_SUSPENDED: u32 = 0x0000_0004;
#[cfg(windows)]
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
#[cfg(windows)]
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
#[cfg(windows)]
const PROCESS_ACCESS: u32 = 0x0001 | 0x0100 | 0x0800;

#[cfg(windows)]
#[repr(C)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}
#[cfg(windows)]
#[repr(C)]
struct IoCounters {
    read_operations: u64,
    write_operations: u64,
    other_operations: u64,
    read_bytes: u64,
    write_bytes: u64,
    other_bytes: u64,
}
#[cfg(windows)]
#[repr(C)]
struct JobObjectExtendedLimitInformation {
    basic: JobObjectBasicLimitInformation,
    io: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[cfg(windows)]
struct WindowsJob(isize);
#[cfg(windows)]
impl WindowsJob {
    fn assign(child: &Child) -> std::io::Result<Self> {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateJobObjectW(attrs: *const (), name: *const u16) -> isize;
            fn SetInformationJobObject(job: isize, class: u32, info: *mut (), len: u32) -> i32;
            fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
        }
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut limits = JobObjectExtendedLimitInformation {
            basic: JobObjectBasicLimitInformation {
                per_process_user_time_limit: 0,
                per_job_user_time_limit: 0,
                limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                minimum_working_set_size: 0,
                maximum_working_set_size: 0,
                active_process_limit: 0,
                affinity: 0,
                priority_class: 0,
                scheduling_class: 0,
            },
            io: IoCounters {
                read_operations: 0,
                write_operations: 0,
                other_operations: 0,
                read_bytes: 0,
                write_bytes: 0,
                other_bytes: 0,
            },
            process_memory_limit: 0,
            job_memory_limit: 0,
            peak_process_memory_used: 0,
            peak_job_memory_used: 0,
        };
        let process = match child_process_handle(child.id()) {
            Ok(process) => process,
            Err(error) => {
                close_handle(job);
                return Err(error);
            }
        };
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&mut limits as *mut JobObjectExtendedLimitInformation).cast::<()>(),
                std::mem::size_of_val(&limits) as u32,
            )
        } != 0
            && unsafe { AssignProcessToJobObject(job, process) } != 0;
        close_handle(process);
        if !ok {
            close_handle(job);
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(job))
    }
    fn resume(&self, pid: u32) -> std::io::Result<()> {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn NtResumeProcess(process: isize) -> i32;
        }
        let process = unsafe { OpenProcess(PROCESS_ACCESS, 0, pid) };
        if process == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let status = unsafe { NtResumeProcess(process) };
        close_handle(process);
        if status != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    fn terminate_tree(&mut self) -> std::io::Result<()> {
        let handle = self.0;
        self.0 = 0;
        close_handle(handle);
        Ok(())
    }
}
#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        if self.0 != 0 {
            close_handle(self.0);
            self.0 = 0;
        }
    }
}
#[cfg(windows)]
fn child_process_handle(pid: u32) -> std::io::Result<isize> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    }
    let handle = unsafe { OpenProcess(PROCESS_ACCESS, 0, pid) };
    if handle == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(handle)
    }
}
#[cfg(windows)]
fn terminate_unassigned(child: &mut Child) -> std::io::Result<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn TerminateProcess(process: isize, code: u32) -> i32;
    }
    let handle = child_process_handle(child.id())?;
    let ok = unsafe { TerminateProcess(handle, 1) } != 0;
    close_handle(handle);
    if !ok {
        return Err(std::io::Error::last_os_error());
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle unassigned Windows child cleanup exceeded its bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}
#[cfg(windows)]
fn close_handle(handle: isize) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CloseHandle(handle: isize) -> i32;
    }
    if handle != 0 {
        let _ = unsafe { CloseHandle(handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[cfg(unix)]
    fn command(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[cfg(windows)]
    fn command(script: &str) -> Command {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", script]);
        command
    }

    #[cfg(unix)]
    #[test]
    fn normal_and_nonzero_exit_are_captured() {
        let dir = tempfile::tempdir().unwrap();
        let mut ok = command("printf ok");
        assert_eq!(
            run(
                &mut ok,
                &dir.path().join("o"),
                &dir.path().join("e"),
                Duration::from_secs(1),
                32
            )
            .unwrap()
            .stdout,
            b"ok"
        );
        let mut bad = command("printf bad; exit 7");
        let output = run(
            &mut bad,
            &dir.path().join("o2"),
            &dir.path().join("e2"),
            Duration::from_secs(1),
            32,
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"bad");
    }

    #[cfg(windows)]
    #[test]
    fn windows_normal_and_nonzero_exit_are_captured() {
        let dir = tempfile::tempdir().unwrap();
        let mut ok = command("echo ok");
        let output = run(
            &mut ok,
            &dir.path().join("o"),
            &dir.path().join("e"),
            Duration::from_secs(1),
            32,
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"ok\r\n");

        let mut bad = command("echo bad & exit /B 7");
        let output = run(
            &mut bad,
            &dir.path().join("o2"),
            &dir.path().join("e2"),
            Duration::from_secs(1),
            32,
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"bad\r\n");
    }

    #[test]
    fn stdin_request_limit_is_checked_before_tempfile_write() {
        assert!(validate_stdin_len(64 * 1024 * 1024 - 1).is_ok());
        assert!(validate_stdin_len(64 * 1024 * 1024).is_ok());
        assert_eq!(
            validate_stdin_len(64 * 1024 * 1024 + 1).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }

    #[cfg(unix)]
    #[test]
    fn timeout_and_output_limit_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let mut slow = command("sleep 10");
        assert_eq!(
            run(
                &mut slow,
                &dir.path().join("o"),
                &dir.path().join("e"),
                Duration::from_millis(30),
                32
            )
            .unwrap_err()
            .kind(),
            std::io::ErrorKind::TimedOut
        );
        let mut loud = command("yes x");
        assert!(
            run(
                &mut loud,
                &dir.path().join("o2"),
                &dir.path().join("e2"),
                Duration::from_secs(1),
                32
            )
            .unwrap_err()
            .to_string()
            .contains("bounded limit")
        );
    }

    #[cfg(unix)]
    #[test]
    fn leader_exit_descendant_holding_output_is_cleaned() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = command("printf ready; sleep 10 & exit 0");
        let output = run(
            &mut command,
            &dir.path().join("o"),
            &dir.path().join("e"),
            Duration::from_secs(1),
            32,
        )
        .unwrap();
        assert_eq!(output.stdout, b"ready");
    }
}
