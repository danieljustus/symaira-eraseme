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

pub(crate) fn run(
    command: &mut Command,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
    output_limit: u64,
) -> std::io::Result<Output> {
    let stdout = fs::File::create(stdout_path)?;
    let stderr = fs::File::create(stderr_path)?;
    configure_process_group(command);
    command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if output_exceeded(stdout_path, stderr_path, output_limit)? {
            terminate_bounded(&mut child, Duration::from_secs(2))?;
            return Err(std::io::Error::other(
                "oracle subprocess output exceeded its bounded limit",
            ));
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            terminate_bounded(&mut child, Duration::from_secs(2))?;
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle subprocess exceeded its bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = read_capped(stdout_path, output_limit)?;
    let stderr = read_capped(stderr_path, output_limit)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
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
    file.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other(
            "oracle subprocess output exceeded its bounded limit",
        ));
    }
    Ok(bytes)
}

fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0000_0200);
    }
}

fn terminate_bounded(child: &mut Child, timeout: Duration) -> std::io::Result<()> {
    let tree_result = kill_process_tree(child);
    let direct_result = child.kill();
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            if let Err(error) = tree_result {
                return Err(std::io::Error::other(format!(
                    "oracle process-tree cleanup failed: {error}"
                )));
            }
            if let Err(error) = direct_result {
                let already_exited = matches!(
                    error.kind(),
                    std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
                ) || error.raw_os_error() == Some(3);
                if !already_exited {
                    return Err(std::io::Error::other(format!(
                        "oracle direct-child cleanup failed: {error}"
                    )));
                }
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle subprocess cleanup exceeded its bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn kill_process_tree(child: &Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let group = format!("-{}", child.id());
        let status = Command::new("kill").args(["-KILL", &group]).status()?;
        if status.success() || status.code() == Some(1) {
            return Ok(());
        }
        return Err(std::io::Error::other(format!(
            "kill process group exited {status}"
        )));
    }
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if status.success() || status.code() == Some(128) {
            return Ok(());
        }
        return Err(std::io::Error::other(format!(
            "taskkill process tree exited {status}"
        )));
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
