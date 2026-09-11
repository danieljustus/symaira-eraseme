use std::fs;
use std::io::Read;
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
    let mut stdin_file =
        tempfile::NamedTempFile::new_in(stdout_path.parent().unwrap_or_else(|| Path::new(".")))?;
    std::io::Write::write_all(&mut stdin_file, input)?;
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
    let stdout = fs::File::create(stdout_path)?;
    let stderr = fs::File::create(stderr_path)?;
    configure_process_group(command)?;
    command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;

    let status = loop {
        match output_exceeded(stdout_path, stderr_path, output_limit) {
            Ok(true) => {
                cleanup_process(&mut child, Duration::from_secs(2))?;
                return Err(std::io::Error::other(
                    "oracle subprocess output exceeded its bounded limit",
                ));
            }
            Ok(false) => {}
            Err(error) => {
                let cleanup = cleanup_process(&mut child, Duration::from_secs(2));
                return Err(with_cleanup_error(error, cleanup));
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let cleanup = cleanup_process(&mut child, Duration::from_secs(2));
                return Err(with_cleanup_error(error, cleanup));
            }
        }
        if Instant::now() >= deadline {
            cleanup_process(&mut child, Duration::from_secs(2))?;
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle subprocess exceeded its bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };

    // The leader may have exited while a descendant still owns stdout/stderr.
    // The process group is ours, so clean it before opening either file.
    cleanup_process(&mut child, Duration::from_secs(2))?;
    let stdout = read_capped(stdout_path, output_limit)?;
    let stderr = read_capped(stderr_path, output_limit)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn with_cleanup_error(original: std::io::Error, cleanup: std::io::Result<()>) -> std::io::Error {
    match cleanup {
        Ok(()) => original,
        Err(error) => std::io::Error::other(format!("{original}; cleanup failed: {error}")),
    }
}

fn output_exceeded(stdout: &Path, stderr: &Path, limit: u64) -> std::io::Result<bool> {
    for path in [stdout, stderr] {
        match fs::metadata(path) {
            Ok(metadata) if metadata.len() > limit => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

fn read_capped(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other(
            "oracle subprocess output exceeded its bounded limit",
        ));
    }
    Ok(bytes)
}

fn configure_process_group(command: &mut Command) -> std::io::Result<()> {
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
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = command;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "oracle process-tree cleanup requires a Windows Job Object; taskkill is not used as a safety substitute",
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = command;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "oracle process-tree cleanup unsupported on this platform",
        ))
    }
}

fn cleanup_process(child: &mut Child, timeout: Duration) -> std::io::Result<()> {
    let group_result = kill_process_group(child);
    let direct_result = child.kill();
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
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
    group_result?;
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

fn kill_process_group(child: &Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        let pid =
            i32::try_from(child.id()).map_err(|_| std::io::Error::other("invalid child pid"))?;
        const SIGKILL: i32 = 9;
        if unsafe { kill(-pid, SIGKILL) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(3) {
                return Err(error);
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = child;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "oracle process-tree cleanup requires a Windows Job Object",
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "oracle process-tree cleanup unsupported on this platform",
        ))
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
