package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"time"

	"github.com/spf13/cobra"

	_ "github.com/danieljustus/symaira-eraseme"
	"github.com/danieljustus/symaira-eraseme/internal/campaign"
	"github.com/danieljustus/symaira-eraseme/internal/config"
	"github.com/danieljustus/symaira-eraseme/internal/deadlines"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
	"github.com/danieljustus/symaira-eraseme/internal/reporting"
)

func outputFormat(cmd *cobra.Command) (string, error) {
	value, err := cmd.Root().PersistentFlags().GetString("output")
	if err != nil {
		return "text", err
	}
	if value != "text" && value != "json" {
		return "", fmt.Errorf("invalid output format %q: use text or json", value)
	}
	return value, nil
}

func dataStore() (*eventstore.Store, error) {
	storage, err := config.ResolveStorage()
	if err != nil {
		return nil, err
	}
	if err := os.MkdirAll(storage.DBDir, 0o700); err != nil {
		return nil, fmt.Errorf("eventstore: create database directory: %w", err)
	}
	if err := os.Chmod(storage.DBDir, 0o700); err != nil {
		return nil, fmt.Errorf("eventstore: secure database directory: %w", err)
	}
	requiresKey := storage.Encrypt
	if !requiresKey {
		requiresKey, err = eventstore.IsEncrypted(storage.DBPath)
		if err != nil {
			return nil, fmt.Errorf("eventstore: inspect database encryption: %w", err)
		}
	}
	if requiresKey {
		// Read-only bootstrap resolves an operator-provided/keyring key but
		// never mints one as a side effect of opening storage.
		if err := identity.BootstrapReadOnly(); err != nil {
			return nil, err
		}
	}
	return eventstore.OpenConfigured(storage.DBPath, storage.TempDir, storage.Encrypt)
}

func loadRegistry() ([]registry.Broker, error) {
	if dir := os.Getenv("SYMERASEME_RESOURCES"); dir != "" {
		return registry.LoadFromDir(dir)
	}
	return registry.LoadEmbedded()
}

func realPlanCommand() *cobra.Command {
	plan := commandGroup("plan")
	var campaignID, jurisdiction, law, priority, category, status, profilePath, notes string
	var maxBrokers int
	var includeInactive bool
	create := &cobra.Command{Use: "create", Short: "Create a removal campaign plan", RunE: func(cmd *cobra.Command, _ []string) (runErr error) {
		if campaignID == "" {
			return fmt.Errorf("--campaign is required")
		}
		brokers, err := loadRegistry()
		if err != nil {
			return err
		}
		store, err := dataStore()
		if err != nil {
			return err
		}
		defer func() { runErr = errors.Join(runErr, store.Close()) }()
		result, err := campaign.PlanCampaign(context.Background(), store, brokers, campaign.PlanOpts{CampaignID: campaignID, Jurisdiction: jurisdiction, Law: law, Priority: priority, Category: category, Status: status, IncludeInactive: includeInactive, MaxBrokers: maxBrokers, Notes: notes}, profilePath)
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
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "planned %d request(s) for campaign %s\n", result.Planned, result.CampaignID)
		return err
	}}
	create.Flags().StringVar(&campaignID, "campaign", "", "campaign identifier")
	create.Flags().StringVar(&jurisdiction, "jurisdiction", "", "jurisdiction filter")
	create.Flags().StringVar(&law, "law", "", "law filter")
	create.Flags().StringVar(&priority, "priority", "", "priority filter")
	create.Flags().StringVar(&category, "category", "", "category filter")
	create.Flags().StringVar(&status, "status", "active", "broker status filter")
	create.Flags().IntVar(&maxBrokers, "max", 30, "maximum brokers")
	create.Flags().BoolVar(&includeInactive, "include-inactive", false, "include inactive brokers")
	create.Flags().StringVar(&profilePath, "profile", "", "identity profile path")
	create.Flags().StringVar(&notes, "notes", "", "campaign notes")

	var showCampaign, showStatus string
	show := &cobra.Command{Use: "show", Short: "Show planned requests", RunE: func(cmd *cobra.Command, _ []string) (runErr error) {
		store, err := dataStore()
		if err != nil {
			return err
		}
		defer func() { runErr = errors.Join(runErr, store.Close()) }()
		result, err := campaign.GetPlan(context.Background(), eventstore.NewRepository(store), showCampaign, showStatus)
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
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "campaign %s: %v request(s)\n", result["campaign_id"], result["total"])
		return err
	}}
	show.Flags().StringVar(&showCampaign, "campaign", "", "campaign identifier")
	show.Flags().StringVar(&showStatus, "status", "", "request status filter")

	var executeCampaignID string
	var batchSize int
	var executeAccount, consentToken, consentFile string
	var executeDryRun bool
	execute := &cobra.Command{Use: "execute", Short: "Execute planned requests", RunE: func(cmd *cobra.Command, _ []string) (runErr error) {
		if executeCampaignID == "" {
			return fmt.Errorf("--campaign is required")
		}
		store, err := dataStore()
		if err != nil {
			return err
		}
		defer func() { runErr = errors.Join(runErr, store.Close()) }()
		if !executeDryRun {
			if err := identity.ConsentGate("execute", identity.ConsentOptions{
				ConsentToken:      consentToken,
				ConsentFile:       consentFile,
				Interactive:       false,
				ConsentEnvVar:     "SYMERASEME_CONSENT",
				ConsentFileEnvVar: "SYMERASEME_CONSENT_FILE",
			}); err != nil {
				return err
			}
		}
		brokers, err := loadRegistry()
		if err != nil {
			return err
		}
		webForm := campaign.NewWebFormAdapter(brokers, nil)
		if !executeDryRun {
			webForm = campaign.NewWebFormAdapterWithStore(store, brokers, nil)
			webForm.DeferManualTask = true
		}
		result, err := campaign.ExecuteCampaign(context.Background(), store, executeCampaignID, campaign.ExecuteOpts{
			Account: executeAccount,
			DryRun:  executeDryRun,
			WebForm: webForm.Run,
		}, batchSize)
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
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "executed %v request(s)\n", result["batch_size"])
		return err
	}}
	execute.Flags().StringVar(&executeCampaignID, "campaign", "", "campaign identifier")
	execute.Flags().IntVar(&batchSize, "batch-size", 5, "maximum requests to execute")
	execute.Flags().StringVar(&executeAccount, "account", "", "email account name")
	execute.Flags().StringVar(&consentToken, "consent", "", "consent token for destructive execution")
	execute.Flags().StringVar(&consentFile, "consent-file", "", "path to consent token file")
	execute.Flags().BoolVar(&executeDryRun, "dry-run", false, "preview without sending")

	plan.AddCommand(create, show, execute, planTickCommand(), planStatusCommand())
	return plan
}

func planTickCommand() *cobra.Command { var dryRun bool; return tickCommandWith(&dryRun) }
func planStatusCommand() *cobra.Command {
	var campaignID string
	return &cobra.Command{Use: "status", Short: "Show campaign status", RunE: func(cmd *cobra.Command, _ []string) (runErr error) {
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
	}}
}

func tickCommandWith(dryRun *bool) *cobra.Command {
	cmd := &cobra.Command{Use: "tick", Short: "Run the deadline tick engine", RunE: func(cmd *cobra.Command, _ []string) (runErr error) {
		store, err := dataStore()
		if err != nil {
			return err
		}
		defer func() { runErr = errors.Join(runErr, store.Close()) }()
		actions, err := deadlines.RunTick(context.Background(), eventstore.NewRepository(store), deadlines.RunOpts{DryRun: *dryRun})
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, map[string]any{"success": true, "dry_run": *dryRun, "actions": actions})
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "tick complete: %d action(s)\n", len(actions))
		return err
	}}
	cmd.Flags().BoolVar(dryRun, "dry-run", false, "do not mutate requests")
	return cmd
}

func realBrokersCommand() *cobra.Command {
	brokers := commandGroup("brokers")
	var priority, jurisdiction, law, status string
	var includeDisabled, includeInactive bool
	list := &cobra.Command{Use: "list", Short: "List brokers", RunE: func(cmd *cobra.Command, _ []string) error {
		items, err := loadRegistry()
		if err != nil {
			return err
		}
		items = registry.FilterBrokers(items, registry.BrokerFilter{Priority: priority, Jurisdiction: jurisdiction, Law: law, Status: status, IncludeDisabled: includeDisabled, IncludeInactive: includeInactive})
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, map[string]any{"schema_version": 1, "count": len(items), "brokers": items, "filters": map[string]any{"include_disabled": includeDisabled, "status": status}})
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "%d broker(s)\n", len(items))
		return err
	}}
	list.Flags().StringVar(&priority, "priority", "", "priority filter")
	list.Flags().StringVar(&jurisdiction, "jurisdiction", "", "jurisdiction filter")
	list.Flags().StringVar(&law, "law", "", "law filter")
	list.Flags().StringVar(&status, "status", "active", "status filter")
	list.Flags().BoolVar(&includeDisabled, "include-disabled", false, "include disabled brokers")
	list.Flags().BoolVar(&includeInactive, "include-inactive", false, "include inactive brokers")
	show := &cobra.Command{Use: "show BROKER_ID", Short: "Show one broker", Args: cobra.ExactArgs(1), RunE: func(cmd *cobra.Command, args []string) error {
		items, err := loadRegistry()
		if err != nil {
			return err
		}
		for _, item := range items {
			if item.ID == args[0] {
				format, err := outputFormat(cmd)
				if err != nil {
					return err
				}
				if format == "json" {
					return writeJSON(cmd, map[string]any{"schema_version": 1, "broker": item})
				}
				_, err = fmt.Fprintf(cmd.OutOrStdout(), "%s (%s)\n", item.Name, item.ID)
				return err
			}
		}
		return fmt.Errorf("broker %q not found", args[0])
	}}
	brokers.AddCommand(list, show)
	return brokers
}

func realRegistryCommand() *cobra.Command {
	group := commandGroup("registry")
	list := &cobra.Command{Use: "list", Short: "List registry brokers", RunE: func(cmd *cobra.Command, _ []string) error {
		items, err := loadRegistry()
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		if format == "json" {
			return writeJSON(cmd, map[string]any{"schema_version": 1, "count": len(items), "brokers": items})
		}
		_, err = fmt.Fprintf(cmd.OutOrStdout(), "%d broker(s)\n", len(items))
		return err
	}}
	validate := &cobra.Command{Use: "validate", Short: "Validate the broker registry", RunE: func(cmd *cobra.Command, _ []string) error {
		items, err := loadRegistry()
		if err != nil {
			return err
		}
		format, err := outputFormat(cmd)
		if err != nil {
			return err
		}
		result := map[string]any{"schema_version": 1, "ok": true, "totals": map[string]any{"valid": len(items), "failed": 0, "duplicate_ids": 0}}
		if format == "json" {
			if err := writeJSON(cmd, result); err != nil {
				return err
			}
		} else if _, err := fmt.Fprintf(cmd.OutOrStdout(), "OK: %d broker(s)\n", len(items)); err != nil {
			return err
		}
		return nil
	}}
	group.AddCommand(list, validate)
	return group
}
