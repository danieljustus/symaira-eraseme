//! Explicit expected-value transforms for the post-cutover contract change
//! #1295 (see `docs/rust-port-contract-matrix.md`).
//!
//! The frozen Go records stay untouched. A replay applies exactly one narrow,
//! named transform per changed field and nothing else: no sorting, no
//! trimming, no broad normalization.
#![allow(dead_code)] // Each including test target uses a subset.

/// `initialize` `serverInfo` exactly as the retired Go server serialized it.
pub const GO_SERVER_INFO: &str = r#""serverInfo":{"name":"symeraseme","version":"dev"}"#;

/// Product version served in `initialize` since #1295 (workspace version).
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

fn current_server_info() -> String {
    format!(r#""serverInfo":{{"name":"symeraseme","version":"{PRODUCT_VERSION}"}}"#)
}

/// Rewrites exactly the recorded `serverInfo` fragment to the current product
/// version; every other byte of the recorded response is kept.
pub fn server_info_version(recorded: &str) -> String {
    recorded.replace(GO_SERVER_INFO, &current_server_info())
}

/// Byte form of [`server_info_version`] for recorded stdout/body bytes.
pub fn server_info_version_bytes(recorded: &[u8]) -> Vec<u8> {
    let needle = GO_SERVER_INFO.as_bytes();
    let replacement = current_server_info().into_bytes();
    let mut output = Vec::with_capacity(recorded.len() + 16);
    let mut index = 0;
    while index < recorded.len() {
        if recorded[index..].starts_with(needle) {
            output.extend_from_slice(&replacement);
            index += needle.len();
        } else {
            output.push(recorded[index]);
            index += 1;
        }
    }
    output
}

/// [`server_info_version_bytes`] applied to the body of a complete recorded
/// HTTP/1.1 response. Only when the body carried the `serverInfo` fragment is
/// the `Content-Length` header corrected to the new body length; every other
/// header byte stays as recorded.
pub fn http_response_server_info(raw: &[u8]) -> Vec<u8> {
    let Some(split) = raw.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
        return raw.to_vec();
    };
    let (head, body) = (&raw[..split + 4], &raw[split + 4..]);
    let new_body = server_info_version_bytes(body);
    if new_body == body {
        return raw.to_vec();
    }
    let head = String::from_utf8(head.to_vec()).expect("recorded HTTP head is UTF-8");
    let head = head
        .split("\r\n")
        .map(|line| {
            if line.to_ascii_lowercase().starts_with("content-length:") {
                format!("{}: {}", &line[.."content-length".len()], new_body.len())
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    let mut output = head.into_bytes();
    output.extend_from_slice(&new_body);
    output
}

/// Rewrites exactly the recorded `Total: map[open:A requests:B resolved:C]`
/// line of `status` / `plan status` text output to the current wording
/// `Total: B requests, A open, C resolved`. Anything else passes through.
pub fn status_total_line(recorded: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(recorded) else {
        return recorded.to_vec();
    };
    let Some(inner) = text
        .strip_prefix("Total: map[open:")
        .and_then(|rest| rest.strip_suffix("]\n"))
    else {
        return recorded.to_vec();
    };
    let Some((open, rest)) = inner.split_once(" requests:") else {
        return recorded.to_vec();
    };
    let Some((requests, resolved)) = rest.split_once(" resolved:") else {
        return recorded.to_vec();
    };
    format!("Total: {requests} requests, {open} open, {resolved} resolved\n").into_bytes()
}

/// Root `--help` long text and `migrate` short text as recorded from Go.
pub const GO_ROOT_LONG: &str = "symeraseme is the Go-port command-line entrypoint of Symaira EraseMe. The CLI vocabulary is settled in docs/mcp-contract.md §5; commands land as their port issues are implemented.";
pub const GO_MIGRATE_SHORT: &str = "Migrate a Python-era installation to the Go layout";
/// Current wording (`command_surface.rs`) that replaces them.
pub const ROOT_LONG: &str = "symeraseme is the command-line interface of Symaira EraseMe, which plans, sends and tracks data broker removal requests. Run `symeraseme registry validate` and `symeraseme config show` to check the broker registry and your configuration; `symeraseme <command> --help` describes every command.";
pub const MIGRATE_SHORT: &str = "Migrate a Python-era installation to the current layout";

/// Overrides exactly `root.long` and `migrate.short` in recorded help output
/// (`help-root`, `operate-help`, `help-migrate`); all other bytes are kept.
pub fn help_wording(recorded: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(recorded) else {
        return recorded.to_vec();
    };
    text.replace(GO_ROOT_LONG, ROOT_LONG)
        .replace(GO_MIGRATE_SHORT, MIGRATE_SHORT)
        .into_bytes()
}

/// Post-cutover addition #1297: `plan delete` joins the `plan` subcommands.
/// Inserts exactly one row between the recorded `create` and `execute` rows of
/// the `plan --help` command list (cobra lists subcommands alphabetically).
pub fn plan_help_with_delete(recorded: &[u8]) -> Vec<u8> {
    const RECORDED: &str =
        "  create      Create a removal campaign plan\n  execute     Execute planned requests\n";
    const CURRENT: &str = "  create      Create a removal campaign plan\n  delete      Delete a never-sent campaign\n  execute     Execute planned requests\n";
    let Ok(text) = std::str::from_utf8(recorded) else {
        return recorded.to_vec();
    };
    text.replace(RECORDED, CURRENT).into_bytes()
}

#[test]
fn transforms_are_narrow() {
    assert_eq!(
        plan_help_with_delete(
            b"Available Commands:\n  create      Create a removal campaign plan\n  execute     Execute planned requests\n  show        Show\n"
        ),
        b"Available Commands:\n  create      Create a removal campaign plan\n  delete      Delete a never-sent campaign\n  execute     Execute planned requests\n  show        Show\n"
    );
    assert_eq!(plan_help_with_delete(b"plan create\n"), b"plan create\n");
    assert_eq!(
        help_wording(format!("{GO_ROOT_LONG}\n\nUsage:\n  {GO_MIGRATE_SHORT}\n").as_bytes()),
        format!("{ROOT_LONG}\n\nUsage:\n  {MIGRATE_SHORT}\n").into_bytes()
    );
    assert_eq!(help_wording(b"Usage:\n"), b"Usage:\n");
    assert_ne!(PRODUCT_VERSION, "dev");
    let recorded = format!("x{GO_SERVER_INFO}y");
    assert_eq!(
        server_info_version(&recorded),
        format!(r#"x"serverInfo":{{"name":"symeraseme","version":"{PRODUCT_VERSION}"}}y"#)
    );
    assert_eq!(server_info_version("{}"), "{}");
    assert_eq!(
        server_info_version_bytes(recorded.as_bytes()),
        server_info_version(&recorded).into_bytes()
    );
    let body = format!("{{{GO_SERVER_INFO}}}");
    let raw = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{body}",
        body.len()
    );
    let new_body = server_info_version(&body);
    assert_eq!(
        http_response_server_info(raw.as_bytes()),
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{new_body}",
            new_body.len()
        )
        .into_bytes()
    );
    let other = b"HTTP/1.1 204 No Content\r\n\r\n";
    assert_eq!(http_response_server_info(other), other);
    assert_eq!(
        status_total_line(b"Total: map[open:2 requests:3 resolved:1]\n"),
        b"Total: 3 requests, 2 open, 1 resolved\n"
    );
    for untouched in [
        &b"Total: map[open:2 requests:3 resolved:1]"[..],
        b"Total: map[open:2 requests:3]\n",
        b"{\"totals\":{}}\n",
        b"",
    ] {
        assert_eq!(status_total_line(untouched), untouched);
    }
}
