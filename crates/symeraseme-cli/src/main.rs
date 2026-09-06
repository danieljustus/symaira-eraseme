#![deny(unsafe_code)]

use clap::{ArgAction, Args, Parser, Subcommand};
use symeraseme_core::version;

#[derive(Debug, Parser)]
#[command(
    name = "symeraseme",
    bin_name = "symeraseme",
    version = concat!("version ", env!("CARGO_PKG_VERSION")),
    disable_version_flag = true,
    about = "Automated data broker removal tool",
    subcommand_required = true,
    arg_required_else_help = false
)]
struct Cli {
    #[arg(
        short = 'v',
        long = "version",
        action = ArgAction::SetTrue,
        required = false
    )]
    version_flag: bool,
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
    handle_version_compatibility_errors();
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

fn handle_version_compatibility_errors() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .filter_map(|arg| arg.into_string().ok())
        .collect();

    if args
        .first()
        .is_some_and(|arg| arg == "--version" || arg == "-v")
    {
        println!("symeraseme version {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    if args.first().is_some_and(|arg| arg == "-V") {
        eprintln!("unknown shorthand flag: 'V' in -V");
        std::process::exit(1);
    }

    let version_extra = (args.len() == 2 && args[0] == "version" && args[1] == "extra")
        || (args.len() == 3 && args[0] == "version" && args[1] == "--json" && args[2] == "extra");
    if version_extra {
        eprintln!("unknown command \"extra\" for \"symeraseme version\"");
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
