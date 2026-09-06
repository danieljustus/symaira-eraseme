#![deny(unsafe_code)]

mod cli;
mod command_surface;
mod commands;

use std::io::Write;

fn main() {
    debug_assert_eq!(commands::all_groups().len(), 31);
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match cli::execute(&args) {
        cli::Outcome::Stdout(bytes) => {
            let _ = std::io::stdout().write_all(&bytes);
        }
        cli::Outcome::Stderr(bytes) => {
            let _ = std::io::stderr().write_all(&bytes);
            std::process::exit(1);
        }
        cli::Outcome::Notice(bytes) => {
            let _ = std::io::stderr().write_all(&bytes);
        }
    }
}
