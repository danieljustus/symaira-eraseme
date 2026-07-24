"""MCP tool registry — the ``tools/list`` catalogue."""

from __future__ import annotations

from typing import Any

TOOL_DEFS: list[dict[str, Any]] = [
    # -- PII Redaction --------------------------------------------------------
    {
        "name": "redact_file",
        "description": (
            "Reads a file, runs PII redaction on it, and returns the redacted content."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to redact",
                },
            },
            "required": ["path"],
        },
    },
    # -- Campaign Planning ----------------------------------------------------
    {
        "name": "plan_create",
        "description": (
            "Create a removal campaign plan selecting brokers by jurisdiction and law."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "campaign_id": {
                    "type": "string",
                    "description": "Campaign identifier",
                },
                "jurisdiction": {
                    "type": "string",
                    "description": "Filter by jurisdiction (e.g. GDPR, CCPA)",
                },
                "law": {
                    "type": "string",
                    "description": "Filter by specific law",
                },
                "priority": {
                    "type": "string",
                    "description": "Filter by priority level",
                },
                "max_brokers": {
                    "type": "integer",
                    "description": "Maximum number of brokers to include",
                    "default": 30,
                },
            },
            "required": ["campaign_id"],
        },
    },
    {
        "name": "plan_show",
        "description": "Show the current removal campaign plan.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "campaign_id": {
                    "type": "string",
                    "description": "Campaign identifier",
                },
                "status": {
                    "type": "string",
                    "description": "Filter by request status",
                },
            },
        },
    },
    {
        "name": "execute",
        "description": ("Execute a removal campaign by sending opt-out requests in batches."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "campaign_id": {
                    "type": "string",
                    "description": "Campaign identifier",
                },
                "account": {
                    "type": "string",
                    "description": "Email account name (himalaya backend)",
                },
                "batch_size": {
                    "type": "integer",
                    "description": "Number of requests per batch",
                    "default": 5,
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview actions without sending",
                    "default": False,
                },
                "backend": {
                    "type": "string",
                    "enum": ["smtp", "himalaya"],
                    "description": "Email backend to use",
                },
                "concurrent": {
                    "type": "boolean",
                    "description": "Use concurrent execution",
                    "default": False,
                },
                "workers": {
                    "type": "integer",
                    "description": "Number of concurrent workers",
                    "default": 3,
                },
                "consent_token": {
                    "type": "string",
                    "description": "Consent token value for destructive operations",
                },
                "consent_file": {
                    "type": "string",
                    "description": "Path to consent token file",
                },
            },
            "required": ["campaign_id"],
        },
    },
    # -- Inbox Triage ---------------------------------------------------------
    {
        "name": "poll_inbox",
        "description": ("Poll IMAP inbox for broker replies and match them to removal requests."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "host": {
                    "type": "string",
                    "description": "IMAP server hostname",
                },
                "port": {
                    "type": "integer",
                    "description": "IMAP server port",
                },
                "username": {
                    "type": "string",
                    "description": "IMAP username (email address)",
                },
                "since_days": {
                    "type": "integer",
                    "description": "Fetch messages from the last N days",
                },
                "ssl": {
                    "type": "boolean",
                    "description": "Use SSL/TLS connection",
                    "default": True,
                },
                "campaign_id": {
                    "type": "string",
                    "description": "Filter by campaign",
                },
                "folders": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": (
                        "IMAP folders to poll (default: ['INBOX']). "
                        "Deduplicates by Message-ID across folders."
                    ),
                },
            },
            "required": ["host", "port", "username", "since_days", "ssl"],
        },
    },
    {
        "name": "classify_reply",
        "description": (
            "Classify a broker reply using LLM (e.g. confirmation, rejection, info request)."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "request_id": {
                    "type": "integer",
                    "description": "Removal request ID",
                },
                "provider": {
                    "type": "string",
                    "description": "LLM provider override",
                },
                "model": {
                    "type": "string",
                    "description": "LLM model override",
                },
                "save": {
                    "type": "boolean",
                    "description": "Save classification to database",
                    "default": True,
                },
            },
            "required": ["request_id"],
        },
    },
    {
        "name": "generate_rebuttal",
        "description": ("Generate a jurisdiction-aware rebuttal for a broker rejection."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "request_id": {
                    "type": "integer",
                    "description": "Removal request ID",
                },
                "provider": {
                    "type": "string",
                    "description": "LLM provider override",
                },
                "model": {
                    "type": "string",
                    "description": "LLM model override",
                },
                "save": {
                    "type": "boolean",
                    "description": "Save rebuttal event to database",
                    "default": True,
                },
            },
            "required": ["request_id"],
        },
    },
    # -- Reporting ------------------------------------------------------------
    {
        "name": "generate_dashboard",
        "description": "Generate an HTML dashboard with campaign analytics.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "output": {
                    "type": "string",
                    "description": "Output file path",
                    "default": "report.html",
                },
                "auto_open": {
                    "type": "boolean",
                    "description": "Open dashboard in browser after generation",
                    "default": False,
                },
                "auto_refresh": {
                    "type": "integer",
                    "description": "Auto-refresh interval in seconds (0 = disabled)",
                    "default": 0,
                },
            },
        },
    },
    {
        "name": "generate_report",
        "description": ("Generate a campaign report in HTML, JSON, or CSV format."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "campaign_id": {
                    "type": "string",
                    "description": "Campaign identifier",
                },
                "format": {
                    "type": "string",
                    "enum": ["html", "json", "csv"],
                    "description": "Report format",
                    "default": "html",
                },
                "output": {
                    "type": "string",
                    "description": "Output file path",
                },
                "all_campaigns": {
                    "type": "boolean",
                    "description": "Include all campaigns",
                    "default": False,
                },
            },
        },
    },
    # -- Manual Tasks ---------------------------------------------------------
    {
        "name": "manual_tasks_list",
        "description": ("List manual fallback tasks for forms that could not be automated."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Filter by task status",
                },
                "request_id": {
                    "type": "integer",
                    "description": "Filter by request ID",
                },
            },
        },
    },
    {
        "name": "manual_tasks_show",
        "description": "Show details of a specific manual task.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "integer",
                    "description": "Manual task ID",
                },
            },
            "required": ["task_id"],
        },
    },
    {
        "name": "manual_tasks_complete",
        "description": "Mark a manual task as completed.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "integer",
                    "description": "Manual task ID",
                },
                "notes": {
                    "type": "string",
                    "description": "Completion notes",
                    "default": "",
                },
            },
            "required": ["task_id"],
        },
    },
    {
        "name": "manual_tasks_cleanup",
        "description": ("Remove old screenshot and HTML snapshot files from manual tasks."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without deleting",
                    "default": False,
                },
            },
        },
    },
    # -- Scheduler ------------------------------------------------------------
    {
        "name": "generate_scheduler",
        "description": ("Generate cron, launchd, or systemd scheduler configurations."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "platform": {
                    "type": "string",
                    "enum": ["cron", "launchd", "systemd"],
                    "description": "Target platform (auto-detected if omitted)",
                },
                "output_dir": {
                    "type": "string",
                    "description": "Output directory for config files",
                    "default": "./schedules",
                },
                "tick_hour": {
                    "type": "integer",
                    "description": "Hour to run tick engine",
                    "default": 10,
                },
                "tick_minute": {
                    "type": "integer",
                    "description": "Minute to run tick engine",
                    "default": 0,
                },
                "poll_hours": {
                    "type": "string",
                    "description": "Comma-separated hours for inbox polling",
                    "default": "8,12,16,20",
                },
                "project_dir": {
                    "type": "string",
                    "description": "Project directory path",
                },
                "symeraseme_bin": {
                    "type": "string",
                    "description": "Path to symeraseme binary",
                },
                "venv_activate": {
                    "type": "string",
                    "description": "Virtualenv activate script path",
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without writing files",
                    "default": False,
                },
            },
        },
    },
    {
        "name": "schedule_install",
        "description": "Generate and install scheduler configurations.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "platform": {
                    "type": "string",
                    "enum": ["cron", "launchd", "systemd"],
                    "description": "Target platform (auto-detected if omitted)",
                },
                "tick_hour": {
                    "type": "integer",
                    "description": "Hour to run tick engine",
                    "default": 10,
                },
                "tick_minute": {
                    "type": "integer",
                    "description": "Minute to run tick engine",
                    "default": 0,
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without installing",
                    "default": False,
                },
            },
        },
    },
    {
        "name": "schedule_uninstall",
        "description": ("Get instructions for uninstalling scheduler configurations."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "platform": {
                    "type": "string",
                    "enum": ["cron", "launchd", "systemd"],
                    "description": "Target platform (auto-detected if omitted)",
                },
            },
        },
    },
    {
        "name": "schedule_status",
        "description": "Check status of installed scheduler services.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "platform": {
                    "type": "string",
                    "enum": ["cron", "launchd", "systemd"],
                    "description": "Target platform (auto-detected if omitted)",
                },
            },
        },
    },
    # -- Registry Validation --------------------------------------------------
    {
        "name": "validate",
        "description": (
            "Validate broker registry YAML files against the JSON Schema and Pydantic model."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "registry_dir": {
                    "type": "string",
                    "description": "Path to registry directory",
                },
            },
        },
    },
    # -- Web Forms ------------------------------------------------------------
    {
        "name": "run_web_form",
        "description": ("Run a broker web-form opt-out via Playwright browser automation."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "broker_id": {
                    "type": "string",
                    "description": "Broker identifier",
                },
                "headed": {
                    "type": "boolean",
                    "description": "Run browser in headed mode (visible)",
                    "default": False,
                },
                "screenshot_dir": {
                    "type": "string",
                    "description": "Directory for screenshots",
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without running",
                    "default": False,
                },
            },
            "required": ["broker_id"],
        },
    },
    # -- Auto-Confirm ---------------------------------------------------------
    {
        "name": "auto_confirm",
        "description": "Auto-click confirmation links in broker reply emails.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "request_id": {
                    "type": "integer",
                    "description": "Removal request ID",
                },
                "headed": {
                    "type": "boolean",
                    "description": "Run browser in headed mode",
                    "default": False,
                },
                "screenshot_dir": {
                    "type": "string",
                    "description": "Directory for screenshots",
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without clicking",
                    "default": False,
                },
            },
            "required": ["request_id"],
        },
    },
    # -- Consent Tokens -------------------------------------------------------
    {
        "name": "grant",
        "description": ("Issue, revoke, or list consent tokens for destructive operations."),
        "inputSchema": {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command to grant consent for",
                    "default": "execute",
                },
                "ttl": {
                    "type": "integer",
                    "description": "Token time-to-live in seconds",
                    "default": 86400,
                },
                "revoke": {
                    "type": "string",
                    "description": "Token value to revoke",
                },
                "revoke_all": {
                    "type": "boolean",
                    "description": "Revoke all active tokens",
                    "default": False,
                },
                "list_tokens": {
                    "type": "boolean",
                    "description": "List all active tokens",
                    "default": False,
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview without issuing or revoking",
                    "default": False,
                },
            },
        },
    },
]

# Fast lookup set for tools/call validation
_TOOL_DEFS_MAP: dict[str, dict[str, Any]] = {t["name"]: t for t in TOOL_DEFS}
