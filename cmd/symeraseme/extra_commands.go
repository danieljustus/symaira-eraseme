package main

import (
	"context"
	"errors"
	"fmt"
	"net/mail"
	"strconv"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/confirmation"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
	"github.com/danieljustus/symaira-eraseme/internal/reporting"
	"github.com/danieljustus/symaira-eraseme/internal/templating"
	"github.com/spf13/cobra"
)

func newCompletionCommand() *cobra.Command {
	return &cobra.Command{
		Use:       "completion [bash|zsh|fish|powershell]",
		Short:     "Generate shell completion scripts",
		Args:      cobra.ExactArgs(1),
		ValidArgs: []string{"bash", "zsh", "fish", "powershell"},
		RunE: func(cmd *cobra.Command, args []string) error {
			switch args[0] {
			case "bash":
				return cmd.Root().GenBashCompletion(cmd.OutOrStdout())
			case "zsh":
				return cmd.Root().GenZshCompletion(cmd.OutOrStdout())
			case "fish":
				return cmd.Root().GenFishCompletion(cmd.OutOrStdout(), true)
			case "powershell":
				return cmd.Root().GenPowerShellCompletion(cmd.OutOrStdout())
			default:
				return fmt.Errorf("unsupported shell %q", args[0])
			}
		},
	}
}

func intArgument(args []string, fallback int, name string) (int, error) {
	if len(args) == 0 {
		return fallback, nil
	}
	value, err := strconv.Atoi(args[0])
	if err != nil {
		return 0, fmt.Errorf("invalid %s %q", name, args[0])
	}
	return value, nil
}

func realRedactFileCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{
		Use:  "review FILE",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if len(args) == 1 {
				argsMap["path"] = args[0]
			}
			if path, _ := argsMap["path"].(string); path == "" {
				return fmt.Errorf("a file path is required")
			}
			res, err := mcp.ContractHandler()(context.Background(), "redact_file", argsMap)
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, res)
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
			return err
		},
	}
	var path string
	cmd.Flags().StringVar(&path, "path", "", "The path to the file to redact")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["path"] = path
		return nil
	}
	return cmd
}

var pollInboxHandler = func() mcp.Handler { return mcp.ContractHandler() }

func realPollInboxCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "poll-inbox", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := pollInboxHandler()(context.Background(), "poll_inbox", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		if m, ok := res.(map[string]any); ok {
			if msg, ok := m["message"].(string); ok && msg != "" {
				_, err = fmt.Fprintln(cmd.OutOrStdout(), msg)
				return err
			}
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var host string
	cmd.Flags().StringVar(&host, "host", "imap.gmail.com", "IMAP server hostname")
	var port int
	cmd.Flags().IntVar(&port, "port", 993, "IMAP server port")
	var username string
	cmd.Flags().StringVar(&username, "username", "", "IMAP username (email address)")
	var oauth2AccessToken string
	cmd.Flags().StringVar(&oauth2AccessToken, "oauth2-access-token", "", "IMAP OAuth2 access token or secret reference (never printed)")
	var oauth2Username string
	cmd.Flags().StringVar(&oauth2Username, "oauth2-username", "", "IMAP OAuth2 username (defaults to --username)")
	var since_days int
	cmd.Flags().IntVar(&since_days, "since-days", 14, "Fetch messages from the last N days")
	cmd.Flags().IntVar(&since_days, "since", 14, "Fetch messages from the last N days")
	var ssl bool
	cmd.Flags().BoolVar(&ssl, "ssl", true, "Use SSL/TLS connection")
	var campaign_id string
	cmd.Flags().StringVar(&campaign_id, "campaign-id", "", "Filter by campaign")
	var folders string
	cmd.Flags().StringVar(&folders, "folders", "", "IMAP folders to poll (default: ['INBOX']). Deduplicates by Message-ID across folders.")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		if cmd.Flags().Changed("host") {
			argsMap["host"] = host
		}
		if cmd.Flags().Changed("port") {
			argsMap["port"] = port
		}
		if cmd.Flags().Changed("username") {
			argsMap["username"] = username
		}
		if cmd.Flags().Changed("oauth2-access-token") {
			argsMap["oauth2_access_token"] = oauth2AccessToken
		}
		if cmd.Flags().Changed("oauth2-username") {
			argsMap["oauth2_username"] = oauth2Username
		}
		if cmd.Flags().Changed("since-days") || cmd.Flags().Changed("since") {
			argsMap["since_days"] = since_days
		}
		if cmd.Flags().Changed("ssl") {
			argsMap["ssl"] = ssl
		}
		if cmd.Flags().Changed("campaign-id") {
			argsMap["campaign_id"] = campaign_id
		}
		if cmd.Flags().Changed("folders") {
			argsMap["folders"] = folders
		}
		return nil
	}
	return cmd
}

func realClassifyReplyCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{
		Use:  "classify-reply REQUEST_ID",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			res, err := mcp.ContractHandler()(context.Background(), "classify_reply", argsMap)
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, res)
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
			return err
		}}
	var request_id int
	cmd.Flags().IntVar(&request_id, "request-id", 0, "Removal request ID")
	var provider string
	cmd.Flags().StringVar(&provider, "provider", "", "LLM provider override")
	var model string
	cmd.Flags().StringVar(&model, "model", "", "LLM model override")
	var save bool
	cmd.Flags().BoolVar(&save, "save", true, "Save classification to database")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		var err error
		request_id, err = intArgument(args, request_id, "request ID")
		if err != nil {
			return err
		}
		argsMap["request_id"] = request_id
		argsMap["provider"] = provider
		argsMap["model"] = model
		argsMap["save"] = save
		return nil
	}
	return cmd
}

func realGenerateRebuttalCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{
		Use:  "generate-rebuttal REQUEST_ID",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			res, err := mcp.ContractHandler()(context.Background(), "generate_rebuttal", argsMap)
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, res)
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
			return err
		}}
	var request_id int
	cmd.Flags().IntVar(&request_id, "request-id", 0, "Removal request ID")
	var provider string
	cmd.Flags().StringVar(&provider, "provider", "", "LLM provider override")
	var model string
	cmd.Flags().StringVar(&model, "model", "", "LLM model override")
	var save bool
	cmd.Flags().BoolVar(&save, "save", true, "Save rebuttal to database")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		var err error
		request_id, err = intArgument(args, request_id, "request ID")
		if err != nil {
			return err
		}
		argsMap["request_id"] = request_id
		argsMap["provider"] = provider
		argsMap["model"] = model
		argsMap["save"] = save
		return nil
	}
	return cmd
}

func realGenerateDashboardCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "generate-dashboard", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "generate_dashboard", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var output string
	cmd.Flags().StringVar(&output, "output", "report.html", "Output file path")
	var auto_open bool
	cmd.Flags().BoolVar(&auto_open, "open", false, "Open dashboard in browser after generation")
	cmd.Flags().BoolVar(&auto_open, "auto-open", false, "Open dashboard in browser after generation")
	var auto_refresh int
	cmd.Flags().IntVar(&auto_refresh, "auto-refresh", 0, "Auto-refresh interval in seconds (0 = disabled)")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["output"] = output
		argsMap["auto_open"] = auto_open
		argsMap["auto_refresh"] = auto_refresh
		return nil
	}
	return cmd
}

func realGenerateReportCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "generate-report", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "generate_report", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var campaign_id string
	cmd.Flags().StringVar(&campaign_id, "campaign-id", "", "Campaign identifier")
	var format string
	cmd.Flags().StringVar(&format, "format", "html", "Report format")
	var output string
	cmd.Flags().StringVar(&output, "output", "", "Output file path")
	var all_campaigns bool
	cmd.Flags().BoolVar(&all_campaigns, "all-campaigns", false, "Include all campaigns")
	cmd.Flags().BoolVar(&all_campaigns, "all", false, "Include all campaigns")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["campaign_id"] = campaign_id
		argsMap["format"] = format
		argsMap["output"] = output
		argsMap["all_campaigns"] = all_campaigns
		return nil
	}
	return cmd
}

func realManualTasksListCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "list", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "manual_tasks_list", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var status string
	cmd.Flags().StringVar(&status, "status", "", "Filter by task status")
	var request_id int
	cmd.Flags().IntVar(&request_id, "request-id", 0, "Filter by request ID")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["status"] = status
		argsMap["request_id"] = request_id
		return nil
	}
	return cmd
}

func realManualTasksShowCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{
		Use:  "show TASK_ID",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			res, err := mcp.ContractHandler()(context.Background(), "manual_tasks_show", argsMap)
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, res)
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
			return err
		}}
	var task_id int
	cmd.Flags().IntVar(&task_id, "task-id", 0, "Manual task ID")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		var err error
		task_id, err = intArgument(args, task_id, "task ID")
		if err != nil {
			return err
		}
		argsMap["task_id"] = task_id
		return nil
	}
	return cmd
}

func realManualTasksCompleteCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{
		Use:  "complete TASK_ID",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			res, err := mcp.ContractHandler()(context.Background(), "manual_tasks_complete", argsMap)
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, res)
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
			return err
		}}
	var task_id int
	cmd.Flags().IntVar(&task_id, "task-id", 0, "Manual task ID")
	var notes string
	cmd.Flags().StringVar(&notes, "notes", "", "Completion notes")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		var err error
		task_id, err = intArgument(args, task_id, "task ID")
		if err != nil {
			return err
		}
		argsMap["task_id"] = task_id
		argsMap["notes"] = notes
		return nil
	}
	return cmd
}

func realManualTasksCleanupCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "cleanup", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "manual_tasks_cleanup", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var dry_run bool
	cmd.Flags().BoolVar(&dry_run, "dry-run", false, "Preview without deleting")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["dry_run"] = dry_run
		return nil
	}
	return cmd
}

func realRequestsCommand() *cobra.Command {
	requests := commandGroup("requests")
	requests.AddCommand(realListRequestsCommand())
	return requests
}

func realEventsCommand() *cobra.Command {
	events := commandGroup("events")
	events.AddCommand(realGetEventsCommand())
	return events
}

func realManualTasksCommand() *cobra.Command {
	manualTasks := commandGroup("manual-tasks")
	manualTasks.AddCommand(realManualTasksListCommand(), realManualTasksShowCommand(), realManualTasksCompleteCommand(), realManualTasksCleanupCommand())
	return manualTasks
}

func realGenerateSchedulerCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "generate-scheduler", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "generate_scheduler", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var platform string
	cmd.Flags().StringVar(&platform, "platform", "", "Target platform (auto-detected if omitted)")
	var output_dir string
	cmd.Flags().StringVar(&output_dir, "output-dir", "", "Output directory for config files")
	var tick_hour int
	cmd.Flags().IntVar(&tick_hour, "tick-hour", 10, "Hour to run tick engine")
	var tick_minute int
	cmd.Flags().IntVar(&tick_minute, "tick-minute", 0, "Minute to run tick engine")
	var poll_hours string
	cmd.Flags().StringVar(&poll_hours, "poll-hours", "", "Comma-separated hours for inbox polling")
	var project_dir string
	cmd.Flags().StringVar(&project_dir, "project-dir", "", "Project directory path")
	var symeraseme_bin string
	cmd.Flags().StringVar(&symeraseme_bin, "symeraseme-bin", "", "Path to symeraseme binary")
	var venv_activate string
	cmd.Flags().StringVar(&venv_activate, "venv-activate", "", "Virtualenv activate script path")
	var dry_run bool
	cmd.Flags().BoolVar(&dry_run, "dry-run", false, "Preview without writing files")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["platform"] = platform
		argsMap["output_dir"] = output_dir
		argsMap["tick_hour"] = tick_hour
		argsMap["tick_minute"] = tick_minute
		argsMap["poll_hours"] = poll_hours
		argsMap["project_dir"] = project_dir
		argsMap["symeraseme_bin"] = symeraseme_bin
		argsMap["venv_activate"] = venv_activate
		argsMap["dry_run"] = dry_run
		return nil
	}
	return cmd
}

func realScheduleInstallCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "install", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "schedule_install", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var platform string
	cmd.Flags().StringVar(&platform, "platform", "", "Target platform (auto-detected if omitted)")
	var tick_hour int
	cmd.Flags().IntVar(&tick_hour, "tick-hour", 10, "Hour to run tick engine")
	var tick_minute int
	cmd.Flags().IntVar(&tick_minute, "tick-minute", 0, "Minute to run tick engine")
	var dry_run bool
	cmd.Flags().BoolVar(&dry_run, "dry-run", false, "Preview without installing")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["platform"] = platform
		argsMap["tick_hour"] = tick_hour
		argsMap["tick_minute"] = tick_minute
		argsMap["dry_run"] = dry_run
		return nil
	}
	return cmd
}

func realScheduleUninstallCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "uninstall", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "schedule_uninstall", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var platform string
	cmd.Flags().StringVar(&platform, "platform", "", "Target platform (auto-detected if omitted)")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["platform"] = platform
		return nil
	}
	return cmd
}

func realScheduleStatusCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "status", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "schedule_status", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var platform string
	cmd.Flags().StringVar(&platform, "platform", "", "Target platform (auto-detected if omitted)")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["platform"] = platform
		return nil
	}
	return cmd
}

func realRunWebFormCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{
		Use:  "run-web-form BROKER_ID",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			res, err := mcp.ContractHandler()(context.Background(), "run_web_form", argsMap)
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, res)
			}
			return writeWebActionText(cmd, res)
		}}
	var broker_id string
	cmd.Flags().StringVar(&broker_id, "broker-id", "", "Broker identifier")
	var request_id int
	cmd.Flags().IntVar(&request_id, "request-id", 0, "Optional removal request ID for manual task linking")
	var headed bool
	cmd.Flags().BoolVar(&headed, "headed", false, "Run browser in headed mode (visible)")
	var screenshot_dir string
	cmd.Flags().StringVar(&screenshot_dir, "screenshot-dir", "", "Directory for screenshots")
	var dry_run bool
	cmd.Flags().BoolVar(&dry_run, "dry-run", false, "Preview without running")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		if len(args) == 1 {
			broker_id = args[0]
		}
		argsMap["broker_id"] = broker_id
		argsMap["request_id"] = request_id
		argsMap["headed"] = headed
		argsMap["screenshot_dir"] = screenshot_dir
		argsMap["dry_run"] = dry_run
		return nil
	}
	return cmd
}

func realAutoConfirmCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{
		Use:  "auto-confirm REQUEST_ID",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			res, err := mcp.ContractHandler()(context.Background(), "auto_confirm", argsMap)
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, res)
			}
			return writeWebActionText(cmd, res)
		}}
	var request_id int
	cmd.Flags().IntVar(&request_id, "request-id", 0, "Removal request ID")
	var headed bool
	cmd.Flags().BoolVar(&headed, "headed", false, "Run browser in headed mode")
	var screenshot_dir string
	cmd.Flags().StringVar(&screenshot_dir, "screenshot-dir", "", "Directory for screenshots")
	var dry_run bool
	cmd.Flags().BoolVar(&dry_run, "dry-run", false, "Preview without clicking")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		var err error
		request_id, err = intArgument(args, request_id, "request ID")
		if err != nil {
			return err
		}
		argsMap["request_id"] = request_id
		argsMap["headed"] = headed
		argsMap["screenshot_dir"] = screenshot_dir
		argsMap["dry_run"] = dry_run
		return nil
	}
	return cmd
}

func writeWebActionText(cmd *cobra.Command, value any) error {
	switch result := value.(type) {
	case map[string]any:
		if success, _ := result["success"].(bool); success {
			_, err := fmt.Fprintln(cmd.OutOrStdout(), "success")
			return err
		}
		if result["status"] == "manual_action_required" {
			_, err := fmt.Fprintf(cmd.OutOrStdout(), "manual_action_required task_id=%v url=%v\n%v\n", result["task_id"], result["url"], result["instructions"])
			return err
		}
		_, err := fmt.Fprintf(cmd.OutOrStdout(), "not_completed: %v\n", result["error"])
		return err
	case confirmation.Result:
		if result.Success {
			_, err := fmt.Fprintln(cmd.OutOrStdout(), "success")
			return err
		}
		if result.ManualActionRequired || result.Step == "manual_confirmation_required" {
			_, err := fmt.Fprintf(cmd.OutOrStdout(), "manual_confirmation_required task_id=%d url=%s\n%s\n", result.TaskID, result.ClickedURL, result.Instructions)
			return err
		}
		_, err := fmt.Fprintf(cmd.OutOrStdout(), "not_completed step=%s: %s\n", result.Step, result.Error)
		return err
	default:
		return fmt.Errorf("unexpected web action result %T", value)
	}
}

func realGetDashboardDataCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "dashboard", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "get_dashboard_data", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		return nil
	}
	return cmd
}

func realListRequestsCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "list", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "list_requests", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var campaign_id string
	cmd.Flags().StringVar(&campaign_id, "campaign-id", "", "Filter by campaign identifier")
	var status string
	cmd.Flags().StringVar(&status, "status", "", "Filter by request status")
	var broker_id string
	cmd.Flags().StringVar(&broker_id, "broker-id", "", "Filter by broker identifier")
	var page int
	cmd.Flags().IntVar(&page, "page", 1, "1-indexed page number")
	var page_size int
	cmd.Flags().IntVar(&page_size, "page-size", 100, "Maximum items per page")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["campaign_id"] = campaign_id
		argsMap["status"] = status
		argsMap["broker_id"] = broker_id
		argsMap["page"] = page
		argsMap["page_size"] = page_size
		return nil
	}
	return cmd
}

func realGetEventsCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{
		Use:  "show REQUEST_ID",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			res, err := mcp.ContractHandler()(context.Background(), "get_events", argsMap)
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, res)
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
			return err
		}}
	var request_id int
	cmd.Flags().IntVar(&request_id, "request-id", 0, "Removal request ID")
	var after_event_id int
	cmd.Flags().IntVar(&after_event_id, "after-event-id", 0, "Only return events with ID greater than this")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		var err error
		request_id, err = intArgument(args, request_id, "request ID")
		if err != nil {
			return err
		}
		argsMap["request_id"] = request_id
		argsMap["after_event_id"] = after_event_id
		return nil
	}
	return cmd
}

func realGetCalendarCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "calendar", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "get_calendar", argsMap)
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, res)
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var weeks int
	cmd.Flags().IntVar(&weeks, "weeks", 4, "Number of weeks to look ahead")
	var campaign_id string
	cmd.Flags().StringVar(&campaign_id, "campaign-id", "", "Filter by campaign identifier")
	cmd.Flags().StringVar(&campaign_id, "campaign", "", "Filter by campaign identifier")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["weeks"] = weeks
		argsMap["campaign_id"] = campaign_id
		return nil
	}
	return cmd
}

func realGrantCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{
		Use:  "grant [COMMAND]",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			res, err := mcp.ContractHandler()(context.Background(), "grant", argsMap)
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, res)
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
			return err
		}}
	var command string
	cmd.Flags().StringVar(&command, "command", "execute", "Command to grant consent for")
	var ttl int
	cmd.Flags().IntVar(&ttl, "ttl", 86400, "Token time-to-live in seconds")
	var revoke string
	cmd.Flags().StringVar(&revoke, "revoke", "", "Token value to revoke")
	var revoke_all bool
	cmd.Flags().BoolVar(&revoke_all, "revoke-all", false, "Revoke all active tokens")
	var list_tokens bool
	cmd.Flags().BoolVar(&list_tokens, "list-tokens", false, "List all active tokens")
	cmd.Flags().BoolVar(&list_tokens, "list", false, "List all active tokens")
	var dry_run bool
	cmd.Flags().BoolVar(&dry_run, "dry-run", false, "Preview without issuing or revoking")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		if len(args) == 1 {
			command = args[0]
		}
		argsMap["command"] = command
		argsMap["ttl"] = ttl
		argsMap["revoke"] = revoke
		argsMap["revoke_all"] = revoke_all
		argsMap["list_tokens"] = list_tokens
		argsMap["dry_run"] = dry_run
		return nil
	}
	return cmd
}

func realInitProfileCommand() *cobra.Command {
	var fullName, email, profilePath string
	cmd := &cobra.Command{
		Use:   "init-profile",
		Short: "Create or replace the encrypted identity profile",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			fullName = strings.TrimSpace(fullName)
			email = strings.TrimSpace(email)
			if fullName == "" || email == "" {
				return fmt.Errorf("--full-name and --email are required")
			}
			parsed, err := mail.ParseAddress(email)
			if err != nil || parsed.Address != email {
				return fmt.Errorf("invalid email address")
			}
			path, err := identity.InitProfile(&identity.Profile{FullName: fullName, EmailAddresses: []string{email}}, profilePath)
			if err != nil {
				return err
			}
			result := map[string]any{"success": true, "profile_path": path}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, result)
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "identity profile saved at %s\n", path)
			return err
		},
	}
	cmd.Flags().StringVar(&fullName, "full-name", "", "Full name")
	cmd.Flags().StringVar(&email, "email", "", "Email address")
	cmd.Flags().StringVar(&profilePath, "profile", "", "Identity profile path")
	return cmd
}

func realShowProfileCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "show-profile",
		Short: "Show the encrypted identity profile",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			profile, err := identity.LoadProfile("")
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, profile)
			}
			if _, err = fmt.Fprintf(cmd.OutOrStdout(), "Name: %s\n", profile.FullName); err != nil {
				return err
			}
			for _, value := range profile.EmailAddresses {
				if _, err = fmt.Fprintf(cmd.OutOrStdout(), "Email: %s\n", value); err != nil {
					return err
				}
			}
			for _, value := range profile.Jurisdictions {
				if _, err = fmt.Fprintf(cmd.OutOrStdout(), "Jurisdiction: %s\n", value); err != nil {
					return err
				}
			}
			return nil
		},
	}
}

func realRenderTemplateCommand() *cobra.Command {
	var brokerName, brokerWebsite string
	cmd := &cobra.Command{
		Use:   "render-template TEMPLATE",
		Short: "Render a legal template",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			var profile *identity.Profile
			if identity.ProfileExists("") {
				var err error
				profile, err = identity.LoadProfile("")
				if err != nil {
					return err
				}
			}
			content, err := templating.Render(args[0], templating.RenderOpts{Profile: profile, BrokerName: brokerName, BrokerWebsite: brokerWebsite})
			if err != nil {
				return err
			}
			_, err = cmd.OutOrStdout().Write([]byte(content))
			return err
		},
	}
	cmd.Flags().StringVar(&brokerName, "broker-name", "", "Name of the data broker")
	cmd.Flags().StringVar(&brokerWebsite, "broker-website", "", "Broker website URL")
	return cmd
}

func realStatusCommand() *cobra.Command {
	var campaignID string
	cmd := &cobra.Command{
		Use:   "status",
		Short: "Show campaign status",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) (runErr error) {
			store, err := dataStore()
			if err != nil {
				return err
			}
			defer func() { runErr = errors.Join(runErr, store.Close()) }()
			result, err := reporting.GetCampaignStatus(context.Background(), store, campaignID, time.Now().UTC())
			if err != nil {
				return err
			}
			format, err := outputFormat(cmd)
			if err != nil {
				return err
			}
			if format == "json" {
				return writeJSON(cmd, result)
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "Total: %v\n", result["totals"])
			return err
		},
	}
	cmd.Flags().StringVar(&campaignID, "campaign", "", "Campaign identifier")
	return cmd
}
