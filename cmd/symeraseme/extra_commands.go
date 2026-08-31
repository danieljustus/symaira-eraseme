package main

import (
	"context"
	"fmt"

	"github.com/danieljustus/symaira-eraseme/internal/mcp"
	"github.com/spf13/cobra"
)

func realRedactFileCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "review", RunE: func(cmd *cobra.Command, args []string) error {
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
	}}
	var path string
	cmd.Flags().StringVar(&path, "path", "", "The path to the file to redact")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["path"] = path
		return nil
	}
	return cmd
}

func realPollInboxCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "poll-inbox", RunE: func(cmd *cobra.Command, args []string) error {
		res, err := mcp.ContractHandler()(context.Background(), "poll_inbox", argsMap)
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
	var host string
	cmd.Flags().StringVar(&host, "host", "", "IMAP server hostname")
	var port int
	cmd.Flags().IntVar(&port, "port", 0, "IMAP server port")
	var username string
	cmd.Flags().StringVar(&username, "username", "", "IMAP username (email address)")
	var since_days int
	cmd.Flags().IntVar(&since_days, "since-days", 0, "Fetch messages from the last N days")
	var ssl bool
	cmd.Flags().BoolVar(&ssl, "ssl", true, "Use SSL/TLS connection")
	var campaign_id string
	cmd.Flags().StringVar(&campaign_id, "campaign-id", "", "Filter by campaign")
	var folders string
	cmd.Flags().StringVar(&folders, "folders", "", "IMAP folders to poll (default: ['INBOX']). Deduplicates by Message-ID across folders.")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["host"] = host
		argsMap["port"] = port
		argsMap["username"] = username
		argsMap["since_days"] = since_days
		argsMap["ssl"] = ssl
		argsMap["campaign_id"] = campaign_id
		argsMap["folders"] = folders
		return nil
	}
	return cmd
}

func realClassifyReplyCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "classify-reply", RunE: func(cmd *cobra.Command, args []string) error {
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
	cmd := &cobra.Command{Use: "generate-rebuttal", RunE: func(cmd *cobra.Command, args []string) error {
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
	cmd.Flags().BoolVar(&save, "save", true, "Save rebuttal event to database")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
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
	cmd.Flags().StringVar(&output, "output", "", "Output file path")
	var auto_open bool
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
	cmd.Flags().StringVar(&format, "format", "", "Report format")
	var output string
	cmd.Flags().StringVar(&output, "output", "", "Output file path")
	var all_campaigns bool
	cmd.Flags().BoolVar(&all_campaigns, "all-campaigns", false, "Include all campaigns")
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
	cmd := &cobra.Command{Use: "show", RunE: func(cmd *cobra.Command, args []string) error {
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
		argsMap["task_id"] = task_id
		return nil
	}
	return cmd
}

func realManualTasksCompleteCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "complete", RunE: func(cmd *cobra.Command, args []string) error {
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
	cmd := &cobra.Command{Use: "run-web-form", RunE: func(cmd *cobra.Command, args []string) error {
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
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
	}}
	var broker_id string
	cmd.Flags().StringVar(&broker_id, "broker-id", "", "Broker identifier")
	var headed bool
	cmd.Flags().BoolVar(&headed, "headed", false, "Run browser in headed mode (visible)")
	var screenshot_dir string
	cmd.Flags().StringVar(&screenshot_dir, "screenshot-dir", "", "Directory for screenshots")
	var dry_run bool
	cmd.Flags().BoolVar(&dry_run, "dry-run", false, "Preview without running")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["broker_id"] = broker_id
		argsMap["headed"] = headed
		argsMap["screenshot_dir"] = screenshot_dir
		argsMap["dry_run"] = dry_run
		return nil
	}
	return cmd
}

func realAutoConfirmCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "auto-confirm", RunE: func(cmd *cobra.Command, args []string) error {
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
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "success\n")
		return err
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
		argsMap["request_id"] = request_id
		argsMap["headed"] = headed
		argsMap["screenshot_dir"] = screenshot_dir
		argsMap["dry_run"] = dry_run
		return nil
	}
	return cmd
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
	cmd := &cobra.Command{Use: "show", RunE: func(cmd *cobra.Command, args []string) error {
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
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
		argsMap["weeks"] = weeks
		argsMap["campaign_id"] = campaign_id
		return nil
	}
	return cmd
}

func realGrantCommand() *cobra.Command {
	argsMap := make(map[string]any)
	cmd := &cobra.Command{Use: "grant", RunE: func(cmd *cobra.Command, args []string) error {
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
	cmd.Flags().StringVar(&command, "command", "", "Command to grant consent for")
	var ttl int
	cmd.Flags().IntVar(&ttl, "ttl", 86400, "Token time-to-live in seconds")
	var revoke string
	cmd.Flags().StringVar(&revoke, "revoke", "", "Token value to revoke")
	var revoke_all bool
	cmd.Flags().BoolVar(&revoke_all, "revoke-all", false, "Revoke all active tokens")
	var list_tokens bool
	cmd.Flags().BoolVar(&list_tokens, "list-tokens", false, "List all active tokens")
	var dry_run bool
	cmd.Flags().BoolVar(&dry_run, "dry-run", false, "Preview without issuing or revoking")
	cmd.PreRunE = func(cmd *cobra.Command, args []string) error {
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
	return &cobra.Command{Use: "init-profile", RunE: func(cmd *cobra.Command, args []string) error { return nil }}
}
func realShowProfileCommand() *cobra.Command {
	return &cobra.Command{Use: "show-profile", RunE: func(cmd *cobra.Command, args []string) error { return nil }}
}
func realRenderTemplateCommand() *cobra.Command {
	return &cobra.Command{Use: "render-template", RunE: func(cmd *cobra.Command, args []string) error { return nil }}
}

func realScheduleCommand() *cobra.Command {
	schedule := commandGroup("schedule")
	schedule.AddCommand(realScheduleInstallCommand(), realScheduleUninstallCommand(), realScheduleStatusCommand())
	return schedule
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
	mt := commandGroup("manual-tasks")
	mt.AddCommand(realManualTasksListCommand(), realManualTasksShowCommand(), realManualTasksCompleteCommand(), realManualTasksCleanupCommand())
	return mt
}

func realStatusCommand() *cobra.Command {
	// this is a root status command, wait, in Python it was just `symeraseme status`
	// but there is also `plan status`. But wait, in command_surface.go there is a root "status".
	return &cobra.Command{Use: "status", RunE: func(cmd *cobra.Command, args []string) error { return nil }}
}
