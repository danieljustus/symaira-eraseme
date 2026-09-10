use super::{Config, WRAPPER_DIR_PLACEHOLDER, wrapper};
use std::collections::BTreeMap;

pub(super) fn generate(
    cfg: &Config,
    binary_path: &str,
    project_dir: &str,
    poll_hours: &[i32],
) -> BTreeMap<String, String> {
    let poll_intervals: Vec<String> = poll_hours.iter().map(|&hour| calendar(hour, 0)).collect();

    let mut files = BTreeMap::new();
    files.insert(
        "symeraseme-tick.sh".to_string(),
        wrapper(
            &[binary_path, "tick", "--output", "json"],
            project_dir,
            &cfg.venv_activate,
        ),
    );
    files.insert(
        "com.symeraseme.tick.plist".to_string(),
        plist(
            "com.symeraseme.tick",
            &format!("{WRAPPER_DIR_PLACEHOLDER}/symeraseme-tick.sh"),
            &calendar(cfg.tick_hour, cfg.tick_minute),
            false,
        ),
    );
    files.insert(
        "symeraseme-poll.sh".to_string(),
        wrapper(
            &[binary_path, "poll-inbox", "--output", "json"],
            project_dir,
            &cfg.venv_activate,
        ),
    );
    files.insert(
        "com.symeraseme.poll.plist".to_string(),
        plist(
            "com.symeraseme.poll",
            &format!("{WRAPPER_DIR_PLACEHOLDER}/symeraseme-poll.sh"),
            &poll_intervals.join("\n"),
            true,
        ),
    );
    files.insert(
        "symeraseme-rescan.sh".to_string(),
        wrapper(
            &[binary_path, "tick", "--output", "json"],
            project_dir,
            &cfg.venv_activate,
        ),
    );
    files.insert(
        "com.symeraseme.rescan.plist".to_string(),
        plist(
            "com.symeraseme.rescan",
            &format!("{WRAPPER_DIR_PLACEHOLDER}/symeraseme-rescan.sh"),
            &quarterly(cfg.tick_hour, cfg.tick_minute),
            true,
        ),
    );
    files.insert("install.sh".to_string(), install_script().to_string());
    files.insert("uninstall.sh".to_string(), uninstall_script().to_string());
    files
}

fn calendar(hour: i32, minute: i32) -> String {
    format!(
        "    <dict>\n        <key>Hour</key>\n        <integer>{hour}</integer>\n        <key>Minute</key>\n        <integer>{minute}</integer>\n    </dict>"
    )
}

fn quarterly(hour: i32, minute: i32) -> String {
    [1, 4, 7, 10]
        .iter()
        .map(|month| {
            format!(
                "    <dict>\n        <key>Month</key>\n        <integer>{month}</integer>\n        <key>Day</key>\n        <integer>1</integer>\n        <key>Hour</key>\n        <integer>{hour}</integer>\n        <key>Minute</key>\n        <integer>{minute}</integer>\n    </dict>"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Escapes text exactly as Go's `encoding/xml.EscapeText` does: `"`, `'`,
/// `&`, `<`, `>`, tab, newline and carriage return become named/numeric
/// character references; characters outside the XML 1.0 legal range (or a
/// lone encoded U+FFFD) become the literal replacement character; every
/// other codepoint, including all other Unicode, passes through unchanged.
fn plist_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("&#34;"),
            '\'' => escaped.push_str("&#39;"),
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\t' => escaped.push_str("&#x9;"),
            '\n' => escaped.push_str("&#xA;"),
            '\r' => escaped.push_str("&#xD;"),
            _ if is_xml_character(ch) => escaped.push(ch),
            _ => escaped.push('\u{FFFD}'),
        }
    }
    escaped
}

/// The XML 1.0 legal character range, matching Go's `isInCharacterRange`.
fn is_xml_character(ch: char) -> bool {
    let code = ch as u32;
    code == 0x09
        || code == 0x0A
        || code == 0x0D
        || (0x20..=0xD7FF).contains(&code)
        || (0xE000..=0xFFFD).contains(&code)
        || (0x10000..=0x10FFFF).contains(&code)
}

fn plist(label: &str, wrapper_path: &str, intervals: &str, array: bool) -> String {
    let intervals = if array {
        format!("    <array>\n{intervals}\n    </array>")
    } else {
        format!("    {intervals}")
    };
    let label = plist_escape(label);
    let wrapper_path = plist_escape(wrapper_path);
    let lines: [String; 29] = [
        r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_string(),
        r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN""#.to_string(),
        r#"  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#.to_string(),
        r#"<plist version="1.0">"#.to_string(),
        "<dict>".to_string(),
        "    <key>Label</key>".to_string(),
        format!("    <string>{label}</string>"),
        "    <key>ProgramArguments</key>".to_string(),
        "    <array>".to_string(),
        "        <string>/bin/bash</string>".to_string(),
        format!("        <string>{wrapper_path}</string>"),
        "    </array>".to_string(),
        "    <key>StartCalendarInterval</key>".to_string(),
        intervals,
        "    <key>StandardOutPath</key>".to_string(),
        format!("    <string>/tmp/{label}.log</string>"),
        "    <key>StandardErrorPath</key>".to_string(),
        format!("    <string>/tmp/{label}.err</string>"),
        "    <key>EnvironmentVariables</key>".to_string(),
        "    <dict>".to_string(),
        "        <key>SYMERASEME_HEADLESS</key>".to_string(),
        "        <string>1</string>".to_string(),
        "    </dict>".to_string(),
        "    <key>RunAtLoad</key>".to_string(),
        "    <false/>".to_string(),
        "    <key>KeepAlive</key>".to_string(),
        "    <false/>".to_string(),
        "</dict>".to_string(),
        "</plist>".to_string(),
    ];
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn install_script() -> &'static str {
    "#!/usr/bin/env bash\n\
     set -euo pipefail\n\
     SCRIPT_DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
     DEST=\"$HOME/Library/LaunchAgents\"\n\
     mkdir -p \"$DEST\"\n\
     for plist in \"$SCRIPT_DIR\"/*.plist; do\n  \
     [ -f \"$plist\" ] || continue\n  \
     name=$(basename \"$plist\")\n  \
     sed \"s|__WRAPPER_DIR__|$SCRIPT_DIR|g\" \"$plist\" > \"$DEST/$name\"\n  \
     chmod 644 \"$DEST/$name\"\n  \
     launchctl unload \"$DEST/$name\" 2>/dev/null || true\n  \
     launchctl load \"$DEST/$name\"\n\
     done\n\
     chmod +x \"$SCRIPT_DIR\"/*.sh\n\
     printf '%s\\n' 'launchd jobs installed.'\n"
}

fn uninstall_script() -> &'static str {
    "#!/usr/bin/env bash\n\
     set -euo pipefail\n\
     SCRIPT_DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
     for plist in \"$SCRIPT_DIR\"/*.plist; do\n  \
     [ -f \"$plist\" ] || continue\n  \
     name=$(basename \"$plist\")\n  \
     dest=\"$HOME/Library/LaunchAgents/$name\"\n  \
     launchctl unload \"$dest\" 2>/dev/null || true\n  \
     rm -f \"$dest\"\n\
     done\n\
     printf '%s\\n' 'launchd jobs uninstalled.'\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_escape_matches_go_xml_escape_text() {
        assert_eq!(plist_escape("com.symeraseme.tick"), "com.symeraseme.tick");
        assert_eq!(
            plist_escape("a\"b'c&d<e>f\tg\nh\ri"),
            "a&#34;b&#39;c&amp;d&lt;e&gt;f&#x9;g&#xA;h&#xD;i"
        );
    }
}
