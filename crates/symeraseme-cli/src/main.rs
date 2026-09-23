#![deny(unsafe_code)]

mod cli;
mod command_surface;
mod commands;
#[allow(dead_code)]
mod mcp;

use std::io::Write;

fn main() {
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
