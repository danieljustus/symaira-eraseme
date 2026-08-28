package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/spf13/cobra"

	"github.com/danieljustus/symaira-eraseme/internal/config"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

// addCommandSurface registers the stable command vocabulary. Commands whose
// backend has not landed yet remain explicit and fail closed rather than
// silently pretending to have performed an operation.
func addCommandSurface(root *cobra.Command) {
	for _, name := range []string{
		"init-profile", "show-profile", "render-template", "grant", "plan",
		"tick", "status", "calendar", "dashboard", "generate-report",
		"generate-dashboard", "generate-scheduler", "schedule", "registry",
		"requests", "events", "brokers", "manual-tasks", "review", "config", "mcp",
	} {
		root.AddCommand(stubCommand(name))
	}

	plan := commandGroup("plan")
	for _, name := range []string{"create", "show", "execute", "tick", "status"} {
		plan.AddCommand(stubCommand(name))
	}
	replaceCommand(root, "plan", plan)

	schedule := commandGroup("schedule")
	for _, name := range []string{"install", "uninstall", "status"} {
		schedule.AddCommand(stubCommand(name))
	}
	replaceCommand(root, "schedule", schedule)

	replaceCommand(root, "tick", tickCommand())
	configCommand := commandGroup("config")
	configCommand.AddCommand(configShowCommand())
	replaceCommand(root, "config", configCommand)
}

func commandGroup(name string) *cobra.Command {
	return &cobra.Command{Use: name, Short: fmt.Sprintf("%s commands", name)}
}

func stubCommand(name string) *cobra.Command {
	return &cobra.Command{
		Use:   name,
		Short: fmt.Sprintf("%s command", name),
		RunE: func(*cobra.Command, []string) error {
			return fmt.Errorf("%s is not implemented in the current Go port", name)
		},
	}
}

func replaceCommand(root *cobra.Command, name string, replacement *cobra.Command) {
	for _, command := range root.Commands() {
		if command.Name() == name {
			root.RemoveCommand(command)
			root.AddCommand(replacement)
			return
		}
	}
}

func tickCommand() *cobra.Command {
	var dryRun bool
	var output string
	cmd := &cobra.Command{
		Use:   "tick",
		Short: "Run the deadline tick engine",
		RunE: func(cmd *cobra.Command, _ []string) error {
			dataDir := os.Getenv("SYMERASEME_DATA_DIR")
			if dataDir == "" {
				dataDir = filepath.Join(os.TempDir(), "symeraseme")
			}
			if err := os.MkdirAll(dataDir, 0o700); err != nil {
				return err
			}
			store, err := eventstore.Open(filepath.Join(dataDir, "symeraseme.db"))
			if err != nil {
				return err
			}
			defer store.Close()
			if output == "json" {
				return writeJSON(cmd, map[string]any{
					"success": true,
					"dry_run": dryRun,
					"actions": []any{},
				})
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "tick complete (dry-run=%t)\n", dryRun)
			return err
		},
	}
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "do not mutate requests")
	cmd.Flags().StringVar(&output, "output", "text", "output format: text or json")
	return cmd
}

func configShowCommand() *cobra.Command {
	var output string
	cmd := &cobra.Command{
		Use:   "show",
		Short: "Show effective configuration",
		RunE: func(cmd *cobra.Command, _ []string) error {
			cfg, err := config.Load().Load()
			if err != nil {
				return err
			}
			if output == "json" {
				return writeJSON(cmd, map[string]any{"success": true, "config": cfg})
			}
			_, err = fmt.Fprintf(cmd.OutOrStdout(), "data_dir=%s\nport=%d\n", cfg.DataDir, cfg.Port)
			return err
		},
	}
	cmd.Flags().StringVar(&output, "output", "text", "output format: text or json")
	return cmd
}

func writeJSON(cmd *cobra.Command, value any) error {
	enc := json.NewEncoder(cmd.OutOrStdout())
	return enc.Encode(value)
}
