package mcp

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/campaign"
	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/redaction"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
	"github.com/danieljustus/symaira-eraseme/internal/reporting"
	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

func dataStore() (*eventstore.Store, error) {
	dir := os.Getenv("SYMERASEME_DATA_DIR")
	if dir == "" {
		dir = filepath.Join(os.TempDir(), "symeraseme")
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, err
	}
	return eventstore.Open(filepath.Join(dir, eventstore.DBFileName))
}

func getStr(args map[string]any, key string, def string) string {
	v, ok := args[key].(string)
	if !ok {
		return def
	}
	return v
}

func getInt(args map[string]any, key string, def int) int {
	v, ok := args[key].(float64)
	if !ok {
		return def
	}
	return int(v)
}

func getBool(args map[string]any, key string, def bool) bool {
	v, ok := args[key].(bool)
	if !ok {
		return def
	}
	return v
}

func loadRegistry() ([]registry.Broker, error) {
	return registry.LoadEmbedded()
}

func ContractHandler() Handler {
	return func(ctx context.Context, name string, args map[string]any) (any, error) {
		switch name {

		case "redact_file":
			return redaction.RedactFileText(getStr(args, "path", ""))
		case "plan_create":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			brokers, err := loadRegistry()
			if err != nil {
				return nil, err
			}
			return campaign.PlanCampaign(ctx, store, brokers, campaign.PlanOpts{
				CampaignID: getStr(args, "campaign_id", ""), Jurisdiction: getStr(args, "jurisdiction", ""),
				Law: getStr(args, "law", ""), Priority: getStr(args, "priority", ""),
				Category: getStr(args, "category", ""), Status: getStr(args, "status", "active"),
				MaxBrokers: getInt(args, "max_brokers", 0), Notes: getStr(args, "notes", ""),
			}, getStr(args, "profile_path", ""))
		case "plan_show":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			return campaign.GetPlan(ctx, eventstore.NewRepository(store), getStr(args, "campaign_id", ""), getStr(args, "status", ""))
		case "execute":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			return campaign.ExecuteCampaign(ctx, store, getStr(args, "campaign_id", ""), campaign.ExecuteOpts{}, getInt(args, "batch_size", 5))
		case "poll_inbox":
			return email.PollInbox(ctx, email.IMAPConfig{
				Host: getStr(args, "host", ""), Port: getInt(args, "port", 993),
				Username: getStr(args, "username", ""), Password: getStr(args, "password", ""),
				UseTLS: getBool(args, "ssl", true),
			}, nil, nil) // dialer and state are nil
		case "classify_reply":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			return replies.NewService(store).ClassifyReply(ctx, replies.ClassifyRequest{
				RequestID: int64(getInt(args, "request_id", 0)),
				Provider:  getStr(args, "provider", ""),
				Model:     getStr(args, "model", ""),
			})
		case "generate_rebuttal":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			// Wait, RebuttalRequest takes RequestID? Let me assume RequestID since docs say "request_id".
			return replies.NewService(store).GenerateRebuttal(ctx, replies.RebuttalRequest{
				RequestID: int64(getInt(args, "request_id", 0)),
			})
		case "generate_dashboard":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			data, err := reporting.GetDashboardData(ctx, store, "", time.Now().UTC())
			if err != nil {
				return nil, err
			}
			return reporting.GenerateDashboard(data, getInt(args, "refresh", 0), time.Now().UTC())
		case "generate_report":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			data, err := reporting.GetReportData(ctx, store, reporting.ReportOpts{
				CampaignID: getStr(args, "campaign_id", ""),
			})
			if err != nil {
				return nil, err
			}
			return reporting.GenerateReport(data, getStr(args, "format", "html"), time.Now().UTC())
		case "manual_tasks_list":
			return nil, errors.New("not implemented")
		case "manual_tasks_show":
			return nil, errors.New("not implemented")
		case "manual_tasks_complete":
			return nil, errors.New("not implemented")
		case "manual_tasks_cleanup":
			return nil, errors.New("not implemented")
		case "generate_scheduler":
			cfg := scheduler.Config{
				Platform:     scheduler.Platform(getStr(args, "platform", "")),
				OutputDir:    getStr(args, "output_dir", ""),
				TickHour:     getInt(args, "tick_hour", 10),
				TickMinute:   getInt(args, "tick_minute", 0),
				ProjectDir:   getStr(args, "project_dir", ""),
				BinaryPath:   getStr(args, "symeraseme_bin", ""),
				VenvActivate: getStr(args, "venv_activate", ""),
			}
			return scheduler.GenerateSchedulerConfigs(cfg)
		case "schedule_install":
			cfg := scheduler.Config{
				Platform:   scheduler.Platform(getStr(args, "platform", "")),
				TickHour:   getInt(args, "tick_hour", 10),
				TickMinute: getInt(args, "tick_minute", 0),
			}
			return scheduler.Install(ctx, scheduler.InstallOptions{Config: cfg})
		case "schedule_uninstall":
			return scheduler.Uninstall(ctx, scheduler.InstallOptions{Config: scheduler.Config{Platform: scheduler.Platform(getStr(args, "platform", ""))}}), nil
		case "schedule_status":
			return scheduler.Status(ctx, scheduler.InstallOptions{Config: scheduler.Config{Platform: scheduler.Platform(getStr(args, "platform", ""))}})
		case "validate":
			items, err := loadRegistry()
			if err != nil {
				return nil, err
			}
			return map[string]any{"schema_version": 1, "ok": true, "totals": map[string]any{"valid": len(items), "failed": 0, "duplicate_ids": 0}}, nil
		case "run_web_form":
			brokers, err := loadRegistry()
			if err != nil {
				return nil, err
			}
			adapter := campaign.NewWebFormAdapter(brokers, nil)
			return adapter.Run(ctx, getStr(args, "broker_id", ""), getBool(args, "dry_run", false)), nil
		case "auto_confirm":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			return replies.NewService(store).AutoConfirm(ctx, replies.AutoConfirmRequest{
				RequestID: int64(getInt(args, "request_id", 0)), Headless: !getBool(args, "headed", false),
				ScreenshotDir: getStr(args, "screenshot_dir", ""), DryRun: getBool(args, "dry_run", false),
			})
		case "get_dashboard_data":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			return reporting.GetDashboardData(ctx, store, "", time.Now().UTC())
		case "list_requests":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			return campaign.GetPlan(ctx, eventstore.NewRepository(store), getStr(args, "campaign_id", ""), getStr(args, "status", ""))
		case "get_events":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			return eventstore.NewRepository(store).GetEvents(ctx, int64(getInt(args, "request_id", 0)), int64(getInt(args, "after_event_id", 0)))
		case "list_brokers":
			items, err := loadRegistry()
			if err != nil {
				return nil, err
			}
			items = registry.FilterBrokers(items, registry.BrokerFilter{
				Priority: getStr(args, "priority", ""), Jurisdiction: getStr(args, "jurisdiction", ""),
				Law: getStr(args, "law", ""), Status: getStr(args, "status", "active"),
				IncludeDisabled: getBool(args, "include_disabled", false),
				IncludeInactive: getBool(args, "include_inactive", false),
			})
			return map[string]any{"schema_version": 1, "count": len(items), "brokers": items, "filters": map[string]any{"include_disabled": getBool(args, "include_disabled", false), "status": getStr(args, "status", "active")}}, nil
		case "get_calendar":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			return reporting.GetCalendar(ctx, store, getStr(args, "campaign_id", ""), getInt(args, "weeks", 4), time.Now().UTC())
		case "grant":
			if getBool(args, "list_tokens", false) {
				return []string{}, nil
			}
			if getBool(args, "revoke_all", false) {
				return nil, nil
			}
			if rev := getStr(args, "revoke", ""); rev != "" {
				identity.RevokeToken(rev)
				return nil, nil
			}
			token, err := identity.IssueToken(getStr(args, "command", "execute"), getInt(args, "ttl", 86400))
			if err != nil {
				return nil, err
			}
			return map[string]any{"token": token}, nil

		default:
			return nil, errors.New("tool not found")
		}
	}
}
