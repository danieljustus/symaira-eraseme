//! Clap-compatible command surface, Cobra-compatible help, and handlers.

use crate::command_surface::{self, CommandSpec, FlagSpec};
use crate::mcp::handler::{ContractHandler, ToolHandler};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::env;
use std::path::Path;

use chrono::{DateTime, Utc};
use symeraseme_core::campaign;
use symeraseme_core::config::{Config, ConfigContext, resolve_storage};
use symeraseme_core::deadlines::{self, RunOpts};
use symeraseme_core::identity::{
    ConsentOptions, ConsentStore, MasterKeyResolver, Profile, ProfileError, ProfilePaths,
    init_profile, load_profile, profile_exists,
};
use symeraseme_core::jsonorder::go_map_order;
use symeraseme_core::registry::{
    Broker, BrokerFilter, filter_brokers, load_embedded, load_from_dir,
};
use symeraseme_core::reporting;
use symeraseme_core::storage::Store;
use symeraseme_core::templating::{Address, RenderContext, list_template_names, render};
use symeraseme_core::version;
use symeraseme_engine::scheduler::install::{self, InstallOptions};
use symeraseme_engine::scheduler::{Config as SchedulerConfig, Platform};

const ROOT_NAME: &str = "symeraseme";

/// A parser/handler result suitable for the process boundary.
#[derive(Debug)]
pub enum Outcome {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    /// Bytes for stdout followed by a Go `RunE` error on stderr: the process
    /// prints the action result first, then the error, and exits 1 — Go's
    /// `writeJSON` / `writeWebActionText` followed by `webActionError`.
    StdoutStderr(Vec<u8>, Vec<u8>),
    /// Run the stdio MCP server: optional stderr notice first, then the
    /// JSON-RPC loop over stdin/stdout until EOF.
    ServeStdio(Option<Vec<u8>>),
    /// Run the token-authenticated MCP HTTP server.
    ServeHttp {
        host: String,
        port: i64,
        allow_remote: bool,
        notice: Option<Vec<u8>>,
    },
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
    if spec.path == "brokers list" {
        command = command.arg(
            clap::Arg::new("trailing")
                .index(1)
                .num_args(0..)
                .value_name("ARG")
                .hide(true)
                .trailing_var_arg(true),
        );
    }
    // Cobra migration flags use the last supplied value.
    if spec.path == "migrate" {
        command = command.args_override_self(true);
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
        .action(clap::ArgAction::Set);
    if flag.kind == "bool" {
        arg = arg
            .value_parser(clap::value_parser!(bool))
            .default_value("false")
            .default_missing_value("true")
            .num_args(0..=1)
            .require_equals(true);
    }
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
    if matches.get_one::<bool>("version").copied().unwrap_or(false) {
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
    /// `--output` seen before any subcommand: Go binds it to the root
    /// persistent flag (the display format). `--output` after a subcommand
    /// belongs to a shadowing local file flag and never reaches the format.
    root_output: Option<String>,
    /// `--output` seen after a subcommand: the local file flag of
    /// `generate-dashboard` / `generate-report`.
    local_output: Option<String>,
}

fn parse(specs: &[CommandSpec], args: &[String]) -> Result<Parsed, String> {
    let mut path: Vec<String> = Vec::new();
    let mut positional = Vec::new();
    let mut flags = BTreeMap::new();
    let mut help = false;
    let mut help_path = Vec::new();
    let mut root_output: Option<String> = None;
    let mut local_output: Option<String> = None;
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
                    root_output: None,
                    local_output: None,
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
                if path == ["poll-inbox"]
                    && matches!(name, "port" | "since" | "since-days")
                    && let Err(error) = value.parse::<i64>()
                {
                    let detail = if matches!(
                        error.kind(),
                        std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow
                    ) {
                        "value out of range"
                    } else {
                        "invalid syntax"
                    };
                    return Err(format!(
                        "invalid argument {value:?} for \"--{name}\" flag: strconv.ParseInt: parsing {value:?}: {detail}\n"
                    ));
                }
                if name == "output" {
                    if path.is_empty() {
                        root_output = Some(value.clone());
                    } else {
                        local_output = Some(value.clone());
                    }
                }
                if path == ["poll-inbox"] && matches!(name, "since" | "since-days") {
                    flags.insert("since".to_owned(), value.clone());
                    flags.insert("since-days".to_owned(), value);
                } else {
                    flags.insert(name.to_owned(), value);
                }
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
            if current.path == "brokers list" || current.positionals.len() > positional.len() {
                positional.push(raw.clone());
            } else if current.path == "brokers show" {
                return Err(format!(
                    "accepts 1 arg(s), received {}\n",
                    positional.len() + 1
                ));
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
        root_output,
        local_output,
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
                match version::json_line(&version::current()) {
                    Ok(bytes) => Outcome::Stdout(bytes),
                    Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
                }
            } else {
                Outcome::Stdout(format!("{}\n", version::text(&version::current())).into_bytes())
            }
        }
        "config show" => config_show(parsed),
        "brokers list" => brokers_list(parsed),
        "brokers show" => brokers_show(parsed),
        "registry list" => registry_list(parsed),
        "registry validate" => registry_validate(parsed),
        "completion" => completion(parsed.positional.first().map(String::as_str).unwrap_or("")),
        "render-template" => render_template(parsed),
        "init-profile" => init_profile_command(parsed),
        "show-profile" => show_profile_command(parsed),
        "serve"
            if parsed
                .flags
                .get("stdio")
                .is_some_and(|value| value == "true") =>
        {
            // Go prints the banner and then serves; only the banner was
            // reproduced before, which made `serve` exit instead of serving.
            Outcome::ServeStdio(Some(
                b"symeraseme serve is deprecated and will be removed. Please use symeraseme mcp instead.\n"
                    .to_vec(),
            ))
        }
        "serve" => Outcome::ServeHttp {
            host: string_flag_or(parsed, "host", "127.0.0.1"),
            port: int_flag(parsed, "port", 8000),
            allow_remote: bool_flag(parsed, "allow-remote"),
            notice: Some(
                b"symeraseme serve is deprecated and will be removed. Please use symeraseme mcp instead.\n"
                    .to_vec(),
            ),
        },
        "mcp"
            if parsed
                .flags
                .get("stdio")
                .is_some_and(|value| value == "true") =>
        {
            Outcome::ServeStdio(None)
        }
        "mcp" => Outcome::ServeHttp {
            host: string_flag_or(parsed, "host", "127.0.0.1"),
            port: int_flag(parsed, "port", 8000),
            allow_remote: bool_flag(parsed, "allow-remote"),
            notice: None,
        },
        "status" => {
            let campaign = parsed.flags.get("campaign").cloned().unwrap_or_default();
            campaign_status(parsed, &campaign)
        }
        "plan create" => plan_create(parsed),
        "plan show" => plan_show(parsed),
        "plan execute" => plan_execute(parsed),
        "plan status" => plan_status(parsed),
        // Go's bare `tick` and `plan tick` are `tickCommandWith` with the same
        // `--dry-run` pointer; the recorded oracle bytes for `operate-tick` and
        // `operate-plan-tick` are identical, so they share one implementation.
        "tick" => plan_tick(parsed),
        "plan tick" => plan_tick(parsed),
        "dashboard" => contract_command("get_dashboard_data", Map::new(), parsed),
        "calendar" => contract_command("get_calendar", calendar_arguments(parsed), parsed),
        "requests list" => contract_command("list_requests", requests_arguments(parsed), parsed),
        "manual-tasks list" => {
            contract_command("manual_tasks_list", manual_tasks_arguments(parsed), parsed)
        }
        "manual-tasks show" => match int_argument(parsed, "task-id", "task ID") {
            Ok(task_id) => {
                let mut arguments = Map::new();
                arguments.insert("task_id".to_owned(), json!(task_id));
                contract_command("manual_tasks_show", arguments, parsed)
            }
            Err(outcome) => outcome,
        },
        "manual-tasks complete" => match int_argument(parsed, "task-id", "task ID") {
            Ok(task_id) => {
                let mut arguments = Map::new();
                arguments.insert("task_id".to_owned(), json!(task_id));
                arguments.insert("notes".to_owned(), json!(string_flag(parsed, "notes")));
                contract_command("manual_tasks_complete", arguments, parsed)
            }
            Err(outcome) => outcome,
        },
        "manual-tasks cleanup" => {
            let mut arguments = Map::new();
            arguments.insert(
                "dry_run".to_owned(),
                json!(
                    parsed
                        .flags
                        .get("dry-run")
                        .is_some_and(|value| value == "true")
                ),
            );
            contract_command("manual_tasks_cleanup", arguments, parsed)
        }
        "events show" => match int_argument(parsed, "request-id", "request ID") {
            Ok(request_id) => contract_command_rendered(
                "get_events",
                events_arguments(parsed, request_id),
                parsed,
            ),
            Err(outcome) => outcome,
        },
        "grant" => contract_command_rendered("grant", grant_arguments(parsed), parsed),
        "poll-inbox" => poll_inbox_command(parsed),
        "generate-dashboard" => generate_dashboard_command(parsed),
        "generate-report" => generate_report_command(parsed),
        "generate-scheduler" => generate_scheduler_command(parsed),
        "schedule install" => schedule_install(parsed),
        "schedule uninstall" => schedule_uninstall(parsed),
        "schedule status" => schedule_status(parsed),
        "review" => review_command(parsed),
        "run-web-form" => run_web_form_command(parsed),
        "auto-confirm" => auto_confirm_command(parsed),
        "migrate" => migrate_command(parsed),
        _ => deferred(&path),
    }
}

fn deferred(path: &str) -> Outcome {
    Outcome::Stderr(format!("deferred command: {path} is not implemented in Rust\n").into_bytes())
}

#[derive(Serialize)]
struct BrokersListFilters {
    include_disabled: bool,
    status: String,
}

#[derive(Serialize)]
struct BrokersListEnvelope<'a> {
    brokers: Vec<&'a Broker>,
    count: usize,
    filters: BrokersListFilters,
    schema_version: u8,
}

#[derive(Serialize)]
struct BrokerShowEnvelope<'a> {
    broker: &'a Broker,
    schema_version: u8,
}

/// Go builds this envelope from a `map[string]any`, whose keys `encoding/json`
/// emits in sorted order: `brokers`, `count`, `schema_version`.
#[derive(Serialize)]
struct RegistryListEnvelope<'a> {
    brokers: Vec<&'a Broker>,
    count: usize,
    schema_version: u8,
}

#[derive(Serialize)]
struct RegistryValidateTotals {
    duplicate_ids: u64,
    failed: u64,
    valid: usize,
}

/// Sorted-key order again: `ok`, `schema_version`, `totals`, and inside `totals`
/// `duplicate_ids`, `failed`, `valid`.
#[derive(Serialize)]
struct RegistryValidateEnvelope {
    ok: bool,
    schema_version: u8,
    totals: RegistryValidateTotals,
}

/// Builds the scheduler options the way Go's `schedule_*` CLI commands do.
///
/// Go passes only `platform` (plus the tick time for `install`) and lets the
/// engine fill the rest from its defaults, so `output_dir` stays empty here and
/// the engine substitutes `./schedules`.
fn schedule_options(parsed: &Parsed) -> InstallOptions<'static> {
    let platform_name = parsed.flags.get("platform").cloned().unwrap_or_default();
    let mut config = SchedulerConfig::default();
    if parsed.path.join(" ") == "schedule install" {
        config.tick_hour = flag_int(parsed, "tick-hour", 10) as i32;
        config.tick_minute = flag_int(parsed, "tick-minute", 0) as i32;
    }
    let mut options = InstallOptions::new(config);
    options.platform_name = Some(platform_name);
    // Go threads `replace_legacy` straight into InstallOptions; it is what
    // grants consent to replace a detected legacy or foreign unit.
    options.replace_legacy = parsed
        .flags
        .get("replace-legacy")
        .is_some_and(|value| value == "true");
    options
}

/// Reads an integer flag, falling back to the declared default. Go's cobra does
/// the same for a flag the caller did not set.
fn flag_int(parsed: &Parsed, name: &str, default: i64) -> i64 {
    parsed
        .flags
        .get(name)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

/// Go's `schedule install`: the dry run renders the files, a real install also
/// writes the wrappers and loads the native units.
fn schedule_install(parsed: &Parsed) -> Outcome {
    let options = schedule_options(parsed);
    let dry_run = parsed
        .flags
        .get("dry-run")
        .is_some_and(|value| value == "true");
    if dry_run {
        // Go's dry run goes through `Generate` on a config whose platform is
        // still the raw string, so the rejection message carries Generate's
        // longer hint. `Config::platform` is empty here, which auto-detects
        // exactly like Go's empty string does.
        let mut config = options.config.clone();
        config.platform = match &options.platform_name {
            Some(name) if !name.is_empty() => match Platform::parse(name) {
                Some(platform) => Some(platform),
                None => {
                    return Outcome::Stderr(
                        format!(
                            "unsupported platform: {} (choose cron, launchd, or systemd)\n",
                            name.to_ascii_lowercase()
                        )
                        .into_bytes(),
                    );
                }
            },
            _ => Some(symeraseme_engine::scheduler::detect_platform()),
        };
        let files = match symeraseme_engine::scheduler::generate(&config) {
            Ok(files) => files,
            Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
        let format = match output_format(parsed) {
            Ok(format) => format,
            Err(outcome) => return outcome,
        };
        if format == "json" {
            // Go writes a `map[string]any`, so the keys come out sorted.
            let payload = go_map_order(json!({
                "success": true,
                "files": files,
                "dry_run": true,
            }));
            return match json_line(&payload) {
                Ok(bytes) => Outcome::Stdout(bytes),
                Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
            };
        }
        return Outcome::Stdout(b"success\n".to_vec());
    }
    if let Err(error) = install::install(&options) {
        return Outcome::Stderr(format!("{error}\n").into_bytes());
    }
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        return match json_line(&json!({"success": true})) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(b"success\n".to_vec())
}

/// Go's `schedule uninstall`.
fn schedule_uninstall(parsed: &Parsed) -> Outcome {
    let options = schedule_options(parsed);
    if let Err(error) = install::uninstall(&options) {
        return Outcome::Stderr(format!("{error}\n").into_bytes());
    }
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        return match json_line(&json!({"success": true})) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(b"success\n".to_vec())
}

/// Go's `schedule status`. The text form prints `success` — Go discards the
/// status payload unless JSON was requested, which is pinned as measured.
fn schedule_status(parsed: &Parsed) -> Outcome {
    let options = schedule_options(parsed);
    let result = match install::status(&options) {
        Ok(result) => result,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        return match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(b"success\n".to_vec())
}

/// The wall clock, as Go's `time.Now().UTC()`. `chrono` is built without its
/// `clock` feature here, so the instant comes from `std`.
fn now_utc() -> DateTime<Utc> {
    DateTime::<Utc>::from(std::time::SystemTime::now())
}

/// The process environment as a [`ConfigContext`], as every command sees it.
fn process_context() -> ConfigContext {
    let home = env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let cwd = env::current_dir().unwrap_or_default();
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    ConfigContext::new(home, cwd, environment)
}

/// Go's `dataStore()`: resolve the storage location, create the database
/// directory with mode `0700`, then open the event store.
fn open_store() -> Result<Store, String> {
    let storage = resolve_storage(&process_context()).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&storage.db_dir)
        .map_err(|error| format!("eventstore: create database directory: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&storage.db_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("eventstore: secure database directory: {error}"))?;
    }
    Store::open(&storage.db_path).map_err(|error| error.to_string())
}

/// Go's `%v` rendering of a string-keyed map: sorted keys, plain values.
fn go_value(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return value.to_string();
    };
    let entries = object
        .iter()
        .map(|(key, value)| format!("{key}:{value}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("map[{entries}]")
}

/// Go's `identityHashForPlanning`: the default profile's hash, an empty hash
/// when no profile exists, and the load failure otherwise.
///
/// `--profile` selects the file; `ProfilePaths::resolve` applies the platform
/// default for an empty path, as Go's `DefaultProfilePath` does.
fn planning_profile(parsed: &Parsed) -> Result<Option<Profile>, String> {
    let requested = string_flag(parsed, "profile");
    let paths = ProfilePaths::from_process();
    let mut keys = MasterKeyResolver::from_process();
    match load_profile(Path::new(&requested), &paths, &mut keys) {
        Ok(profile) => Ok(Some(profile)),
        Err(ProfileError::NotFound) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// `plan create` — Go's `realPlanCommand`'s `create`.
fn plan_create(parsed: &Parsed) -> Outcome {
    let campaign_id = string_flag(parsed, "campaign");
    if campaign_id.is_empty() {
        return Outcome::Stderr(b"--campaign is required\n".to_vec());
    }
    let identity_hash = match planning_profile(parsed) {
        Ok(profile) => profile
            .as_ref()
            .map(symeraseme_core::identity::hash_profile)
            .unwrap_or_default(),
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let brokers = match load_brokers() {
        Ok(brokers) => brokers,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let store = match open_store() {
        Ok(store) => store,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let options = campaign::PlanOpts {
        campaign_id,
        jurisdiction: string_flag(parsed, "jurisdiction"),
        law: string_flag(parsed, "law"),
        priority: string_flag(parsed, "priority"),
        category: string_flag(parsed, "category"),
        status: string_flag_or(parsed, "status", "active"),
        include_inactive: bool_flag(parsed, "include-inactive"),
        include_disabled: false,
        max_brokers: int_flag(parsed, "max", 30),
        notes: string_flag(parsed, "notes"),
    };
    let result =
        match campaign::plan_campaign(&store, &brokers, &identity_hash, &options, now_utc()) {
            Ok(result) => result,
            Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        return match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(
        format!(
            "planned {} request(s) for campaign {}\n",
            result.planned, result.campaign_id
        )
        .into_bytes(),
    )
}

/// `plan show` — Go's `campaign.GetPlan` behind the `show` subcommand.
fn plan_show(parsed: &Parsed) -> Outcome {
    let store = match open_store() {
        Ok(store) => store,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let campaign_id = string_flag(parsed, "campaign");
    let status = string_flag(parsed, "status");
    let result = match campaign::get_plan(&store, &campaign_id, &status) {
        Ok(result) => Value::Object(result),
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let label = result["campaign_id"].as_str().unwrap_or("all");
    let total = result["total"].as_u64().unwrap_or_default();
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        return match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(format!("campaign {label}: {total} request(s)\n").into_bytes())
}

/// `plan execute` — Go's `realPlanCommand`'s `execute`.
///
/// Go gates a non-dry run behind a consent token, then builds a web-form
/// adapter with no `FormExecutor` and leaves the email sender nil, so no branch
/// of this command can reach the network. [`campaign::ExecuteOpts`] carries no
/// adapters at all, which keeps that true by construction.
fn plan_execute(parsed: &Parsed) -> Outcome {
    let campaign_id = string_flag(parsed, "campaign");
    if campaign_id.is_empty() {
        return Outcome::Stderr(b"--campaign is required\n".to_vec());
    }
    let store = match open_store() {
        Ok(store) => store,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let dry_run = bool_flag(parsed, "dry-run");
    if !dry_run && let Err(error) = consent_gate(parsed) {
        return Outcome::Stderr(format!("{error}\n").into_bytes());
    }
    let brokers = match load_brokers() {
        Ok(brokers) => brokers,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let profile = planning_profile(parsed);
    let result = match campaign::execute_campaign(
        &store,
        &campaign_id,
        &campaign::ExecuteOpts {
            account: string_flag(parsed, "account"),
            dry_run,
            brokers: &brokers,
        },
        profile.as_ref().map(Option::as_ref).map_err(String::as_str),
        int_flag(parsed, "batch-size", 5),
        now_utc(),
    ) {
        Ok(result) => result,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        return match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(
        format!("executed {} request(s)\n", go_value(&result["batch_size"])).into_bytes(),
    )
}

/// Go's `identity.ConsentGate("execute", ...)` for the flags `execute` declares.
fn consent_gate(parsed: &Parsed) -> Result<(), String> {
    let store = ConsentStore::from_default_directory().map_err(|error| error.to_string())?;
    store
        .authorize(
            "execute",
            &ConsentOptions {
                consent_token: Some(string_flag(parsed, "consent")),
                consent_file: Some(string_flag(parsed, "consent-file")),
                consent_env_var: Some("SYMERASEME_CONSENT".to_owned()),
                consent_file_env_var: Some("SYMERASEME_CONSENT_FILE".to_owned()),
                ..ConsentOptions::default()
            },
        )
        .map_err(|error| error.to_string())
}

/// `plan status` — Go's `planStatusCommand`.
///
/// The order is Go's and it is observable: the store is opened first, the status
/// is computed second, and only then is `--output` validated. A broken store
/// therefore wins over a bad `--output` value.
fn plan_status(parsed: &Parsed) -> Outcome {
    // `plan status` declares no `--campaign` flag at all, so it always reports
    // every campaign.
    campaign_status(parsed, "")
}

/// Go's `status` and `plan status` are the same body.
///
/// `realStatusCommand` and `planStatusCommand` both call
/// `reporting.GetCampaignStatus` and print either the marshalled result or
/// `Total: %v`; only the top-level `status` declares a `--campaign` flag. The
/// recorded oracle bytes are identical for both commands, which is what makes
/// this a shared function rather than two implementations.
fn campaign_status(parsed: &Parsed, campaign_id: &str) -> Outcome {
    let store = match open_store() {
        Ok(store) => store,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let result = match reporting::get_campaign_status(&store, campaign_id, now_utc()) {
        Ok(result) => result,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        return match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(format!("Total: {}\n", go_value(&result["totals"])).into_bytes())
}

/// One string flag as the command saw it; `""` when it was not given.
fn string_flag(parsed: &Parsed, name: &str) -> String {
    parsed.flags.get(name).cloned().unwrap_or_default()
}

/// One boolean flag as the command saw it.
fn bool_flag(parsed: &Parsed, name: &str) -> bool {
    parsed.flags.get(name).is_some_and(|value| value == "true")
}

/// One string flag, falling back to Go's default when it was not given.
fn string_flag_or(parsed: &Parsed, name: &str, default: &str) -> String {
    match parsed.flags.get(name) {
        Some(value) => value.clone(),
        None => default.to_owned(),
    }
}

/// One integer flag as the command saw it, falling back to Go's default when it
/// was not given.
fn int_flag(parsed: &Parsed, name: &str, default: i64) -> i64 {
    parsed
        .flags
        .get(name)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Go's `mcp.ContractHandler()`: the process working directory, the resolved
/// configuration and the current instant.
fn contract_handler() -> Result<ContractHandler, String> {
    let root = env::current_dir().map_err(|error| error.to_string())?;
    Ok(ContractHandler::new(root).with_store(process_context(), now_utc()))
}

/// The `mcp --stdio` / `serve --stdio` body: an optional banner on stderr,
/// then Go's `ServeStdio` loop over the process pipes. Clean EOF exits 0.
pub(crate) fn serve_stdio(notice: Option<Vec<u8>>) -> Outcome {
    use std::io::Write as _;

    if let Some(bytes) = &notice {
        let _ = std::io::stderr().write_all(bytes);
    }
    let handler = match contract_handler() {
        Ok(handler) => handler,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    match crate::mcp::stream::serve_stdio(&mut input, &mut output, &handler) {
        Ok(()) => Outcome::Stdout(Vec::new()),
        Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
    }
}

/// Starts the production MCP HTTP server after the command surface has been
/// parsed, keeping long-running process behavior out of the parser outcome.
pub(crate) fn serve_http(
    host: String,
    port: i64,
    allow_remote: bool,
    notice: Option<Vec<u8>>,
) -> Outcome {
    use std::io::Write as _;

    if let Some(bytes) = notice {
        let _ = std::io::stderr().write_all(&bytes);
    }
    match crate::mcp::http::serve(host, port, allow_remote, || {
        contract_handler().map(|handler| std::sync::Arc::new(handler) as std::sync::Arc<_>)
    }) {
        Ok(()) => Outcome::Stdout(Vec::new()),
        Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
    }
}

/// The four CLI commands that are thin wrappers over an MCP tool share Go's
/// body: call the tool, print its result as JSON, or `success` in text mode.
fn contract_command(tool: &str, arguments: Map<String, Value>, parsed: &Parsed) -> Outcome {
    match contract_result(tool, &arguments, parsed) {
        Err(outcome) => outcome,
        Ok(None) => Outcome::Stdout(b"success\n".to_vec()),
        Ok(Some(result)) => match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        },
    }
}

/// Go's `poll-inbox` CLI adapter: forward only explicitly supplied flags and
/// print the handler's message in text mode.
fn poll_inbox_command(parsed: &Parsed) -> Outcome {
    let mut arguments = Map::new();
    for (flag, key) in [
        ("host", "host"),
        ("username", "username"),
        ("oauth2-access-token", "oauth2_access_token"),
        ("oauth2-username", "oauth2_username"),
        ("campaign-id", "campaign_id"),
        ("folders", "folders"),
    ] {
        if let Some(value) = parsed.flags.get(flag) {
            arguments.insert(key.to_owned(), json!(value));
        }
    }
    for flag in ["port", "since", "since-days"] {
        if let Some(value) = parsed.flags.get(flag) {
            let number = match value.parse::<i64>() {
                Ok(number) => number,
                Err(_) => {
                    let parse_error = if value.parse::<i64>().is_err_and(|error| {
                        matches!(
                            error.kind(),
                            std::num::IntErrorKind::PosOverflow
                                | std::num::IntErrorKind::NegOverflow
                        )
                    }) {
                        "value out of range"
                    } else {
                        "invalid syntax"
                    };
                    return Outcome::Stderr(format!(
                        "invalid argument {value:?} for \"--{flag}\" flag: strconv.ParseInt: parsing {value:?}: {parse_error}\n"
                    ).into_bytes());
                }
            };
            if flag == "port" {
                arguments.insert("port".to_owned(), json!(number));
            } else {
                arguments.insert("since_days".to_owned(), json!(number));
            }
        }
    }
    if let Some(value) = parsed.flags.get("ssl") {
        arguments.insert("ssl".to_owned(), json!(value == "true"));
    }

    let handler = match contract_handler() {
        Ok(handler) => handler,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let result = match handler.call("poll_inbox", &arguments) {
        Ok(result) => result,
        Err(error) => return Outcome::Stderr(format!("{}\n", error.0).into_bytes()),
    };
    match output_format(parsed) {
        Err(outcome) => outcome,
        Ok("json") => match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        },
        Ok(_) => Outcome::Stdout(
            result
                .get("message")
                .and_then(Value::as_str)
                .map(|message| format!("{message}\n").into_bytes())
                .unwrap_or_else(|| b"success\n".to_vec()),
        ),
    }
}

/// The tool result when the command asked for JSON, `None` for text mode.
fn contract_result(
    tool: &str,
    arguments: &Map<String, Value>,
    parsed: &Parsed,
) -> Result<Option<Value>, Outcome> {
    contract_result_with_format(tool, arguments, output_format(parsed))
}

/// [`contract_result`] with an explicit format: `generate-dashboard` and
/// `generate-report` declare their own `--output` file flag, so Go reads the
/// format from the root persistent flag instead of the merged value. Like
/// Go's `RunE`, the tool runs before the format is validated, so a tool
/// error takes precedence over a format error.
fn contract_result_with_format(
    tool: &str,
    arguments: &Map<String, Value>,
    format: Result<&str, Outcome>,
) -> Result<Option<Value>, Outcome> {
    let handler =
        contract_handler().map_err(|error| Outcome::Stderr(format!("{error}\n").into_bytes()))?;
    let result = handler
        .call(tool, arguments)
        .map_err(|error| Outcome::Stderr(format!("{}\n", error.0).into_bytes()))?;
    Ok((format? == "json").then_some(result))
}

/// `contract_command` for the tools whose JSON result is carried as pre-rendered
/// text because Go marshals a *struct* there and keeps its field order:
/// `get_events`' `[]Event` and `grant --list-tokens`' `[]ConsentToken`.
fn contract_command_rendered(
    tool: &str,
    arguments: Map<String, Value>,
    parsed: &Parsed,
) -> Outcome {
    match contract_result(tool, &arguments, parsed) {
        Err(outcome) => outcome,
        Ok(None) => Outcome::Stdout(b"success\n".to_vec()),
        Ok(Some(Value::String(rendered))) => Outcome::Stdout(format!("{rendered}\n").into_bytes()),
        Ok(Some(result)) => match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        },
    }
}

/// Go's `events show` `PreRunE`: the positional overrides `--request-id`.
fn events_arguments(parsed: &Parsed, request_id: i64) -> Map<String, Value> {
    let mut arguments = Map::new();
    arguments.insert("request_id".to_owned(), json!(request_id));
    arguments.insert(
        "after_event_id".to_owned(),
        json!(int_flag(parsed, "after-event-id", 0)),
    );
    arguments
}

/// Go's `grant` `PreRunE`: every flag is sent, the positional overrides
/// `--command`, and `--list` is a second name for `--list-tokens`.
fn grant_arguments(parsed: &Parsed) -> Map<String, Value> {
    let command = match parsed.positional.first() {
        Some(positional) => positional.clone(),
        None => string_flag_or(parsed, "command", "execute"),
    };
    let mut arguments = Map::new();
    arguments.insert("command".to_owned(), json!(command));
    arguments.insert("ttl".to_owned(), json!(int_flag(parsed, "ttl", 86_400)));
    arguments.insert("revoke".to_owned(), json!(string_flag(parsed, "revoke")));
    arguments.insert(
        "revoke_all".to_owned(),
        json!(bool_flag(parsed, "revoke-all")),
    );
    arguments.insert(
        "list_tokens".to_owned(),
        json!(bool_flag(parsed, "list-tokens") || bool_flag(parsed, "list")),
    );
    arguments.insert("dry_run".to_owned(), json!(bool_flag(parsed, "dry-run")));
    arguments
}

/// Go's `generate-dashboard` `PreRunE`: every flag is sent, `--open` and
/// `--auto-open` feed one argument, and the file flag defaults to
/// `report.html`. The format comes from the root persistent `--output`
/// because the local `--output` names the file, exactly like cobra's
/// shadowing.
fn generate_dashboard_command(parsed: &Parsed) -> Outcome {
    let mut arguments = Map::new();
    arguments.insert(
        "output".to_owned(),
        json!(
            parsed
                .local_output
                .clone()
                .unwrap_or("report.html".to_owned())
        ),
    );
    arguments.insert(
        "auto_open".to_owned(),
        json!(bool_flag(parsed, "open") || bool_flag(parsed, "auto-open")),
    );
    arguments.insert(
        "auto_refresh".to_owned(),
        json!(int_flag(parsed, "auto-refresh", 0)),
    );
    match contract_result_with_format("generate_dashboard", &arguments, persistent_format(parsed)) {
        Err(outcome) => outcome,
        Ok(None) => Outcome::Stdout(b"success\n".to_vec()),
        Ok(Some(result)) => match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        },
    }
}

/// Go's `generate-report` `PreRunE`: every flag is sent, `--all` and
/// `--all-campaigns` feed one argument. Same `--output` shadowing as
/// `generate-dashboard`.
fn generate_report_command(parsed: &Parsed) -> Outcome {
    let mut arguments = Map::new();
    arguments.insert(
        "campaign_id".to_owned(),
        json!(string_flag(parsed, "campaign-id")),
    );
    arguments.insert(
        "format".to_owned(),
        json!(string_flag_or(parsed, "format", "html")),
    );
    arguments.insert(
        "output".to_owned(),
        json!(parsed.local_output.clone().unwrap_or_default()),
    );
    arguments.insert(
        "all_campaigns".to_owned(),
        json!(bool_flag(parsed, "all-campaigns") || bool_flag(parsed, "all")),
    );
    match contract_result_with_format("generate_report", &arguments, persistent_format(parsed)) {
        Err(outcome) => outcome,
        Ok(None) => Outcome::Stdout(b"success\n".to_vec()),
        Ok(Some(result)) => match json_line(&result) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        },
    }
}

/// Go's `outputFormat`: the root persistent `--output`, defaulting to text.
/// A value set after the subcommand belongs to a shadowing local flag and
/// never reaches the format.
fn persistent_format(parsed: &Parsed) -> Result<&str, Outcome> {
    let value = parsed.root_output.as_deref().unwrap_or("text");
    match value {
        "text" | "json" => Ok(value),
        _ => Err(Outcome::Stderr(
            format!("invalid output format {value:?}: use text or json\n").into_bytes(),
        )),
    }
}

/// Go's `generate-scheduler` `PreRunE`: every flag is sent. This command has
/// no local `--output`, so the merged format applies directly.
fn generate_scheduler_command(parsed: &Parsed) -> Outcome {
    let mut arguments = Map::new();
    arguments.insert(
        "platform".to_owned(),
        json!(string_flag(parsed, "platform")),
    );
    arguments.insert(
        "output_dir".to_owned(),
        json!(string_flag(parsed, "output-dir")),
    );
    arguments.insert(
        "tick_hour".to_owned(),
        json!(int_flag(parsed, "tick-hour", 10)),
    );
    arguments.insert(
        "tick_minute".to_owned(),
        json!(int_flag(parsed, "tick-minute", 0)),
    );
    arguments.insert(
        "poll_hours".to_owned(),
        json!(string_flag(parsed, "poll-hours")),
    );
    arguments.insert(
        "project_dir".to_owned(),
        json!(string_flag(parsed, "project-dir")),
    );
    arguments.insert(
        "symeraseme_bin".to_owned(),
        json!(string_flag(parsed, "symeraseme-bin")),
    );
    arguments.insert(
        "venv_activate".to_owned(),
        json!(string_flag(parsed, "venv-activate")),
    );
    arguments.insert("dry_run".to_owned(), json!(bool_flag(parsed, "dry-run")));
    contract_command("generate_scheduler", arguments, parsed)
}

/// Go's `calendar` `PreRunE`: `--campaign` and `--campaign-id` feed one key.
fn calendar_arguments(parsed: &Parsed) -> Map<String, Value> {
    let campaign = {
        let named = string_flag(parsed, "campaign-id");
        if named.is_empty() {
            string_flag(parsed, "campaign")
        } else {
            named
        }
    };
    let mut arguments = Map::new();
    arguments.insert("weeks".to_owned(), json!(int_flag(parsed, "weeks", 4)));
    arguments.insert("campaign_id".to_owned(), json!(campaign));
    arguments
}

/// Go's `requests list` `PreRunE`: every flag is always sent, defaults included.
fn requests_arguments(parsed: &Parsed) -> Map<String, Value> {
    let mut arguments = Map::new();
    arguments.insert(
        "campaign_id".to_owned(),
        json!(string_flag(parsed, "campaign-id")),
    );
    arguments.insert("status".to_owned(), json!(string_flag(parsed, "status")));
    arguments.insert(
        "broker_id".to_owned(),
        json!(string_flag(parsed, "broker-id")),
    );
    arguments.insert("page".to_owned(), json!(int_flag(parsed, "page", 1)));
    arguments.insert(
        "page_size".to_owned(),
        json!(int_flag(parsed, "page-size", 100)),
    );
    arguments
}

/// Go's `manual-tasks list` `PreRunE`.
fn manual_tasks_arguments(parsed: &Parsed) -> Map<String, Value> {
    let mut arguments = Map::new();
    arguments.insert("status".to_owned(), json!(string_flag(parsed, "status")));
    arguments.insert(
        "request_id".to_owned(),
        json!(int_flag(parsed, "request-id", 0)),
    );
    arguments
}

/// Go's `intArgument`: the positional `TASK_ID` wins over `--task-id`.
fn int_argument(parsed: &Parsed, flag: &str, name: &str) -> Result<i64, Outcome> {
    let Some(argument) = parsed.positional.first() else {
        return Ok(int_flag(parsed, flag, 0));
    };
    argument
        .parse()
        .map_err(|_| Outcome::Stderr(format!("invalid {name} {argument:?}\n").into_bytes()))
}

/// `review` — Go's `realRedactFileCommand`: the positional wins over `--path`,
/// an empty path fails before the tool runs, and the body is the
/// `redact_file` contract handler (text mode prints only `success`).
fn review_command(parsed: &Parsed) -> Outcome {
    let path = parsed
        .positional
        .first()
        .cloned()
        .unwrap_or_else(|| string_flag(parsed, "path"));
    if path.is_empty() {
        return Outcome::Stderr(b"a file path is required\n".to_vec());
    }
    let mut arguments = Map::new();
    arguments.insert("path".to_owned(), json!(path));
    contract_command("redact_file", arguments, parsed)
}

/// `run-web-form` — Go's `realRunWebFormCommand`: all five arguments are sent
/// on every call (the positional overrides `--broker-id`), the tool runs
/// before the format is validated, and the answer goes through Go's web-action
/// rendering.
fn run_web_form_command(parsed: &Parsed) -> Outcome {
    let mut arguments = Map::new();
    let broker_id = parsed
        .positional
        .first()
        .cloned()
        .unwrap_or_else(|| string_flag(parsed, "broker-id"));
    arguments.insert("broker_id".to_owned(), json!(broker_id));
    arguments.insert(
        "request_id".to_owned(),
        json!(int_flag(parsed, "request-id", 0)),
    );
    arguments.insert("headed".to_owned(), json!(bool_flag(parsed, "headed")));
    arguments.insert(
        "screenshot_dir".to_owned(),
        json!(string_flag(parsed, "screenshot-dir")),
    );
    arguments.insert("dry_run".to_owned(), json!(bool_flag(parsed, "dry-run")));

    // Same order as `contract_result_with_format`: the tool runs before the
    // output format is validated, so a tool error wins over a format error.
    let handler = match contract_handler() {
        Ok(handler) => handler,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let result = match handler.call("run_web_form", &arguments) {
        Ok(result) => result,
        Err(error) => return Outcome::Stderr(format!("{}\n", error.0).into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        let bytes = match json_line(&result) {
            Ok(bytes) => bytes,
            Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
        return match web_action_error_message(&result) {
            None => Outcome::Stdout(bytes),
            Some(message) => Outcome::StdoutStderr(bytes, message),
        };
    }
    web_action_text(&result)
}

/// Render the migration report even when a per-item operation fails.
fn migrate_command(parsed: &Parsed) -> Outcome {
    let source = string_flag(parsed, "source");
    let destination = string_flag(parsed, "destination");
    if source.is_empty() || destination.is_empty() {
        return Outcome::Stderr(
            b"migrate requires --source and --destination; use --dry-run first\n".to_vec(),
        );
    }
    // Go resolves the home directory before Run when --home is empty.
    let mut home = string_flag(parsed, "home");
    if home.is_empty() {
        home = match env::var("HOME") {
            Ok(value) if !value.is_empty() => value,
            _ => {
                return Outcome::Stderr(b"resolve home directory: $HOME is not defined\n".to_vec());
            }
        };
    }
    let options = symeraseme_engine::migration::Options {
        source_root: source,
        destination_root: destination,
        home_dir: home,
        source_config_root: string_flag(parsed, "source-config"),
        destination_config_root: string_flag(parsed, "destination-config"),
        backup_dir: string_flag(parsed, "backup"),
        platform: string_flag(parsed, "platform"),
        binary_path: string_flag(parsed, "binary"),
        project_dir: string_flag(parsed, "project-dir"),
        copy_secrets: bool_flag(parsed, "copy-secrets"),
        dry_run: bool_flag(parsed, "dry-run"),
        ..Default::default()
    };
    let (report, error) = symeraseme_engine::migration::run(&options);
    match report {
        Some(report) => migration_output(parsed, &report, error),
        None => Outcome::Stderr(format!("{}\n", error.unwrap_or_default()).into_bytes()),
    }
}

fn migration_output(
    parsed: &Parsed,
    report: &symeraseme_engine::migration::Report,
    error: Option<String>,
) -> Outcome {
    let bytes = if bool_flag(parsed, "json") {
        match serde_json::to_vec_pretty(report) {
            Ok(mut bytes) => {
                normalize_go_json(&mut bytes);
                bytes.push(b'\n');
                bytes
            }
            Err(e) => return Outcome::Stderr(format!("{e}\n").into_bytes()),
        }
    } else {
        let mut text = format!("{}\n", report.detection.summary);
        if !report.backup_dir.is_empty() {
            text.push_str(&format!("Backup: {}\n", report.backup_dir));
        }
        for item in report.items.iter().flatten() {
            text.push_str(&format!("[{}] {}\n", item.status, item.artifact.id));
        }
        for warning in &report.warnings {
            text.push_str(&format!("Warning: {warning}\n"));
        }
        if report.dry_run {
            text.push_str("Dry run: no files were changed.\n");
        } else if report.complete {
            text.push_str("Migration complete; the Python source was retained.\n");
        }
        text.into_bytes()
    };
    match error {
        Some(error) => Outcome::StdoutStderr(bytes, format!("{error}\n").into_bytes()),
        None => Outcome::Stdout(bytes),
    }
}

/// `auto-confirm` — Go's `realAutoConfirmCommand`: a thin wrapper over the
/// `auto_confirm` tool (the tool runs before the format is validated); JSON
/// writes the confirmation result, then `webActionError` decides the exit,
/// text renders the struct-shaped action line.
fn auto_confirm_command(parsed: &Parsed) -> Outcome {
    let request_id = match int_argument(parsed, "request-id", "request ID") {
        Ok(request_id) => request_id,
        Err(outcome) => return outcome,
    };
    let mut arguments = Map::new();
    arguments.insert("request_id".to_owned(), json!(request_id));
    arguments.insert("headed".to_owned(), json!(bool_flag(parsed, "headed")));
    arguments.insert(
        "screenshot_dir".to_owned(),
        json!(string_flag(parsed, "screenshot-dir")),
    );
    arguments.insert("dry_run".to_owned(), json!(bool_flag(parsed, "dry-run")));
    let handler = match contract_handler() {
        Ok(handler) => handler,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let result = match handler.call("auto_confirm", &arguments) {
        Ok(result) => result,
        Err(error) => return Outcome::Stderr(format!("{}\n", error.0).into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        let bytes = match json_line(&result) {
            Ok(bytes) => bytes,
            Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
        return match web_action_error_message(&result) {
            None => Outcome::Stdout(bytes),
            Some(message) => Outcome::StdoutStderr(bytes, message),
        };
    }
    web_action_text(&result)
}

/// Go's `webActionError`: `None` on success.
///
/// Two shapes reach it: the lower-case `map[string]any` tool results and the
/// capital-key `confirmation.Result` struct `auto_confirm` answers with.
fn web_action_error_message(result: &Value) -> Option<Vec<u8>> {
    if result.get("Success").is_some() {
        if result
            .get("Success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return None;
        }
        if confirmation_requires_manual(result) {
            return Some(b"manual confirmation required\n".to_vec());
        }
        return Some(b"action not completed\n".to_vec());
    }
    // ponytail: Go's default arm formats the unreachable non-map case as
    // `unexpected web action result %T`; this handler only ever answers with a
    // JSON object, so a non-object is folded into the generic failure.
    // Upgrade path: mirror Go's `%T` text if a non-map result is ever wired.
    if result
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    if result.get("status").and_then(Value::as_str) == Some("manual_action_required") {
        return Some(b"manual action required\n".to_vec());
    }
    Some(b"action not completed\n".to_vec())
}

/// Go's `case confirmation.Result` branch condition, shared by the error and
/// the text renderer.
fn confirmation_requires_manual(result: &Value) -> bool {
    result
        .get("ManualActionRequired")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || result.get("Step").and_then(Value::as_str) == Some("manual_confirmation_required")
}

/// Go's `writeWebActionText`, for the capital-key `confirmation.Result`
/// shape `auto_confirm` returns.
fn confirmation_text(result: &Value) -> Outcome {
    if result
        .get("Success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Outcome::Stdout(b"success\n".to_vec());
    }
    if confirmation_requires_manual(result) {
        let line = format!(
            "manual_confirmation_required task_id={} url={}\n{}\n",
            go_fmt_value(result.get("TaskID")),
            go_fmt_value(result.get("ClickedURL")),
            go_fmt_value(result.get("Instructions")),
        );
        return Outcome::StdoutStderr(
            line.into_bytes(),
            b"manual confirmation required\n".to_vec(),
        );
    }
    let line = format!(
        "not_completed step={}: {}\n",
        go_fmt_value(result.get("Step")),
        go_fmt_value(result.get("Error")),
    );
    Outcome::StdoutStderr(line.into_bytes(), b"action not completed\n".to_vec())
}

/// Go's `writeWebActionText` for the `map[string]any` arm.
fn web_action_text(result: &Value) -> Outcome {
    if result.get("Success").is_some() {
        return confirmation_text(result);
    }
    if result
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Outcome::Stdout(b"success\n".to_vec());
    }
    if result.get("status").and_then(Value::as_str) == Some("manual_action_required") {
        let line = format!(
            "manual_action_required task_id={} url={}\n{}\n",
            go_fmt_value(result.get("task_id")),
            go_fmt_value(result.get("url")),
            go_fmt_value(result.get("instructions")),
        );
        return Outcome::StdoutStderr(line.into_bytes(), b"manual action required\n".to_vec());
    }
    let line = format!("not_completed: {}\n", go_fmt_value(result.get("error")));
    Outcome::StdoutStderr(line.into_bytes(), b"action not completed\n".to_vec())
}

/// Go's `fmt` `%v` for the scalars a web-action map carries. Missing keys are
/// nil and print `<nil>`.
// ponytail: the non-scalar fallback renders compact JSON rather than Go's
// `[a b]` slice spelling; only `task_id`, `url`, `instructions` and `error`
// reach this helper and all four are scalars or nil.
fn go_fmt_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "<nil>".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(other) => other.to_string(),
    }
}

/// `plan tick` — Go's `tickCommandWith`.
///
/// `--dry-run` only decides whether the tick mutates: Go never calls the apply
/// step separately and always leaves the batch limit disabled (`batch_size: 0`).
fn plan_tick(parsed: &Parsed) -> Outcome {
    let store = match open_store() {
        Ok(store) => store,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let dry_run = parsed
        .flags
        .get("dry-run")
        .is_some_and(|value| value == "true");
    let actions = match deadlines::run_tick(
        &store,
        &RunOpts {
            dry_run,
            batch_size: 0,
        },
        now_utc(),
    ) {
        Ok(actions) => actions,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        // Go marshals a `map[string]any`, so the keys come out sorted, and a nil
        // action slice stays `null` rather than becoming `[]`.
        let serialized = match serde_json::to_value(&actions) {
            Ok(value) => value,
            Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
        let actions_value = if actions.is_empty() {
            Value::Null
        } else {
            serialized
        };
        let payload = json!({
            "actions": actions_value,
            "dry_run": dry_run,
            "success": true,
        });
        return match json_line(&payload) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(format!("tick complete: {} action(s)\n", actions.len()).into_bytes())
}

fn output_format(parsed: &Parsed) -> Result<&str, Outcome> {
    let format = parsed
        .flags
        .get("output")
        .map(String::as_str)
        .unwrap_or("text");
    match format {
        "text" | "json" => Ok(format),
        value => Err(Outcome::Stderr(
            format!("invalid output format {value:?}: use text or json\n").into_bytes(),
        )),
    }
}

fn json_line<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(value)?;
    normalize_go_json(&mut bytes);
    bytes.push(b'\n');
    Ok(bytes)
}

fn normalize_go_json(bytes: &mut Vec<u8>) {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            let html_escape = match byte {
                b'&' => Some(b"\\u0026".as_slice()),
                b'<' => Some(b"\\u003c".as_slice()),
                b'>' => Some(b"\\u003e".as_slice()),
                _ => None,
            };
            if let Some(escape) = html_escape {
                normalized.extend_from_slice(escape);
                index += 1;
                continue;
            }
            if bytes[index..].starts_with(b"\xE2\x80\xA8") {
                normalized.extend_from_slice(b"\\u2028");
                index += 3;
                continue;
            }
            if bytes[index..].starts_with(b"\xE2\x80\xA9") {
                normalized.extend_from_slice(b"\\u2029");
                index += 3;
                continue;
            }
            normalized.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            normalized.push(byte);
            index += 1;
            continue;
        }
        if byte == b'-' || byte.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_digit()
                    || matches!(bytes[index], b'.' | b'e' | b'E' | b'+' | b'-'))
            {
                index += 1;
            }
            let token = &bytes[start..index];
            if token.ends_with(b".0") && !token.iter().any(|byte| matches!(byte, b'e' | b'E')) {
                normalized.extend_from_slice(&token[..token.len() - 2]);
            } else {
                normalized.extend_from_slice(token);
            }
            continue;
        }
        normalized.push(byte);
        index += 1;
    }
    *bytes = normalized;
}

fn load_brokers() -> Result<Vec<Broker>, symeraseme_core::registry::RegistryError> {
    match env::var_os("SYMERASEME_RESOURCES") {
        Some(resources) if !resources.is_empty() => load_from_dir(Path::new(&resources)),
        _ => load_embedded(),
    }
}

fn brokers_list(parsed: &Parsed) -> Outcome {
    let brokers = match load_brokers() {
        Ok(brokers) => brokers,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let include_disabled = parsed
        .flags
        .get("include-disabled")
        .is_some_and(|value| value == "true");
    let include_inactive = parsed
        .flags
        .get("include-inactive")
        .is_some_and(|value| value == "true");
    let status = parsed
        .flags
        .get("status")
        .cloned()
        .unwrap_or_else(|| "active".to_owned());
    let filtered = filter_brokers(
        &brokers,
        &BrokerFilter {
            jurisdiction: parsed.flags.get("jurisdiction").cloned(),
            law: parsed.flags.get("law").cloned(),
            priority: parsed.flags.get("priority").cloned(),
            category: None,
            include_disabled,
            status: Some(status.clone()),
            include_inactive,
        },
    );
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        let envelope = BrokersListEnvelope {
            count: filtered.len(),
            brokers: filtered,
            filters: BrokersListFilters {
                include_disabled,
                status,
            },
            schema_version: 1,
        };
        return match json_line(&envelope) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(format!("{} broker(s)\n", filtered.len()).into_bytes())
}

fn brokers_show(parsed: &Parsed) -> Outcome {
    let Some(broker_id) = parsed.positional.first() else {
        return Outcome::Stderr(b"accepts 1 arg(s), received 0\n".to_vec());
    };
    let brokers = match load_brokers() {
        Ok(brokers) => brokers,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let Some(broker) = brokers.iter().find(|broker| broker.id == *broker_id) else {
        return Outcome::Stderr(format!("broker {broker_id:?} not found\n").into_bytes());
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        return match json_line(&BrokerShowEnvelope {
            broker,
            schema_version: 1,
        }) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(format!("{} ({})\n", broker.name, broker.id).into_bytes())
}

/// Go's `registry list`: the whole embedded registry, unfiltered.
///
/// Unlike `brokers list` this applies no status filter and emits no `filters`
/// object, because `realRegistryCommand` calls `loadRegistry` and marshals the
/// slice directly. The two commands therefore disagree on both the count
/// (1,277 here against 1,273 active) and the envelope shape, and that
/// disagreement is the contract.
fn registry_list(parsed: &Parsed) -> Outcome {
    let brokers = match load_brokers() {
        Ok(brokers) => brokers,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        let envelope = RegistryListEnvelope {
            count: brokers.len(),
            brokers: brokers.iter().collect(),
            schema_version: 1,
        };
        return match json_line(&envelope) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(format!("{} broker(s)\n", brokers.len()).into_bytes())
}

/// Go's `registry validate`: it re-loads the registry and reports the count.
///
/// The totals are load-bearing: a load that cannot complete returns the error
/// rather than a partial report, so `failed` and `duplicate_ids` are always
/// zero here. Reporting them as measured zeros would overstate the check, so
/// they are reported exactly as Go does — structurally present, always zero —
/// and the real per-file validation evidence lives with the loader
/// (`load_reporting_from_dir`), not in this envelope.
fn registry_validate(parsed: &Parsed) -> Outcome {
    let brokers = match load_brokers() {
        Ok(brokers) => brokers,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        let envelope = RegistryValidateEnvelope {
            ok: true,
            schema_version: 1,
            totals: RegistryValidateTotals {
                duplicate_ids: 0,
                failed: 0,
                valid: brokers.len(),
            },
        };
        return match json_line(&envelope) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(format!("OK: {} broker(s)\n", brokers.len()).into_bytes())
}

fn render_template(parsed: &Parsed) -> Outcome {
    let Some(template_name) = parsed.positional.first() else {
        return Outcome::Stderr(b"accepts 1 arg(s), received 0\n".to_vec());
    };
    let broker_name = parsed.flags.get("broker-name").cloned().unwrap_or_default();
    let broker_website = parsed
        .flags
        .get("broker-website")
        .cloned()
        .unwrap_or_default();

    let paths = ProfilePaths::from_process();
    let profile = if profile_exists(Path::new(""), &paths) {
        let mut keys = MasterKeyResolver::from_process();
        match load_profile(Path::new(""), &paths, &mut keys) {
            Ok(profile) => Some(profile),
            Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
        }
    } else {
        None
    };
    let context = render_context(profile.as_ref(), broker_name, broker_website);

    let lookup_name = normalize_template_name(template_name);
    if !list_template_names().contains(&lookup_name) {
        return Outcome::Stderr(
            format!("templating: unknown template {lookup_name:?}\n").into_bytes(),
        );
    }
    match render(lookup_name, &context) {
        Ok(content) => Outcome::Stdout(content.into_bytes()),
        Err(error) => {
            let message = match error.to_string().as_str() {
                "unknown template" | "invalid template name" => {
                    format!("templating: unknown template {lookup_name:?}")
                }
                message => format!("templating: {message}"),
            };
            Outcome::Stderr(format!("{message}\n").into_bytes())
        }
    }
}

fn init_profile_command(parsed: &Parsed) -> Outcome {
    let full_name = parsed
        .flags
        .get("full-name")
        .map(|value| value.trim())
        .unwrap_or_default();
    let email = parsed
        .flags
        .get("email")
        .map(|value| value.trim())
        .unwrap_or_default();
    if full_name.is_empty() || email.is_empty() {
        return Outcome::Stderr(b"--full-name and --email are required\n".to_vec());
    }
    if !is_plain_address(email) {
        return Outcome::Stderr(b"invalid email address\n".to_vec());
    }
    let profile = Profile {
        full_name: full_name.to_owned(),
        email_addresses: vec![email.to_owned()],
        ..Profile::default()
    };
    let requested = parsed.flags.get("profile").cloned().unwrap_or_default();
    let paths = ProfilePaths::from_process();
    let mut keys = MasterKeyResolver::from_process();
    let target = match init_profile(&profile, Path::new(&requested), &paths, &mut keys) {
        Ok(target) => target,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        let payload = json!({
            "profile_path": target.to_string_lossy(),
            "success": true,
        });
        return match json_line(&payload) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    Outcome::Stdout(format!("identity profile saved at {}\n", target.display()).into_bytes())
}

fn show_profile_command(parsed: &Parsed) -> Outcome {
    let paths = ProfilePaths::from_process();
    let mut keys = MasterKeyResolver::from_process();
    let profile = match load_profile(Path::new(""), &paths, &mut keys) {
        Ok(profile) => profile,
        Err(error) => return Outcome::Stderr(format!("{error}\n").into_bytes()),
    };
    let format = match output_format(parsed) {
        Ok(format) => format,
        Err(outcome) => return outcome,
    };
    if format == "json" {
        return match json_line(&profile) {
            Ok(bytes) => Outcome::Stdout(bytes),
            Err(error) => Outcome::Stderr(format!("{error}\n").into_bytes()),
        };
    }
    let mut output = format!("Name: {}\n", profile.full_name);
    for value in &profile.email_addresses {
        output.push_str(&format!("Email: {value}\n"));
    }
    for value in &profile.jurisdictions {
        output.push_str(&format!("Jurisdiction: {value}\n"));
    }
    Outcome::Stdout(output.into_bytes())
}

/// Go accepts the address only when `mail.ParseAddress` round-trips it
/// unchanged: a bare dot-atom local part, `@`, and a dot-atom or bracketed
/// domain literal. Display names and quoted local parts never round-trip.
fn is_plain_address(email: &str) -> bool {
    let Some((local, domain)) = email.rsplit_once('@') else {
        return false;
    };
    if !is_dot_atom(local) {
        return false;
    }
    if let Some(literal) = domain.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        return !literal.is_empty()
            && literal
                .chars()
                .all(|c| c > ' ' && c != '[' && c != ']' && c != '\\' && c != '\u{7f}');
    }
    is_dot_atom(domain)
}

fn is_dot_atom(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|atom| {
            !atom.is_empty()
                && atom.chars().all(|c| {
                    c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~".contains(c) || !c.is_ascii()
                })
        })
}

fn normalize_template_name(input: &str) -> &str {
    let name = input.strip_prefix("laws/").unwrap_or(input);
    name.strip_prefix("templates/").unwrap_or(name)
}

fn render_context(
    profile: Option<&Profile>,
    broker_name: String,
    broker_website: String,
) -> RenderContext {
    let mut context = RenderContext {
        broker_name,
        broker_website,
        ..RenderContext::default()
    };
    match profile {
        Some(profile) => {
            context.full_name = profile.full_name.clone();
            context.name_variants = profile.name_variants.clone();
            context.date_of_birth = Some(profile.date_of_birth.clone().unwrap_or_default());
            context.addresses = profile
                .addresses
                .iter()
                .map(|address| Address {
                    street: address.street.clone(),
                    city: address.city.clone(),
                    postal_code: address.postal_code.clone(),
                    country: address.country.clone(),
                })
                .collect();
            context.email_addresses = profile.email_addresses.clone();
            context.phone_numbers = profile.phone_numbers.clone();
            context.jurisdictions = profile.jurisdictions.clone();
        }
        None => context.full_name = "<no value>".to_owned(),
    }
    context
}

#[derive(Serialize)]
struct ConfigEnvelope<'a> {
    config: &'a Config,
    success: bool,
}

fn config_show(parsed: &Parsed) -> Outcome {
    let context = process_context();
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

#[cfg(test)]
mod migration_tests {
    use super::*;
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "migration-cli-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn migration_failed_secret_copy_emits_report_and_error() {
        let root = Scratch::new();
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("identity.enc"), b"synthetic encrypted fixture").unwrap();
        let destination = root.path().join("destination");
        let args = vec![
            "migrate".into(),
            "--source".into(),
            source.to_string_lossy().into_owned(),
            "--destination".into(),
            destination.to_string_lossy().into_owned(),
            "--home".into(),
            root.path().to_string_lossy().into_owned(),
            "--platform".into(),
            "cron".into(),
            "--copy-secrets".into(),
            "--json".into(),
        ];
        let Outcome::StdoutStderr(stdout, stderr) = execute(&args) else {
            panic!("expected report and error")
        };
        let report: Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(report["items"][0]["status"], "planned");
        assert_eq!(
            stderr,
            b"secret store was detected but no migratable SecretStore was injected\n"
        );
        assert!(!destination.exists());
    }
    #[test]
    fn migration_repeated_platform_uses_last_value() {
        let root = Scratch::new();
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let args = vec![
            "migrate".into(),
            "--source".into(),
            source.to_string_lossy().into_owned(),
            "--destination".into(),
            root.path()
                .join("destination")
                .to_string_lossy()
                .into_owned(),
            "--home".into(),
            root.path().to_string_lossy().into_owned(),
            "--platform".into(),
            "cron".into(),
            "--platform".into(),
            "unsupported".into(),
            "--dry-run".into(),
        ];
        let Outcome::Stderr(stderr) = execute(&args) else {
            panic!("expected invalid platform")
        };
        assert_eq!(
            stderr,
            b"unsupported platform: unsupported (choose cron, launchd, or systemd)\n"
        );
    }
}
