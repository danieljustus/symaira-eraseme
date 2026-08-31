package main

import (
	"encoding/json"
	"fmt"

	"github.com/spf13/cobra"

	"github.com/danieljustus/symaira-eraseme/internal/config"
)

// addCommandSurface registers the stable command vocabulary. Commands whose
// backend has not landed yet remain explicit and fail closed rather than
// silently pretending to have performed an operation.
func addCommandSurface(root *cobra.Command) {
	root.AddCommand(
		realPlanCommand(),
		realBrokersCommand(),
		realRegistryCommand(),
		tickCommandWith(new(bool)),
	)
	schedule := commandGroup("schedule")
	schedule.AddCommand(realScheduleInstallCommand(), realScheduleUninstallCommand(), realScheduleStatusCommand())
	root.AddCommand(schedule)
	config := commandGroup("config")
	config.AddCommand(configShowCommand())
	root.AddCommand(config)

	root.AddCommand(
		realInitProfileCommand(), realShowProfileCommand(), realRenderTemplateCommand(),
		realGrantCommand(), realStatusCommand(), realGetCalendarCommand(),
		realGetDashboardDataCommand(), realGenerateReportCommand(), realGenerateDashboardCommand(),
		realGenerateSchedulerCommand(), realRequestsCommand(), realEventsCommand(),
		realManualTasksCommand(), realRedactFileCommand(), realPollInboxCommand(), realClassifyReplyCommand(),
		realGenerateRebuttalCommand(), realAutoConfirmCommand(), realRunWebFormCommand(),
	)
	root.AddCommand(realScheduleCommand())

}

func commandGroup(name string) *cobra.Command {
	return &cobra.Command{Use: name, Short: fmt.Sprintf("%s commands", name)}
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
