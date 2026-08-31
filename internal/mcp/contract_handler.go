package mcp

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	_ "github.com/danieljustus/symaira-eraseme"
	"github.com/danieljustus/symaira-eraseme/internal/campaign"
	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/llm"
	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
	"github.com/danieljustus/symaira-eraseme/internal/redaction"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
	"github.com/danieljustus/symaira-eraseme/internal/reporting"
	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

func dataStore() (*eventstore.Store, error) {
	dir := os.Getenv("SYMERASEME_DATA_DIR")
	if dir == "" {
		var err error
		dir, err = identity.DefaultConsentDir()
		if err != nil {
			return nil, err
		}
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, err
	}
	return eventstore.Open(filepath.Join(dir, eventstore.DBFileName))
}

func writeGeneratedFile(path, content string) (string, error) {
	absolute, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(filepath.Dir(absolute), 0o700); err != nil {
		return "", err
	}
	if err := os.WriteFile(absolute, []byte(content), 0o600); err != nil {
		return "", err
	}
	if err := os.Chmod(absolute, 0o600); err != nil {
		return "", err
	}
	return absolute, nil
}

func getStr(args map[string]any, key string, def string) string {
	v, ok := args[key].(string)
	if !ok {
		return def
	}
	return v
}

func getInt(args map[string]any, key string, def int) int {
	value, ok := args[key]
	if !ok || value == nil {
		return def
	}
	switch number := value.(type) {
	case int:
		return number
	case int8:
		return int(number)
	case int16:
		return int(number)
	case int32:
		return int(number)
	case int64:
		return int(number)
	case uint:
		return int(number)
	case uint8:
		return int(number)
	case uint16:
		return int(number)
	case uint32:
		return int(number)
	case uint64:
		return int(number)
	case float64:
		return int(number)
	case float32:
		return int(number)
	case json.Number:
		parsed, err := number.Int64()
		if err == nil {
			return int(parsed)
		}
	}
	return def
}

func int64Ptr(value int64) *int64 { return &value }

func newLLMClient(args map[string]any) (llm.Client, error) {
	return llm.Create(llm.CreateOptions{
		Provider: getStr(args, "provider", ""),
		Model:    getStr(args, "model", ""),
	})
}

func parsePollHours(raw string) ([]int, error) {
	if strings.TrimSpace(raw) == "" {
		return nil, nil
	}
	parts := strings.Split(raw, ",")
	hours := make([]int, 0, len(parts))
	for _, part := range parts {
		value, err := strconv.Atoi(strings.TrimSpace(part))
		if err != nil || value < 0 || value > 23 {
			return nil, fmt.Errorf("invalid poll hour %q: choose values from 0 to 23", strings.TrimSpace(part))
		}
		hours = append(hours, value)
	}
	return hours, nil
}

func getBool(args map[string]any, key string, def bool) bool {
	v, ok := args[key].(bool)
	if !ok {
		return def
	}
	return v
}

func loadRegistry(resourceDirs ...string) ([]registry.Broker, error) {
	if len(resourceDirs) > 0 && strings.TrimSpace(resourceDirs[0]) != "" {
		return registry.LoadFromDir(resourceDirs[0])
	}
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
				IncludeInactive: getBool(args, "include_inactive", false),
				MaxBrokers:      getInt(args, "max_brokers", 30), Notes: getStr(args, "notes", ""),
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
			dryRun := getBool(args, "dry_run", false)
			if !dryRun {
				if err := identity.ConsentGate("execute", identity.ConsentOptions{
					Yes:               false,
					ConsentToken:      getStr(args, "consent_token", ""),
					ConsentFile:       getStr(args, "consent_file", ""),
					Interactive:       false,
					ConsentEnvVar:     "SYMERASEME_CONSENT",
					ConsentFileEnvVar: "SYMERASEME_CONSENT_FILE",
				}); err != nil {
					return nil, err
				}
			}
			return campaign.ExecuteCampaign(ctx, store, getStr(args, "campaign_id", ""), campaign.ExecuteOpts{
				Account: getStr(args, "account", ""), DryRun: dryRun,
			}, getInt(args, "batch_size", 5))
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
			client, err := newLLMClient(args)
			if err != nil {
				return nil, err
			}
			defer client.Close()
			return replies.NewService(store).ClassifyReply(ctx, replies.ClassifyRequest{
				RequestID: int64(getInt(args, "request_id", 0)),
				Provider:  getStr(args, "provider", ""),
				Model:     getStr(args, "model", ""),
				Client:    client,
				Save:      getBool(args, "save", true),
			})
		case "generate_rebuttal":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			client, err := newLLMClient(args)
			if err != nil {
				return nil, err
			}
			defer client.Close()
			return replies.NewService(store).GenerateRebuttal(ctx, replies.RebuttalRequest{
				RequestID: int64(getInt(args, "request_id", 0)),
				Client:    client,
				Save:      getBool(args, "save", true),
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
			content, err := reporting.GenerateDashboard(data, getInt(args, "auto_refresh", 0), time.Now().UTC())
			if err != nil {
				return nil, err
			}
			if output := getStr(args, "output", ""); output != "" {
				path, err := writeGeneratedFile(output, content)
				if err != nil {
					return nil, err
				}
				return map[string]any{"success": true, "output_file": path, "size_bytes": len(content), "campaigns": data["total_campaigns"], "requests": data["total_requests"]}, nil
			}
			return map[string]any{"success": true, "dashboard": content, "campaigns": data["total_campaigns"], "requests": data["total_requests"]}, nil
		case "generate_report":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			format := getStr(args, "format", "html")
			data, err := reporting.GetReportData(ctx, store, reporting.ReportOpts{
				CampaignID:   getStr(args, "campaign_id", ""),
				AllCampaigns: getBool(args, "all_campaigns", false),
			})
			if err != nil {
				return nil, err
			}
			content, err := reporting.GenerateReport(data, format, time.Now().UTC())
			if err != nil {
				return nil, err
			}
			if output := getStr(args, "output", ""); output != "" {
				path, err := writeGeneratedFile(output, content)
				if err != nil {
					return nil, err
				}
				return map[string]any{"success": true, "output_file": path, "size_bytes": len(content), "format": format}, nil
			}
			return map[string]any{"success": true, "report": content, "format": format}, nil
		case "manual_tasks_list":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			var status *string
			if value := getStr(args, "status", ""); value != "" {
				status = &value
			}
			var requestID *int64
			if value := getInt(args, "request_id", 0); value != 0 {
				requestID = int64Ptr(int64(value))
			}
			result, err := manualtasks.HandleList(ctx, store, manualtasks.ListOpts{Status: status, RequestID: requestID})
			return result, err
		case "manual_tasks_show":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			result, err := manualtasks.HandleShow(ctx, store, int64(getInt(args, "task_id", 0)))
			return result, err
		case "manual_tasks_complete":
			store, err := dataStore()
			if err != nil {
				return nil, err
			}
			defer store.Close()
			result, err := manualtasks.HandleComplete(ctx, store, int64(getInt(args, "task_id", 0)), getStr(args, "notes", ""))
			return result, err
		case "manual_tasks_cleanup":
			result, err := manualtasks.HandleCleanup(getBool(args, "dry_run", false))
			return result, err
		case "generate_scheduler":
			pollHours, err := parsePollHours(getStr(args, "poll_hours", ""))
			if err != nil {
				return nil, err
			}
			outputDir := getStr(args, "output_dir", "./schedules")
			cfg := scheduler.Config{
				Platform:     scheduler.Platform(getStr(args, "platform", "")),
				OutputDir:    outputDir,
				TickHour:     getInt(args, "tick_hour", 10),
				TickMinute:   getInt(args, "tick_minute", 0),
				PollHours:    pollHours,
				ProjectDir:   getStr(args, "project_dir", ""),
				BinaryPath:   getStr(args, "symeraseme_bin", ""),
				VenvActivate: getStr(args, "venv_activate", ""),
			}
			files, err := scheduler.GenerateSchedulerConfigs(cfg)
			if err != nil {
				return nil, err
			}
			if getBool(args, "dry_run", false) {
				return map[string]any{"success": true, "files": files, "dry_run": true}, nil
			}
			written, err := scheduler.WriteSchedulerFiles(cfg.OutputDir, files)
			if err != nil {
				return nil, err
			}
			return map[string]any{"success": true, "files": written, "dry_run": false}, nil
		case "schedule_install":
			cfg := scheduler.Config{
				Platform:   scheduler.Platform(getStr(args, "platform", "")),
				TickHour:   getInt(args, "tick_hour", 10),
				TickMinute: getInt(args, "tick_minute", 0),
			}
			if getBool(args, "dry_run", false) {
				files, err := scheduler.Generate(cfg)
				if err != nil {
					return nil, err
				}
				return map[string]any{"success": true, "files": files, "dry_run": true}, nil
			}
			return scheduler.Install(ctx, scheduler.InstallOptions{Config: cfg})
		case "schedule_uninstall":
			err := scheduler.Uninstall(ctx, scheduler.InstallOptions{Config: scheduler.Config{Platform: scheduler.Platform(getStr(args, "platform", ""))}})
			if err != nil {
				return nil, err
			}
			return map[string]any{"success": true}, nil
		case "schedule_status":
			return scheduler.Status(ctx, scheduler.InstallOptions{Config: scheduler.Config{Platform: scheduler.Platform(getStr(args, "platform", ""))}})
		case "validate":
			items, err := loadRegistry(getStr(args, "registry_dir", ""))
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
			page := getInt(args, "page", 1)
			if page < 1 {
				return nil, errors.New("page must be at least 1")
			}
			pageSize := getInt(args, "page_size", 100)
			if pageSize < 1 {
				return nil, errors.New("page_size must be at least 1")
			}
			var campaignID, status, brokerID *string
			if value := getStr(args, "campaign_id", ""); value != "" {
				campaignID = &value
			}
			if value := getStr(args, "status", ""); value != "" {
				status = &value
			}
			if value := getStr(args, "broker_id", ""); value != "" {
				brokerID = &value
			}
			offset := (page - 1) * pageSize
			requests, err := eventstore.NewRepository(store).ListRemovalRequests(ctx, eventstore.ListRemovalRequestsOpts{
				CampaignID: campaignID, Status: status, BrokerID: brokerID, Limit: &pageSize, Offset: &offset,
			})
			if err != nil {
				return nil, err
			}
			total, err := eventstore.NewRepository(store).CountRemovalRequests(ctx, campaignID, status)
			if err != nil {
				return nil, err
			}
			return map[string]any{"requests": requests, "total": total, "page": page, "page_size": pageSize}, nil
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
				Law: getStr(args, "law", ""), Category: getStr(args, "category", ""), Status: getStr(args, "status", "active"),
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
			listTokens := getBool(args, "list_tokens", false)
			revokeAll := getBool(args, "revoke_all", false)
			revoke := getStr(args, "revoke", "")
			if listTokens {
				tokens, err := identity.ListTokens()
				if err != nil {
					return nil, err
				}
				return map[string]any{"success": true, "tokens": tokens, "count": len(tokens)}, nil
			}
			if getBool(args, "dry_run", false) {
				return map[string]any{"success": true, "dry_run": true, "command": getStr(args, "command", "execute"), "revoke": revoke, "revoke_all": revokeAll}, nil
			}
			if revokeAll {
				tokens, err := identity.ListTokens()
				if err != nil {
					return nil, err
				}
				revoked := 0
				for _, token := range tokens {
					ok, err := identity.RevokeToken(token.Token)
					if err != nil {
						return nil, err
					}
					if ok {
						revoked++
					}
				}
				return map[string]any{"success": true, "revoked": revoked, "revoke_all": true}, nil
			}
			if revoke != "" {
				ok, err := identity.RevokeToken(revoke)
				if err != nil {
					return nil, err
				}
				if !ok {
					return nil, fmt.Errorf("consent token not found")
				}
				return map[string]any{"success": true, "revoked": 1}, nil
			}
			token, err := identity.IssueToken(getStr(args, "command", "execute"), getInt(args, "ttl", 86400))
			if err != nil {
				return nil, err
			}
			return map[string]any{"success": true, "token": token}, nil

		default:
			return nil, errors.New("tool not found")
		}
	}
}
