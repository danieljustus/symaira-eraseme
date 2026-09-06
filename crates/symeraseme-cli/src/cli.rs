//! Clap-compatible command surface, Cobra-compatible help, and handlers.

use crate::command_surface::{self, CommandSpec, FlagSpec};
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;

use symeraseme_core::config::{Config, ConfigContext};
use symeraseme_core::version;

const ROOT_NAME: &str = "symeraseme";

/// A parser/handler result suitable for the process boundary.
#[derive(Debug)]
pub enum Outcome {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Notice(Vec<u8>),
}

/// Build the complete native Clap surface for debug assertions and tooling.
/// Runtime compatibility rendering is intentionally kept separate because Clap
/// and Cobra format help and parse errors differently.
pub fn clap_surface() -> clap::Command {
    let specs = command_surface::all();
    debug_assert_eq!(crate::commands::all_groups().len(), 31);
    build_command(&specs, &[])
}

fn build_command(specs: &[CommandSpec], path: &[&str]) -> clap::Command {
    let spec = command_surface::find(specs, path).expect("command surface path");
    let name = if path.is_empty() {
        ROOT_NAME
    } else {
        spec.path
            .rsplit(' ')
            .next()
            .expect("non-empty command path")
    };
    let mut command = clap::Command::new(name)
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .disable_version_flag(true)
        .subcommand_precedence_over_arg(true)
        .hide(spec.hidden);
    if !spec.short.is_empty() {
        command = command.about(spec.short);
    }
    for flag in spec.flags {
        if flag.name == "output" {
            continue;
        }
        command = command.arg(clap_arg(flag, None));
    }
    if path.is_empty() {
        command = command.arg(
            clap::Arg::new("output")
                .long("output")
                .global(true)
                .value_name("string")
                .default_value("text")
                .help("output format: text or json"),
        );
    }
    command = command.arg(
        clap::Arg::new("help")
            .short('h')
            .long("help")
            .action(clap::ArgAction::SetTrue)
            .help(format!("help for {name}")),
    );
    for (index, positional) in spec.positionals.iter().enumerate() {
        command = command.arg(
            clap::Arg::new(["arg0", "arg1"][index])
                .index(index + 1)
                .value_name(*positional)
                .required(false),
        );
    }
    for child in command_surface::children(specs, path) {
        let child_path = child.path.split_whitespace().collect::<Vec<_>>();
        command = command.subcommand(build_command(specs, &child_path));
    }
    command
}

fn clap_arg(flag: &FlagSpec, _shorthand: Option<char>) -> clap::Arg {
    let mut arg = clap::Arg::new(flag.name)
        .long(flag.name)
        .help(flag.usage)
        .action(if flag.kind == "bool" {
            clap::ArgAction::SetTrue
        } else {
            clap::ArgAction::Set
        });
    if flag.name == "version" {
        arg = arg.short('v');
    }
    if flag.kind != "bool" {
        arg = arg.value_name(flag.kind);
    }
    arg
}

/// Parse, render, and dispatch one argv vector without global mutable state.
pub fn execute(args: &[String]) -> Outcome {
    let specs = command_surface::all();
    if matches!(args.first().map(String::as_str), Some("--version" | "-v")) {
        return Outcome::Stdout(
            format!("{ROOT_NAME} version {}\n", version::BUILD_VERSION).into_bytes(),
        );
    }
    let argv = std::iter::once(ROOT_NAME.to_owned()).chain(args.iter().cloned());
    let matches = match clap_surface().try_get_matches_from(argv) {
        Ok(matches) => matches,
        Err(error) => {
            if let Err(message) = parse(&specs, args) {
                return Outcome::Stderr(message.into_bytes());
            }
            return parse_error(error, args);
        }
    };
    if matches.get_flag("version") {
        return Outcome::Stdout(
            format!("{ROOT_NAME} version {}\n", version::BUILD_VERSION).into_bytes(),
        );
    }
    match parse(&specs, args) {
        Ok(parsed) if parsed.help => {
            Outcome::Stdout(render_help(&specs, &parsed.help_path).into_bytes())
        }
        Ok(parsed) => dispatch(&specs, &parsed),
        Err(message) => Outcome::Stderr(message.into_bytes()),
    }
}

fn parse_error(error: clap::Error, args: &[String]) -> Outcome {
    use clap::error::ErrorKind;
    match error.kind() {
        ErrorKind::UnknownArgument => {
            if args.first().is_some_and(|arg| arg == "-V") {
                return Outcome::Stderr(b"unknown shorthand flag: 'V' in -V\n".to_vec());
            }
            let flag = args
                .iter()
                .find(|arg| arg.starts_with("--"))
                .map(String::as_str)
                .unwrap_or("--unknown");
            let flag = flag.split('=').next().unwrap_or(flag);
            Outcome::Stderr(format!("unknown flag: {flag}\n").into_bytes())
        }
        ErrorKind::InvalidSubcommand => {
            let command = args
                .iter()
                .find(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .unwrap_or("");
            Outcome::Stderr(
                format!("unknown command \"{command}\" for \"{ROOT_NAME}\"\n").into_bytes(),
            )
        }
        _ => Outcome::Stderr(error.to_string().into_bytes()),
    }
}

struct Parsed {
    path: Vec<String>,
    positional: Vec<String>,
    flags: BTreeMap<String, String>,
    help: bool,
    help_path: Vec<String>,
}

fn parse(specs: &[CommandSpec], args: &[String]) -> Result<Parsed, String> {
    let mut path: Vec<String> = Vec::new();
    let mut positional = Vec::new();
    let mut flags = BTreeMap::new();
    let mut help = false;
    let mut help_path = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let raw = &args[index];
        if raw == "--help" || raw == "-h" {
            help = true;
            help_path = path.clone();
            index += 1;
            continue;
        }
        if raw == "--version" || raw == "-v" {
            if path.is_empty() {
                return Ok(Parsed {
                    path,
                    positional,
                    flags,
                    help: false,
                    help_path,
                });
            }
            return Err("unknown flag: --version\n".to_owned());
        }
        if raw.starts_with('-') && !raw.starts_with("--") {
            let shorthand = raw.trim_start_matches('-').chars().next().unwrap_or('?');
            return Err(format!("unknown shorthand flag: '{shorthand}' in {raw}\n"));
        }
        if raw.starts_with('-') {
            let (name, inline_value) = split_flag(raw);
            let known = known_flag(specs, &path, name);
            let Some(flag) = known else {
                return Err(format!("unknown flag: --{name}\n"));
            };
            if flag.kind == "bool" {
                let value = inline_value.unwrap_or_else(|| "true".to_owned());
                if value != "true" && value != "false" {
                    return Err(format!("invalid argument '{value}' for '--{name}'\n"));
                }
                flags.insert(name.to_owned(), value);
            } else {
                let value = if let Some(value) = inline_value {
                    value
                } else {
                    index += 1;
                    args.get(index)
                        .filter(|value| !value.starts_with('-'))
                        .cloned()
                        .ok_or_else(|| format!("flag needs an argument: --{name}\n"))?
                };
                flags.insert(name.to_owned(), value);
            }
            index += 1;
            continue;
        }

        let parts = path.iter().map(String::as_str).collect::<Vec<_>>();
        if let Some(child) = command_surface::children(specs, &parts)
            .into_iter()
            .find(|child| child.path.split_whitespace().last() == Some(raw.as_str()))
        {
            path = child.path.split_whitespace().map(str::to_owned).collect();
        } else {
            let current = command_surface::find(specs, &parts)
                .ok_or_else(|| format!("unknown command \"{raw}\" for \"{ROOT_NAME}\"\n"))?;
            if current.positionals.len() > positional.len() {
                positional.push(raw.clone());
            } else {
                let parent = if path.is_empty() {
                    ROOT_NAME.to_owned()
                } else {
                    format!("{ROOT_NAME} {}", path.join(" "))
                };
                return Err(format!("unknown command \"{raw}\" for \"{parent}\"\n"));
            }
        }
        index += 1;
    }

    if path == ["help"] && !help {
        path = positional.clone();
        positional.clear();
        help_path = path.clone();
        help = true;
    }
    let parts = path.iter().map(String::as_str).collect::<Vec<_>>();
    let current = command_surface::find(specs, &parts).ok_or_else(|| {
        format!(
            "unknown command \"{}\" for \"{ROOT_NAME}\"\n",
            path.last().unwrap_or(&String::new())
        )
    })?;
    let required = if current.args_required {
        current.positionals.len()
    } else {
        0
    };
    if !help && positional.len() < required {
        return Err(format!(
            "accepts {required} arg(s), received {}\n",
            positional.len()
        ));
    }
    if help_path.is_empty() && help {
        help_path = path.clone();
    }
    Ok(Parsed {
        path,
        positional,
        flags,
        help,
        help_path,
    })
}

fn split_flag(raw: &str) -> (&str, Option<String>) {
    let raw = raw.strip_prefix("--").unwrap_or(raw);
    if let Some((name, value)) = raw.split_once('=') {
        (name, Some(value.to_owned()))
    } else {
        (raw, None)
    }
}

fn known_flag<'a>(specs: &'a [CommandSpec], path: &[String], name: &str) -> Option<&'a FlagSpec> {
    let name = name.strip_prefix("--").unwrap_or(name);
    if name == "output" {
        let parts = path.iter().map(String::as_str).collect::<Vec<_>>();
        let current = command_surface::find(specs, &parts)?;
        if current.flags.iter().any(|flag| flag.name == "output") {
            return current.flags.iter().find(|flag| flag.name == "output");
        }
        return Some(&GLOBAL_OUTPUT);
    }
    let parts = path.iter().map(String::as_str).collect::<Vec<_>>();
    let current = command_surface::find(specs, &parts)?;
    if name == "h" {
        return Some(&HELP_FLAG);
    }
    current.flags.iter().find(|flag| flag.name == name)
}

static HELP_FLAG: FlagSpec = FlagSpec {
    name: "help",
    kind: "bool",
    default: "false",
    usage: "",
};
static GLOBAL_OUTPUT: FlagSpec = FlagSpec {
    name: "output",
    kind: "string",
    default: "text",
    usage: "output format: text or json",
};

fn dispatch(_specs: &[CommandSpec], parsed: &Parsed) -> Outcome {
    let path = parsed.path.join(" ");
    match path.as_str() {
        "version" => {
            if parsed
                .flags
                .get("json")
                .is_some_and(|value| value == "true")
            {
                match version::current().json_line() {
                    Ok(bytes) => Outcome::Stdout(bytes),
                    Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
                }
            } else {
                Outcome::Stdout(format!("{}\n", version::current().text()).into_bytes())
            }
        }
        "config show" => config_show(parsed),
        "completion" => completion(parsed.positional.first().map(String::as_str).unwrap_or("")),
        "serve" if parsed.flags.get("stdio").is_some_and(|value| value == "true") => {
            Outcome::Notice(
                b"symeraseme serve is deprecated and will be removed. Please use symeraseme mcp instead.\n"
                    .to_vec(),
            )
        }
        _ => deferred(&path),
    }
}

fn deferred(path: &str) -> Outcome {
    Outcome::Stderr(format!("deferred command: {path} is not implemented in Rust\n").into_bytes())
}

#[derive(Serialize)]
struct ConfigEnvelope<'a> {
    config: &'a Config,
    success: bool,
}

fn config_show(parsed: &Parsed) -> Outcome {
    let home = env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let cwd = env::current_dir().unwrap_or_default();
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    let context = ConfigContext::new(home, cwd, environment);
    let config = match Config::load(&context) {
        Ok(config) => config,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    if parsed
        .flags
        .get("output")
        .is_some_and(|value| value == "json")
    {
        match serde_json::to_vec(&ConfigEnvelope {
            config: &config,
            success: true,
        }) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                Outcome::Stdout(bytes)
            }
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        }
    } else {
        Outcome::Stdout(
            format!("data_dir={}\nport={}\n", config.data_dir, config.port).into_bytes(),
        )
    }
}

fn completion(shell: &str) -> Outcome {
    let body = match shell {
        "bash" => include_str!("commands/completion/bash.txt"),
        "zsh" => include_str!("commands/completion/zsh.txt"),
        "fish" => include_str!("commands/completion/fish.txt"),
        "powershell" => include_str!("commands/completion/powershell.txt"),
        _ => {
            return Outcome::Stderr(
                format!("unsupported completion shell: {shell}\n").into_bytes(),
            );
        }
    };
    Outcome::Stdout(body.as_bytes().to_vec())
}

fn render_help(specs: &[CommandSpec], path: &[String]) -> String {
    let path_refs = path.iter().map(String::as_str).collect::<Vec<_>>();
    let spec = command_surface::find(specs, &path_refs)
        .or_else(|| command_surface::find(specs, &[]))
        .expect("root command");
    let children = command_surface::children(specs, &path_refs)
        .into_iter()
        .filter(|child| !child.hidden)
        .collect::<Vec<_>>();
    let mut out = String::new();
    let heading = if spec.long.is_empty() {
        spec.short
    } else {
        spec.long
    };
    if !heading.is_empty() {
        out.push_str(heading);
        out.push_str("\n\n");
    }
    out.push_str("Usage:\n  ");
    if path.is_empty() {
        out.push_str(ROOT_NAME);
        out.push_str(" [command]");
    } else {
        out.push_str(ROOT_NAME);
        out.push(' ');
        if path.len() > 1 {
            out.push_str(&path[..path.len() - 1].join(" "));
            out.push(' ');
        }
        out.push_str(spec.use_line);
        if !spec.use_line.contains("[flags]") {
            if children.is_empty() {
                out.push_str(" [flags]");
            } else {
                out.push_str(" [command]");
            }
        }
    }
    out.push_str("\n\n");
    if !children.is_empty() {
        out.push_str("Available Commands:\n");
        let max_len = children
            .iter()
            .map(|child| child.path.rsplit(' ').next().unwrap_or("").len())
            .max()
            .unwrap_or(0);
        let command_column = (max_len + 3).max(14);
        for child in &children {
            let name = child.path.rsplit(' ').next().unwrap_or("");
            let desc = child.short;
            out.push_str("  ");
            out.push_str(name);
            let padding = command_column.saturating_sub(2 + name.len());
            out.extend(std::iter::repeat_n(' ', padding));
            out.push_str(desc);
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str("Flags:\n");
    let local = local_flags(spec);
    out.push_str(&render_flags(
        &local,
        path.last().map(String::as_str).unwrap_or(ROOT_NAME),
    ));
    if has_global_output(spec, path) {
        out.push_str("\nGlobal Flags:\n");
        out.push_str(&render_flags(
            &[FlagDisplay {
                flag: &GLOBAL_OUTPUT,
                shorthand: false,
            }],
            path.last().map(String::as_str).unwrap_or(ROOT_NAME),
        ));
    }
    if !children.is_empty() {
        let use_text = if path.is_empty() {
            "[command]".to_owned()
        } else {
            format!("{} [command]", path.join(" "))
        };
        out.push_str(&format!(
            "\nUse \"{ROOT_NAME} {use_text} --help\" for more information about a command.\n"
        ));
    }
    out
}

fn local_flags(spec: &CommandSpec) -> Vec<FlagDisplay<'_>> {
    let mut flags = spec
        .flags
        .iter()
        .map(|flag| FlagDisplay {
            flag,
            shorthand: flag.name == "version",
        })
        .collect::<Vec<_>>();
    flags.push(FlagDisplay {
        flag: &HELP_FLAG_FOR_RENDER,
        shorthand: true,
    });
    if spec.path.is_empty() {
        flags.push(FlagDisplay {
            flag: &GLOBAL_OUTPUT,
            shorthand: false,
        });
    }
    flags.sort_by(|a, b| a.flag.name.cmp(b.flag.name));
    flags
}
static HELP_FLAG_FOR_RENDER: FlagSpec = FlagSpec {
    name: "help",
    kind: "bool",
    default: "false",
    usage: "",
};

fn has_global_output(spec: &CommandSpec, path: &[String]) -> bool {
    !(path.is_empty() || spec.flags.iter().any(|flag| flag.name == "output"))
}

struct FlagDisplay<'a> {
    flag: &'a FlagSpec,
    shorthand: bool,
}
fn render_flags(flags: &[FlagDisplay<'_>], command_name: &str) -> String {
    let mut labels = flags
        .iter()
        .map(|item| flag_label(item))
        .collect::<Vec<_>>();
    let max = labels.iter().map(String::len).max().unwrap_or(0);
    let mut out = String::new();
    for (index, item) in flags.iter().enumerate() {
        let label = std::mem::take(&mut labels[index]);
        out.push_str(&label);
        out.extend(std::iter::repeat_n(' ', max + 3 - label.len()));
        let usage = if item.flag.name == "help" {
            "help for".to_owned()
        } else {
            item.flag.usage.to_owned()
        };
        let usage = if item.flag.name == "help" {
            format!("help for {command_name}")
        } else {
            usage
        };
        // The caller patches the command-specific help wording through the
        // stable flag usage field below; this branch only handles formatting.
        out.push_str(&usage);
        let default = default_suffix(item.flag);
        if !default.is_empty() {
            out.push_str(&default);
        }
        out.push('\n');
    }
    out
}
fn flag_label(item: &FlagDisplay<'_>) -> String {
    let prefix = if item.shorthand {
        if item.flag.name == "version" {
            "  -v, --"
        } else {
            "  -h, --"
        }
    } else {
        "      --"
    };
    let mut label = format!("{}{}", prefix, item.flag.name);
    if item.flag.kind != "bool" {
        label.push(' ');
        label.push_str(item.flag.kind);
    }
    label
}
fn default_suffix(flag: &FlagSpec) -> String {
    match (flag.kind, flag.default) {
        ("bool", "true") => " (default true)".to_owned(),
        ("int", value) if value != "0" => format!(" (default {value})"),
        ("string", value) if !value.is_empty() => format!(" (default \"{value}\")"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::clap_surface;

    fn count_commands(command: &clap::Command) -> usize {
        1 + command.get_subcommands().map(count_commands).sum::<usize>()
    }

    #[test]
    fn actual_clap_tree_matches_frozen_inventory() {
        let command = clap_surface();
        assert_eq!(count_commands(&command), 51);
        assert_eq!(command.get_subcommands().count(), 31);
        let serve = command
            .get_subcommands()
            .find(|child| child.get_name() == "serve")
            .expect("hidden serve compatibility command");
        assert!(serve.is_hide_set());
    }
}
