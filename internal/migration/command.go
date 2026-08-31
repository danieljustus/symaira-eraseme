package migration

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"

	"github.com/spf13/cobra"

	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

// NewCommand returns the registration-compatible `symeraseme migrate`
// command. Explicit source and destination flags are required so an operator
// cannot accidentally rewrite the live installation while learning the
// migration workflow.
func NewCommand() *cobra.Command {
	var (
		source       string
		destination  string
		backup       string
		home         string
		sourceConfig string
		destConfig   string
		platform     string
		binaryPath   string
		projectDir   string
		copySecrets  bool
		dryRun       bool
		jsonOutput   bool
	)
	cmd := &cobra.Command{
		Use:   "migrate",
		Short: "Migrate a Python-era installation to the Go layout",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			if source == "" || destination == "" {
				return errors.New("migrate requires --source and --destination; use --dry-run first")
			}
			if home == "" {
				var err error
				home, err = os.UserHomeDir()
				if err != nil {
					return fmt.Errorf("resolve home directory: %w", err)
				}
			}
			opts := Options{
				SourceRoot:            source,
				DestinationRoot:       destination,
				SourceConfigRoot:      sourceConfig,
				DestinationConfigRoot: destConfig,
				BackupDir:             backup,
				HomeDir:               home,
				Platform:              schedulerPlatform(platform),
				BinaryPath:            binaryPath,
				ProjectDir:            projectDir,
				CopySecrets:           copySecrets,
				DryRun:                dryRun,
			}
			report, err := Run(context.Background(), opts)
			if report != nil {
				if jsonOutput {
					if writeErr := writeJSON(cmd.OutOrStdout(), report); writeErr != nil {
						return writeErr
					}
				} else {
					writeText(cmd.OutOrStdout(), report)
				}
			}
			return err
		},
	}
	cmd.Flags().StringVar(&source, "source", "", "Python-era installation data/config directory (required)")
	cmd.Flags().StringVar(&destination, "destination", "", "separate Go destination directory (required)")
	cmd.Flags().StringVar(&sourceConfig, "source-config", "", "optional separate Python config/profile directory")
	cmd.Flags().StringVar(&destConfig, "destination-config", "", "optional separate Go config/profile directory")
	cmd.Flags().StringVar(&backup, "backup", "", "backup directory (default: <destination>.migration-backup)")
	cmd.Flags().StringVar(&home, "home", "", "synthetic or operator-selected home for native scheduler units")
	cmd.Flags().StringVar(&platform, "platform", "", "scheduler platform: cron, launchd, or systemd")
	cmd.Flags().StringVar(&binaryPath, "binary", "", "absolute symeraseme binary path for generated schedules")
	cmd.Flags().StringVar(&projectDir, "project-dir", "", "project directory for generated schedule wrappers")
	cmd.Flags().BoolVar(&copySecrets, "copy-secrets", false, "copy secrets through an injected SecretStore (never enabled by default)")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "report detected items without creating backups or changing files")
	cmd.Flags().BoolVar(&jsonOutput, "json", false, "write the migration report as JSON")
	return cmd
}

func schedulerPlatform(value string) scheduler.Platform {
	return scheduler.Platform(value)
}

func writeJSON(w io.Writer, report *Report) error {
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	return enc.Encode(report)
}

func writeText(w io.Writer, report *Report) {
	fmt.Fprintln(w, report.Detection.Summary)
	if report.BackupDir != "" {
		fmt.Fprintf(w, "Backup: %s\n", report.BackupDir)
	}
	for _, item := range report.Items {
		fmt.Fprintf(w, "[%s] %s\n", item.Status, item.Artifact.ID)
	}
	for _, warning := range report.Warnings {
		fmt.Fprintf(w, "Warning: %s\n", warning)
	}
	if report.DryRun {
		fmt.Fprintln(w, "Dry run: no files were changed.")
	} else if report.Complete {
		fmt.Fprintln(w, "Migration complete; the Python source was retained.")
	}
}
