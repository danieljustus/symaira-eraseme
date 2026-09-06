//! Subprocess isolation and raw byte capture.

use crate::case::{Case, Program};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
    pub sandbox: PathBuf,
    pub cwd: PathBuf,
    pub home: PathBuf,
    pub xdg: PathBuf,
}

fn unique_dir(label: &str) -> std::io::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "symeraseme-parity-{label}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn create_layout(root: &Path, case: &Case) -> std::io::Result<PathBuf> {
    let cwd = root.join("cwd");
    fs::create_dir_all(&cwd)?;
    for fixture in &case.cwd.files {
        let path = cwd.join(&fixture.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &fixture.contents)?;
        if let Some(mode) = fixture.mode {
            set_mode(&path, mode)?;
        }
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

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe extern "C" {
        fn setpgid(pid: i32, pgid: i32) -> i32;
    }
    unsafe {
        command.pre_exec(|| {
            // A child calling setpgid(0, 0) makes itself the group leader. The
            // parent can then terminate descendants with kill(-pid, SIGKILL).
            let result = setpgid(0, 0);
            if result == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

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

pub fn run_program(case: &Case, program: &Program) -> std::io::Result<RunResult> {
    reject_reserved_environment(case)?;
    let sandbox = unique_dir(&case.id)?;
    let cwd = create_layout(&sandbox, case)?;
    let home = sandbox.join("home");
    let xdg = sandbox.join("xdg");
    let tmp = sandbox.join("tmp");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&xdg)?;
    fs::create_dir_all(&tmp)?;

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
    for (name, value) in &case.environment.values {
        command.env(name, value);
    }
    configure_process_group(&mut command);
    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&case.stdin)?;
    }
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let stdout_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = stdout;
        reader.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = stderr;
        reader.read_to_end(&mut bytes).map(|_| bytes)
    });

    let deadline = Instant::now() + case.timeout;
    let mut timed_out = false;
    let status = loop {
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
    let stdout = stdout_thread
        .join()
        .map_err(|_| std::io::Error::other("stdout reader panicked"))??;
    let stderr = stderr_thread
        .join()
        .map_err(|_| std::io::Error::other("stderr reader panicked"))??;
    Ok(RunResult {
        status: raw_status(&status, timed_out),
        stdout,
        stderr,
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
    fn isolates_home_and_cwd_and_captures_raw_bytes() {
        let case = Case::new("isolation", dummy("same"), dummy("same"));
        let result = run_program(&case, &case.go).expect("dummy program runs");
        assert_eq!(result.stdout, b"same");
        assert_ne!(result.cwd, std::env::current_dir().unwrap());
        assert!(result.home.starts_with(&result.sandbox));
        assert_eq!(result.status.exit_code, Some(0));
        let _ = fs::remove_dir_all(result.sandbox);
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
        let _ = fs::remove_dir_all(result.sandbox);
    }
}
