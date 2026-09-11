use super::{Config, WRAPPER_DIR_PLACEHOLDER, wrapper};
use std::collections::BTreeMap;

pub(super) fn generate(
    cfg: &Config,
    binary_path: &str,
    project_dir: &str,
    _poll_hours: &[i32],
) -> BTreeMap<String, String> {
    // systemd invokes the poll wrapper hourly, matching Python; poll hours
    // do not vary the generated unit files.
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
        "symeraseme-tick.service".to_string(),
        service(
            "Symaira EraseMe daily tick",
            &format!("{WRAPPER_DIR_PLACEHOLDER}/symeraseme-tick.sh"),
        ),
    );
    files.insert(
        "symeraseme-tick.timer".to_string(),
        timer(
            "Symaira EraseMe daily tick",
            "symeraseme-tick",
            &format!("Daily-{:02}:{:02}", cfg.tick_hour, cfg.tick_minute),
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
        "symeraseme-poll.service".to_string(),
        service(
            "Symaira EraseMe hourly inbox poll",
            &format!("{WRAPPER_DIR_PLACEHOLDER}/symeraseme-poll.sh"),
        ),
    );
    files.insert(
        "symeraseme-poll.timer".to_string(),
        timer(
            "Symaira EraseMe hourly inbox poll",
            "symeraseme-poll",
            "Hourly",
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
        "symeraseme-rescan.service".to_string(),
        service(
            "Symaira EraseMe quarterly re-scan",
            &format!("{WRAPPER_DIR_PLACEHOLDER}/symeraseme-rescan.sh"),
        ),
    );
    files.insert(
        "symeraseme-rescan.timer".to_string(),
        timer(
            "Symaira EraseMe quarterly re-scan",
            "symeraseme-rescan",
            "Quarterly",
        ),
    );
    files.insert("install.sh".to_string(), install_script().to_string());
    files.insert("uninstall.sh".to_string(), uninstall_script().to_string());
    files
}

fn service(description: &str, wrapper_path: &str) -> String {
    format!(
        "[Unit]\n\
         Description={description}\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart=/bin/bash \"{wrapper_path}\"\n\
         Environment=SYMERASEME_HEADLESS=1\n\
         StandardOutput=journal\n\
         StandardError=journal\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

fn timer(description: &str, service_name: &str, calendar: &str) -> String {
    format!(
        "[Unit]\n\
         Description={description}\n\
         \n\
         [Timer]\n\
         Unit={service_name}.service\n\
         OnCalendar={calendar}\n\
         Persistent=true\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
    )
}

fn install_script() -> &'static str {
    "#!/usr/bin/env bash\n\
     set -euo pipefail\n\
     SCRIPT_DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
     DEST=\"${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user\"\n\
     mkdir -p \"$DEST\"\n\
     chmod +x \"$SCRIPT_DIR\"/*.sh\n\
     for unit in \"$SCRIPT_DIR\"/*.service \"$SCRIPT_DIR\"/*.timer; do\n  \
     [ -f \"$unit\" ] || continue\n  \
     name=$(basename \"$unit\")\n  \
     sed \"s|__WRAPPER_DIR__|$SCRIPT_DIR|g\" \"$unit\" > \"$DEST/$name\"\n\
     done\n\
     systemctl --user daemon-reload\n\
     for timer in symeraseme-tick symeraseme-poll symeraseme-rescan; do\n  \
     systemctl --user enable --now \"$timer.timer\"\n\
     done\n\
     printf '%s\\n' 'Systemd timers installed and enabled.'\n"
}

fn uninstall_script() -> &'static str {
    "#!/usr/bin/env bash\n\
     set -euo pipefail\n\
     SCRIPT_DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
     DEST=\"${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user\"\n\
     for timer in symeraseme-tick symeraseme-poll symeraseme-rescan; do\n  \
     systemctl --user disable --now \"$timer.timer\" 2>/dev/null || true\n\
     done\n\
     for unit in \"$SCRIPT_DIR\"/*.service \"$SCRIPT_DIR\"/*.timer; do\n  \
     [ -f \"$unit\" ] || continue\n  \
     rm -f \"$DEST/$(basename \"$unit\")\"\n\
     done\n\
     systemctl --user daemon-reload\n\
     printf '%s\\n' 'Systemd timers uninstalled.'\n"
}
