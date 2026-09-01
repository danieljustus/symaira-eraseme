// Package reporting ports dashboard, calendar and report aggregation/export.
package reporting

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/csv"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/templating"
)

const pythonISO = "2006-01-02T15:04:05.999999-07:00"

// ReportOpts selects campaigns and pins time for deterministic callers/tests.
type ReportOpts struct {
	CampaignID   string
	AllCampaigns bool
	Now          time.Time
}

type campaignRow struct{ ID, CreatedAt, Kind, Notes string }

type requestRow struct {
	ID                                                                                       int64
	BrokerID, Channel, CampaignID, CreatedAt, Jurisdiction, TemplateID                       string
	CurrentStatus, SentAt, AcknowledgedAt, ResolvedAt, DeadlineAt, NextActionAt, LastEventAt string
	RemindersSent, EscalationLevel                                                           int
}

type eventRow struct {
	ID, RequestID                 int64
	EventType, OccurredAt, Source string
}

func nowUTC(now time.Time) time.Time {
	if now.IsZero() {
		return time.Now().UTC()
	}
	return now.UTC()
}
func iso(t time.Time) string { return t.Format(pythonISO) }

// GetReportData mirrors core/reports/data.py.
func GetReportData(ctx context.Context, store *eventstore.Store, opts ReportOpts) (map[string]any, error) {
	now := nowUTC(opts.Now)
	campaigns, err := loadCampaigns(ctx, store, opts.CampaignID, opts.AllCampaigns)
	if err != nil {
		return nil, err
	}
	if len(campaigns) == 0 {
		return emptyReport(opts.CampaignID, now), nil
	}
	ids := make([]string, len(campaigns))
	for i := range campaigns {
		ids[i] = campaigns[i].ID
	}
	requests, err := loadRequests(ctx, store, ids)
	if err != nil {
		return nil, err
	}
	events, err := loadEvents(ctx, store, requests)
	if err != nil {
		return nil, err
	}
	byCampaign := map[string][]requestRow{}
	for _, r := range requests {
		byCampaign[r.CampaignID] = append(byCampaign[r.CampaignID], r)
	}
	byRequest := map[int64][]eventRow{}
	for _, e := range events {
		byRequest[e.RequestID] = append(byRequest[e.RequestID], e)
	}
	aggregates := make([]any, 0, len(campaigns))
	for _, c := range campaigns {
		aggregates = append(aggregates, aggregateCampaign(c, byCampaign[c.ID], byRequest))
	}
	result := map[string]any{
		"generated_at": iso(now), "campaigns": aggregates, "total_campaigns": len(aggregates),
		"total_requests": len(requests), "status_breakdown": statusBreakdown(requests),
		"broker_leaderboard": brokerLeaderboard(requests), "jurisdiction_stats": jurisdictionBreakdown(requests),
		"timeline": buildTimeline(events), "historical_comparison": historicalComparison(aggregates),
		"success_metrics": successMetrics(requests),
	}
	return result, nil
}

func emptyReport(id string, now time.Time) map[string]any {
	if id == "" {
		id = "none"
	}
	return map[string]any{"generated_at": iso(now), "campaigns": []any{}, "total_campaigns": 0, "total_requests": 0,
		"status_breakdown": map[string]int{}, "broker_leaderboard": []any{}, "jurisdiction_stats": []any{},
		"timeline": []any{}, "historical_comparison": map[string]any{}, "success_metrics": map[string]any{},
		"error": fmt.Sprintf("Campaign '%s' not found or empty", id)}
}

func loadCampaigns(ctx context.Context, store *eventstore.Store, id string, all bool) ([]campaignRow, error) {
	query := "SELECT id, created_at, kind, notes FROM campaigns"
	args := []any{}
	if id != "" && !all {
		query += " WHERE id = ?"
		args = append(args, id)
	} else {
		query += " ORDER BY created_at DESC"
		if !all {
			query += " LIMIT 1"
		}
	}
	rows, err := store.DB().QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []campaignRow{}
	for rows.Next() {
		var c campaignRow
		var notes sql.NullString
		if err := rows.Scan(&c.ID, &c.CreatedAt, &c.Kind, &notes); err != nil {
			return nil, err
		}
		c.Notes = nullText(notes)
		c.CreatedAt = pyTimestamp(c.CreatedAt)
		out = append(out, c)
	}
	return out, rows.Err()
}

func loadRequests(ctx context.Context, store *eventstore.Store, ids []string) ([]requestRow, error) {
	marks := strings.TrimSuffix(strings.Repeat("?,", len(ids)), ",")
	args := make([]any, len(ids))
	for i := range ids {
		args[i] = ids[i]
	}
	q := `SELECT r.id,r.broker_id,r.channel,r.campaign_id,r.created_at,r.jurisdiction,r.template_id,
	COALESCE(s.current_status,'PLANNED'),s.sent_at,s.acknowledged_at,s.resolved_at,s.deadline_at,s.next_action_at,s.last_event_at,
	COALESCE(s.reminders_sent,0),COALESCE(s.escalation_level,0)
	FROM removal_requests r LEFT JOIN request_state s ON s.request_id=r.id WHERE r.campaign_id IN (` + marks + `) ORDER BY r.created_at ASC`
	rows, err := store.DB().QueryContext(ctx, q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []requestRow{}
	for rows.Next() {
		var r requestRow
		var sent, ack, res, dead, next, last sql.NullString
		if err := rows.Scan(&r.ID, &r.BrokerID, &r.Channel, &r.CampaignID, &r.CreatedAt, &r.Jurisdiction, &r.TemplateID, &r.CurrentStatus, &sent, &ack, &res, &dead, &next, &last, &r.RemindersSent, &r.EscalationLevel); err != nil {
			return nil, err
		}
		r.SentAt = pyTimestamp(nullText(sent))
		r.AcknowledgedAt = pyTimestamp(nullText(ack))
		r.ResolvedAt = pyTimestamp(nullText(res))
		r.DeadlineAt = pyTimestamp(nullText(dead))
		r.NextActionAt = pyTimestamp(nullText(next))
		r.LastEventAt = pyTimestamp(nullText(last))
		r.CreatedAt = pyTimestamp(r.CreatedAt)
		out = append(out, r)
	}
	return out, rows.Err()
}
func nullText(v sql.NullString) string {
	if v.Valid {
		return v.String
	}
	return ""
}

func pyTimestamp(value string) string {
	if strings.HasSuffix(value, "Z") {
		return strings.TrimSuffix(value, "Z") + "+00:00"
	}
	return value
}

func loadEvents(ctx context.Context, store *eventstore.Store, requests []requestRow) ([]eventRow, error) {
	if len(requests) == 0 {
		return []eventRow{}, nil
	}
	marks := strings.TrimSuffix(strings.Repeat("?,", len(requests)), ",")
	args := make([]any, len(requests))
	for i := range requests {
		args[i] = requests[i].ID
	}
	rows, err := store.DB().QueryContext(ctx, `SELECT id,request_id,event_type,occurred_at,source FROM request_events WHERE request_id IN (`+marks+`) ORDER BY occurred_at ASC,id ASC`, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []eventRow{}
	for rows.Next() {
		var e eventRow
		if err := rows.Scan(&e.ID, &e.RequestID, &e.EventType, &e.OccurredAt, &e.Source); err != nil {
			return nil, err
		}
		e.OccurredAt = pyTimestamp(e.OccurredAt)
		out = append(out, e)
	}
	return out, rows.Err()
}

func requestMap(r requestRow, events []eventRow) map[string]any {
	evs := make([]any, 0, len(events))
	for _, e := range events {
		evs = append(evs, map[string]any{"id": e.ID, "request_id": e.RequestID, "event_type": e.EventType, "occurred_at": e.OccurredAt, "source": e.Source})
	}
	return map[string]any{"id": r.ID, "broker_id": r.BrokerID, "channel": r.Channel, "campaign_id": r.CampaignID, "created_at": r.CreatedAt, "jurisdiction": r.Jurisdiction, "template_id": r.TemplateID, "current_status": r.CurrentStatus, "sent_at": nullable(r.SentAt), "acknowledged_at": nullable(r.AcknowledgedAt), "resolved_at": nullable(r.ResolvedAt), "deadline_at": nullable(r.DeadlineAt), "reminders_sent": r.RemindersSent, "escalation_level": r.EscalationLevel, "events": evs}
}
func nullable(s string) any {
	if s == "" {
		return nil
	}
	return s
}

func aggregateCampaign(c campaignRow, requests []requestRow, events map[int64][]eventRow) map[string]any {
	counts := countStatuses(requests)
	times := responseTimes(requests)
	var avg any = nil
	if len(times) > 0 {
		avg = round1(sum(times) / float64(len(times)))
	}
	reqs := make([]any, 0, len(requests))
	reminders := 0
	for _, r := range requests {
		reqs = append(reqs, requestMap(r, events[r.ID]))
		reminders += r.RemindersSent
	}
	total := len(requests)
	return map[string]any{"campaign_id": c.ID, "created_at": c.CreatedAt, "kind": c.Kind, "total": total, "status_counts": counts, "planned": counts["PLANNED"], "sent": counts["SENT"], "awaiting_ack": counts["AWAITING_ACK"], "awaiting_response": counts["AWAITING_RESPONSE"], "confirmed": counts["CONFIRMED"], "rejected": counts["REJECTED_FINAL"], "overdue": counts["OVERDUE"], "confirmation_rate": round1(float64(counts["CONFIRMED"]) / float64(max(total, 1)) * 100), "rejection_rate": round1(float64(counts["REJECTED_FINAL"]) / float64(max(total, 1)) * 100), "avg_response_time_days": avg, "total_reminders_sent": reminders, "requests": reqs}
}
func countStatuses(rs []requestRow) map[string]int {
	m := map[string]int{}
	for _, r := range rs {
		s := strings.ToUpper(r.CurrentStatus)
		if s == "" {
			s = "PLANNED"
		}
		m[s]++
	}
	return m
}
func statusBreakdown(rs []requestRow) map[string]int { return countStatuses(rs) }
func responseTimes(rs []requestRow) []float64 {
	out := []float64{}
	for _, r := range rs {
		a, ok1 := parseTime(r.SentAt)
		b, ok2 := parseTime(r.ResolvedAt)
		if ok1 && ok2 {
			out = append(out, b.Sub(a).Hours()/24)
		}
	}
	return out
}
func parseTime(v string) (time.Time, bool) {
	if v == "" {
		return time.Time{}, false
	}
	for _, layout := range []string{time.RFC3339Nano, "2006-01-02T15:04:05", "2006-01-02 15:04:05", "2006-01-02 15:04:05 -0700 MST"} {
		if t, e := time.Parse(layout, v); e == nil {
			return t.UTC(), true
		}
	}
	return time.Time{}, false
}
func round1(v float64) float64 {
	if v >= 0 {
		return float64(int(v*10+0.5)) / 10
	}
	return float64(int(v*10-0.5)) / 10
}
func sum(v []float64) float64 {
	n := 0.0
	for _, x := range v {
		n += x
	}
	return n
}

func brokerLeaderboard(rs []requestRow) []any {
	type stat struct {
		total, confirmed, rejected, overdue, pending int
		times                                        []float64
	}
	m := map[string]*stat{}
	for _, r := range rs {
		s := m[r.BrokerID]
		if s == nil {
			s = &stat{}
			m[r.BrokerID] = s
		}
		s.total++
		switch strings.ToUpper(r.CurrentStatus) {
		case "CONFIRMED":
			s.confirmed++
		case "REJECTED_FINAL":
			s.rejected++
		case "OVERDUE":
			s.overdue++
		default:
			s.pending++
		}
		a, aok := parseTime(r.SentAt)
		b, bok := parseTime(r.ResolvedAt)
		if aok && bok {
			s.times = append(s.times, b.Sub(a).Hours()/24)
		}
	}
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.SliceStable(keys, func(i, j int) bool { return m[keys[i]].total > m[keys[j]].total })
	out := []any{}
	for _, k := range keys {
		s := m[k]
		var avg any = nil
		if len(s.times) > 0 {
			avg = round1(sum(s.times) / float64(len(s.times)))
		}
		out = append(out, map[string]any{"broker_id": k, "total": s.total, "confirmed": s.confirmed, "rejected": s.rejected, "overdue": s.overdue, "pending": s.pending, "success_rate": round1(float64(s.confirmed) / float64(max(s.total, 1)) * 100), "avg_response_time_days": avg})
	}
	return out
}
func jurisdictionBreakdown(rs []requestRow) []any {
	m := map[string]map[string]any{}
	order := []string{}
	for _, r := range rs {
		k := strings.ToUpper(r.Jurisdiction)
		if k == "" {
			k = "UNKNOWN"
		}
		s := m[k]
		if s == nil {
			s = map[string]any{"jurisdiction": k, "total": 0, "confirmed": 0, "rejected": 0, "overdue": 0}
			m[k] = s
			order = append(order, k)
		}
		s["total"] = s["total"].(int) + 1
		switch strings.ToUpper(r.CurrentStatus) {
		case "CONFIRMED":
			s["confirmed"] = s["confirmed"].(int) + 1
		case "REJECTED_FINAL":
			s["rejected"] = s["rejected"].(int) + 1
		case "OVERDUE":
			s["overdue"] = s["overdue"].(int) + 1
		}
	}
	sort.SliceStable(order, func(i, j int) bool { return m[order[i]]["total"].(int) > m[order[j]]["total"].(int) })
	out := []any{}
	for _, k := range order {
		s := m[k]
		s["confirmation_rate"] = round1(float64(s["confirmed"].(int)) / float64(max(s["total"].(int), 1)) * 100)
		out = append(out, s)
	}
	return out
}
func buildTimeline(events []eventRow) []any {
	counts := map[string]map[string]int{}
	for _, e := range events {
		day := "unknown"
		if len(e.OccurredAt) >= 10 {
			day = e.OccurredAt[:10]
		}
		if counts[day] == nil {
			counts[day] = map[string]int{}
		}
		counts[day][e.EventType]++
	}
	days := make([]string, 0, len(counts))
	for d := range counts {
		days = append(days, d)
	}
	sort.Strings(days)
	out := []any{}
	for _, d := range days {
		n := 0
		for _, v := range counts[d] {
			n += v
		}
		out = append(out, map[string]any{"date": d, "total_events": n, "events": counts[d]})
	}
	return out
}
func historicalComparison(camps []any) map[string]any {
	if len(camps) < 2 {
		return map[string]any{}
	}
	a := camps[0].(map[string]any)
	b := camps[1].(map[string]any)
	out := map[string]any{"latest_campaign": a["campaign_id"], "previous_campaign": b["campaign_id"], "requests_change": a["total"].(int) - b["total"].(int), "confirmation_rate_change": round1(a["confirmation_rate"].(float64) - b["confirmation_rate"].(float64)), "rejection_rate_change": round1(a["rejection_rate"].(float64) - b["rejection_rate"].(float64))}
	if a["avg_response_time_days"] != nil && b["avg_response_time_days"] != nil {
		out["avg_response_time_change"] = round1(a["avg_response_time_days"].(float64) - b["avg_response_time_days"].(float64))
	} else {
		out["avg_response_time_change"] = nil
	}
	return out
}
func successMetrics(rs []requestRow) map[string]any {
	if len(rs) == 0 {
		return map[string]any{}
	}
	c, r, o := 0, 0, 0
	for _, x := range rs {
		switch strings.ToUpper(x.CurrentStatus) {
		case "CONFIRMED":
			c++
		case "REJECTED_FINAL":
			r++
		case "OVERDUE":
			o++
		}
	}
	times := responseTimes(rs)
	var avg, med any = nil, nil
	if len(times) > 0 {
		sort.Float64s(times)
		avg = round1(sum(times) / float64(len(times)))
		if len(times)%2 == 1 {
			med = times[len(times)/2]
		} else {
			med = (times[len(times)/2-1] + times[len(times)/2]) / 2
		}
	}
	n := len(rs)
	return map[string]any{"total_requests": n, "overall_confirmation_rate": round1(float64(c) / float64(n) * 100), "overall_rejection_rate": round1(float64(r) / float64(n) * 100), "overdue_rate": round1(float64(o) / float64(n) * 100), "avg_response_time_days": avg, "median_response_time_days": med}
}

// GetDashboardData mirrors core/dashboard.py.
func GetDashboardData(ctx context.Context, store *eventstore.Store, campaignID string, now time.Time) (map[string]any, error) {
	all := []requestRow{}
	ids := []string{}
	cs, err := loadCampaigns(ctx, store, campaignID, campaignID == "")
	if err != nil {
		return nil, err
	}
	for _, c := range cs {
		ids = append(ids, c.ID)
	}
	if len(ids) > 0 {
		all, err = loadRequests(ctx, store, ids)
		if err != nil {
			return nil, err
		}
	}
	byCampaign := map[string][]requestRow{}
	for _, request := range all {
		byCampaign[request.CampaignID] = append(byCampaign[request.CampaignID], request)
	}
	campaigns := make([]any, 0, len(cs))
	for _, campaign := range cs {
		requests := byCampaign[campaign.ID]
		counts := countStatuses(requests)
		requestMaps := make([]any, 0, len(requests))
		for _, request := range requests {
			requestMaps = append(requestMaps, dashboardRequestMap(request))
		}
		campaigns = append(campaigns, map[string]any{"id": campaign.ID, "created_at": campaign.CreatedAt,
			"kind": campaign.Kind, "requests": requestMaps, "total": len(requests), "planned": counts["PLANNED"],
			"sent": counts["SENT"], "awaiting_ack": counts["AWAITING_ACK"], "awaiting_response": counts["AWAITING_RESPONSE"],
			"confirmed": counts["CONFIRMED"], "rejected": counts["REJECTED_FINAL"], "overdue": counts["OVERDUE"]})
	}
	events, err := recentEvents(ctx, store, 50)
	if err != nil {
		return nil, err
	}
	counts := countStatuses(all)
	return map[string]any{"campaigns": campaigns, "total_requests": len(all), "planned": counts["PLANNED"], "sent": counts["SENT"], "awaiting_ack": counts["AWAITING_ACK"], "awaiting_response": counts["AWAITING_RESPONSE"], "confirmed": counts["CONFIRMED"], "rejected": counts["REJECTED_FINAL"], "overdue": counts["OVERDUE"], "broker_status": brokerDashboard(all), "recent_events": events, "generated_at": iso(nowUTC(now))}, nil
}

func dashboardRequestMap(r requestRow) map[string]any {
	return map[string]any{"id": r.ID, "broker_id": r.BrokerID, "channel": r.Channel, "campaign_id": r.CampaignID,
		"created_at": r.CreatedAt, "jurisdiction": r.Jurisdiction, "template_id": r.TemplateID,
		"current_status": r.CurrentStatus, "sent_at": nullable(r.SentAt), "acknowledged_at": nullable(r.AcknowledgedAt),
		"resolved_at": nullable(r.ResolvedAt), "deadline_at": nullable(r.DeadlineAt), "reminders_sent": r.RemindersSent,
		"escalation_level": r.EscalationLevel, "last_event_at": nullable(r.LastEventAt)}
}
func brokerDashboard(rs []requestRow) []any {
	full := brokerLeaderboard(rs)
	out := []any{}
	for _, v := range full {
		m := v.(map[string]any)
		out = append(out, map[string]any{"broker_id": m["broker_id"], "total": m["total"], "confirmed": m["confirmed"], "rejected": m["rejected"], "overdue": m["overdue"], "pending": m["pending"]})
	}
	return out
}
func recentEvents(ctx context.Context, store *eventstore.Store, limit int) ([]any, error) {
	rows, err := store.DB().QueryContext(ctx, `SELECT e.id,e.request_id,e.event_type,e.occurred_at,e.source,r.broker_id
		FROM request_events e JOIN removal_requests r ON r.id=e.request_id ORDER BY e.occurred_at DESC,e.id DESC LIMIT ?`, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []any{}
	for rows.Next() {
		var e eventRow
		var brokerID string
		if err := rows.Scan(&e.ID, &e.RequestID, &e.EventType, &e.OccurredAt, &e.Source, &brokerID); err != nil {
			return nil, err
		}
		out = append(out, map[string]any{"id": e.ID, "request_id": e.RequestID, "event_type": e.EventType,
			"occurred_at": pyTimestamp(e.OccurredAt), "source": e.Source, "broker_id": brokerID})
	}
	return out, rows.Err()
}

// GetCampaignStatus returns lifecycle counts at a pinned instant.
func GetCampaignStatus(ctx context.Context, store *eventstore.Store, campaignID string, now time.Time) (map[string]any, error) {
	now = nowUTC(now)
	where := ""
	args := []any{}
	if campaignID != "" {
		where = " WHERE r.campaign_id = ?"
		args = append(args, campaignID)
	}
	ids := []string{}
	cs, err := loadCampaigns(ctx, store, campaignID, campaignID == "")
	if err != nil {
		return nil, err
	}
	for _, c := range cs {
		ids = append(ids, c.ID)
	}
	rs := []requestRow{}
	if len(ids) > 0 {
		rs, err = loadRequests(ctx, store, ids)
		if err != nil {
			return nil, err
		}
	}
	_ = where
	_ = args
	status := countStatuses(rs)
	channels := map[string]int{}
	esc := map[int]int{0: 0, 1: 0, 2: 0}
	resolved, overdue, d7, d30, tick := 0, 0, 0, 0, 0
	for _, r := range rs {
		channels[r.Channel]++
		esc[r.EscalationLevel]++
		if r.ResolvedAt != "" {
			resolved++
			continue
		}
		if t, ok := parseTime(r.DeadlineAt); ok {
			if !t.After(now) {
				overdue++
			}
			if !t.Before(now) && !t.After(now.Add(7*24*time.Hour)) {
				d7++
			}
			if !t.Before(now) && !t.After(now.Add(30*24*time.Hour)) {
				d30++
			}
		}
		if t, ok := parseTime(r.NextActionAt); ok && !t.After(now) {
			tick++
		}
	}
	scope := "all"
	if campaignID != "" {
		scope = campaignID
	}
	return map[string]any{"schema_version": 1, "as_of": iso(now), "scope": map[string]any{"campaign_id": scope}, "totals": map[string]any{"requests": len(rs), "resolved": resolved, "open": len(rs) - resolved}, "by_status": status, "by_channel": channels, "escalation": map[string]any{"none": esc[0], "reminder": esc[1], "dpa_pending": esc[2]}, "upcoming": map[string]any{"overdue": overdue, "deadline_due_within_7d": d7, "deadline_due_within_30d": d30, "tick_actions_ready": tick}}, nil
}

// GetCalendar returns unresolved deadlines/actions grouped by ISO week.
func GetCalendar(ctx context.Context, store *eventstore.Store, campaignID string, weeks int, now time.Time) (map[string]any, error) {
	now = nowUTC(now)
	horizon := now.Add(time.Duration(weeks) * 7 * 24 * time.Hour)
	cs, err := loadCampaigns(ctx, store, campaignID, campaignID == "")
	if err != nil {
		return nil, err
	}
	ids := []string{}
	for _, c := range cs {
		ids = append(ids, c.ID)
	}
	rs := []requestRow{}
	if len(ids) > 0 {
		rs, err = loadRequests(ctx, store, ids)
		if err != nil {
			return nil, err
		}
	}
	buckets := map[string][]any{}
	entries := 0
	overdue := 0
	for _, r := range rs {
		if r.ResolvedAt != "" {
			continue
		}
		marker := r.DeadlineAt
		kind := "deadline"
		if r.NextActionAt != "" {
			marker = r.NextActionAt
			kind = "next_action"
		}
		t, ok := parseTime(marker)
		if !ok || t.After(horizon) {
			continue
		}
		days := int(t.Sub(now).Hours() / 24)
		if t.Before(now) && time.Duration(days)*24*time.Hour != t.Sub(now) {
			days--
		}
		isOverdue := days < 0
		if isOverdue {
			overdue++
		}
		entry := map[string]any{"request_id": r.ID, "broker_id": r.BrokerID, "campaign_id": r.CampaignID, "jurisdiction": r.Jurisdiction, "current_status": r.CurrentStatus, "marker": kind, "marker_at": marker, "days_from_now": days, "overdue": isOverdue, "deadline_at": nullable(r.DeadlineAt), "next_action_at": nullable(r.NextActionAt), "escalation_level": r.EscalationLevel, "reminders_sent": r.RemindersSent}
		y, w := t.ISOWeek()
		key := fmt.Sprintf("%04d-W%02d", y, w)
		buckets[key] = append(buckets[key], entry)
		entries++
	}
	keys := make([]string, 0, len(buckets))
	for k := range buckets {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	grouped := []any{}
	for _, k := range keys {
		grouped = append(grouped, map[string]any{"week": k, "entries": buckets[k]})
	}
	scope := "all"
	if campaignID != "" {
		scope = campaignID
	}
	return map[string]any{"schema_version": 1, "as_of": iso(now), "horizon_weeks": weeks, "horizon_until": iso(horizon), "scope": map[string]any{"campaign_id": scope}, "totals": map[string]any{"entries": entries, "overdue": overdue, "weeks_with_actions": len(buckets)}, "weeks": grouped}, nil
}

// ExportJSON matches json.dumps(..., indent=2, ensure_ascii=False).
func ExportJSON(data map[string]any) (string, error) {
	var b bytes.Buffer
	e := json.NewEncoder(&b)
	e.SetIndent("", "  ")
	e.SetEscapeHTML(false)
	if err := e.Encode(data); err != nil {
		return "", err
	}
	return strings.TrimSuffix(b.String(), "\n"), nil
}
func ExportCSV(data map[string]any) (string, error) {
	var b bytes.Buffer
	w := csv.NewWriter(&b)
	w.UseCRLF = true
	header := []string{"campaign_id", "request_id", "broker_id", "jurisdiction", "channel", "status", "sent_at", "acknowledged_at", "resolved_at", "deadline_at", "reminders_sent", "escalation_level"}
	if err := w.Write(header); err != nil {
		return "", err
	}
	for _, cv := range data["campaigns"].([]any) {
		c := cv.(map[string]any)
		for _, rv := range c["requests"].([]any) {
			r := rv.(map[string]any)
			row := []string{fmt.Sprint(c["campaign_id"]), fmt.Sprint(r["id"]), fmt.Sprint(r["broker_id"]), fmt.Sprint(r["jurisdiction"]), fmt.Sprint(r["channel"]), fmt.Sprint(r["current_status"]), text(r["sent_at"]), text(r["acknowledged_at"]), text(r["resolved_at"]), text(r["deadline_at"]), fmt.Sprint(r["reminders_sent"]), fmt.Sprint(r["escalation_level"])}
			if err := w.Write(row); err != nil {
				return "", err
			}
		}
	}
	w.Flush()
	return b.String(), w.Error()
}
func text(v any) string {
	if v == nil {
		return ""
	}
	return fmt.Sprint(v)
}
func ExportHTML(data map[string]any, now time.Time) (string, error) {
	return templating.Render("report.html.j2", templating.RenderOpts{ExtraVars: map[string]any{"data": data, "now": nowUTC(now)}})
}
func GenerateDashboard(data map[string]any, refresh int, now time.Time) (string, error) {
	return templating.Render("dashboard.html.j2", templating.RenderOpts{ExtraVars: map[string]any{"data": data, "auto_refresh_seconds": refresh, "now": nowUTC(now)}})
}
func GenerateReport(data map[string]any, format string, now time.Time) (string, error) {
	switch strings.ToLower(format) {
	case "json":
		return ExportJSON(data)
	case "csv":
		return ExportCSV(data)
	case "html":
		return ExportHTML(data, now)
	default:
		return "", fmt.Errorf("unsupported format: %s; choose html, json, or csv", strings.ToLower(format))
	}
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
