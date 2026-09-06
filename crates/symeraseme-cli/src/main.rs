#![deny(unsafe_code)]

use clap::{Args, Parser, Subcommand};
use symeraseme_core::version;

#[derive(Debug, Parser)]
#[command(
    name = "symeraseme",
    bin_name = "symeraseme",
    version = concat!("version ", env!("SYMERASEME_VERSION")),
    about = "Automated data broker removal tool",
    subcommand_required = true,
    arg_required_else_help = false
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the installed Symaira EraseMe version.
    Version(VersionArgs),
}

#[derive(Debug, Args)]
struct VersionArgs {
    /// Print the versionkit handshake payload as compact JSON.
    #[arg(long)]
    json: bool,
}

fn main() {
    reject_root_version_trailing_arguments();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = if error.exit_code() == 0 { 0 } else { 1 };
            let _ = error.print();
            std::process::exit(exit_code);
        }
    };
    if let Err(error) = execute(cli) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn reject_root_version_trailing_arguments() {
    let mut args = std::env::args_os().skip(1);
    let Some(first) = args.next() else {
        return;
    };
    if (first == "--version" || first == "-V") && args.next().is_some() {
        eprintln!("unexpected argument after --version");
        std::process::exit(1);
    }
}

fn execute(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Version(args) => {
            let info = version::current();
            if args.json {
                let bytes = info.json_line()?;
                print_bytes(&bytes)?;
            } else {
                println!("{}", info.text());
            }
        }
    }
    Ok(())
}

fn print_bytes(bytes: &[u8]) -> Result<(), std::io::Error> {
    use std::io::Write;

    std::io::stdout().write_all(bytes)
}
