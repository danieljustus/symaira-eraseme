#![deny(unsafe_op_in_unsafe_fn)]

pub mod case;
pub mod compare;
pub mod filesystem;
pub mod http;
pub mod mcp;
pub mod process;
pub mod sqlite;

use case::{Case, ComparisonMode, Normalizer, Program};
use compare::{compare_case, format_differences};
use std::fs;
use std::path::PathBuf;

fn usage() -> &'static str {
    "usage: parity run --go <path> --rust <path> [--go-arg <arg>] [--rust-arg <arg>]\n\
     [--stdin-file <path>] [--json-semantic] [--sqlite <relative-path>]\n\
     [--sqlite-query <sql>] [--http-response-file <path>] [--no-mcp]\n\
     [--normalize <scope> <reason> <from> <to>]"
}

fn next(args: &[String], index: &mut usize, option: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("{option} requires a value\n{}", usage()))
}

fn build_case(args: &[String]) -> Result<Case, String> {
    if args.first().map(String::as_str) != Some("run") {
        return Err(usage().into());
    }
    let mut go = None;
    let mut rust = None;
    let mut go_args = Vec::new();
    let mut rust_args = Vec::new();
    let mut stdin = Vec::new();
    let mut comparison = ComparisonMode::Exact;
    let mut sqlite_database = None;
    let mut sqlite_queries = Vec::new();
    let mut http_response = None;
    let mut normalizers = Vec::new();
    let mut capture_mcp = true;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--go" => go = Some(PathBuf::from(next(args, &mut index, "--go")?)),
            "--rust" => rust = Some(PathBuf::from(next(args, &mut index, "--rust")?)),
            "--go-arg" => go_args.push(next(args, &mut index, "--go-arg")?),
            "--rust-arg" => rust_args.push(next(args, &mut index, "--rust-arg")?),
            "--stdin-file" => {
                stdin = fs::read(next(args, &mut index, "--stdin-file")?)
                    .map_err(|error| error.to_string())?;
            }
            "--json-semantic" => comparison = ComparisonMode::JsonSemantic,
            "--sqlite" => {
                sqlite_database = Some(PathBuf::from(next(args, &mut index, "--sqlite")?));
            }
            "--sqlite-query" => sqlite_queries.push(next(args, &mut index, "--sqlite-query")?),
            "--http-response-file" => {
                http_response = Some(
                    fs::read(next(args, &mut index, "--http-response-file")?)
                        .map_err(|error| error.to_string())?,
                );
            }
            "--no-mcp" => capture_mcp = false,
            "--normalize" => {
                let scope = next(args, &mut index, "--normalize scope")?;
                let reason = next(args, &mut index, "--normalize reason")?;
                let from = next(args, &mut index, "--normalize from")?;
                let to = next(args, &mut index, "--normalize to")?;
                normalizers.push(Normalizer::exact(
                    scope,
                    reason,
                    from.as_bytes(),
                    to.as_bytes(),
                ));
            }
            option => return Err(format!("unknown option {option}\n{}", usage())),
        }
        index += 1;
    }
    let go = go.ok_or_else(|| format!("missing --go\n{}", usage()))?;
    let rust = rust.ok_or_else(|| format!("missing --rust\n{}", usage()))?;
    let mut case = Case::new(
        "command-line-case",
        Program {
            executable: go,
            argv: go_args,
        },
        Program {
            executable: rust,
            argv: rust_args,
        },
    );
    case.stdin = stdin;
    case.comparison = comparison;
    case.sqlite_database = sqlite_database;
    case.sqlite_queries = sqlite_queries;
    case.http = http_response.map(|response| case::HttpFixture { response });
    case.capture_mcp = capture_mcp;
    case.normalizers = normalizers;
    Ok(case)
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let case = match build_case(&args) {
        Ok(case) => case,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    match compare_case(&case) {
        Ok(Ok(())) => {
            println!("parity case passed: {}", case.id);
        }
        Ok(Err(differences)) => {
            eprintln!(
                "parity case failed: {}\n{}",
                case.id,
                format_differences(&differences)
            );
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("parity case could not run: {error}");
            std::process::exit(2);
        }
    }
}
