#![deny(unsafe_code)]

mod cli;
mod command_surface;
mod commands;
#[allow(dead_code)]
mod mcp;
mod store;

use std::io::Write;
use std::process::{Command, Stdio};

fn main() {
    if let Err(error) = run_go_backend_if_requested() {
        let _ = writeln!(std::io::stderr(), "symeraseme-rust: {error}");
        std::process::exit(1);
    }

    debug_assert_eq!(commands::all_groups().len(), 31);
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    // The stdio server needs the process pipes, so its outcome is produced
    // before the common report path.
    let outcome = match cli::execute(&args) {
        cli::Outcome::ServeStdio(notice) => cli::serve_stdio(notice),
        cli::Outcome::ServeHttp {
            host,
            port,
            allow_remote,
            notice,
        } => cli::serve_http(host, port, allow_remote, notice),
        other => other,
    };
    match outcome {
        cli::Outcome::Stdout(bytes) => {
            let _ = std::io::stdout().write_all(&bytes);
        }
        cli::Outcome::Stderr(bytes) => {
            let _ = std::io::stderr().write_all(&bytes);
            std::process::exit(1);
        }
        cli::Outcome::StdoutStderr(out, err) => {
            let _ = std::io::stdout().write_all(&out);
            let _ = std::io::stderr().write_all(&err);
            std::process::exit(1);
        }
        // Handled above; a nested ServeStdio would mean serve_stdio returned one.
        cli::Outcome::ServeStdio(_) => unreachable!("serve_stdio does not return ServeStdio"),
        cli::Outcome::ServeHttp { .. } => unreachable!("serve_http does not return ServeHttp"),
    }
}

fn run_go_backend_if_requested() -> std::io::Result<()> {
    match std::env::var("SYMERASEME_BACKEND").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("rust") => return Ok(()),
        Ok("go") => {}
        Ok(value) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported SYMERASEME_BACKEND {value:?}; expected \"rust\" or \"go\""),
            ));
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "SYMERASEME_BACKEND is not valid Unicode",
            ));
        }
    }

    let executable = std::env::current_exe()?;
    let go_name = if cfg!(windows) {
        "symeraseme-go.exe"
    } else {
        "symeraseme-go"
    };
    let go_binary = executable
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(go_name);
    if !go_binary.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Go fallback is missing: {}", go_binary.display()),
        ));
    }

    let mut command = Command::new(&go_binary);
    command
        .args(std::env::args_os().skip(1))
        .env_remove("SYMERASEME_BACKEND")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec())
    }

    #[cfg(windows)]
    {
        let status = command.status()?;
        std::process::exit(status.code().unwrap_or(1));
    }

    #[cfg(not(any(unix, windows)))]
    {
        let status = command.status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
}
