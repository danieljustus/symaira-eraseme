//! Dashboard, calendar and report aggregation.
//!
//! Ports `internal/reporting/reporting.go`, which in turn mirrors
//! `core/reports/data.py`. The shared golden fixture is
//! `tests/fixtures/event-store/golden-reporting.json`; the Go test normalises
//! both sides through the same JSON encoder before comparing, and the Rust test
//! does the same, so a wrong integer/float choice still fails while float
//! formatting differences do not.

use chrono::{DateTime, Datelike, FixedOffset, NaiveDateTime, Utc};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::jsonorder::go_map_order;
use crate::storage::Store;
use crate::templating::{FrozenDateTime, RenderContext, render};
use crate::timeutil::format_iso;

/// Selects campaigns. The clock is injected rather than read from the
/// environment, matching the rest of this crate: `chrono` is compiled without
/// its `clock` feature.
#[derive(Debug, Clone, Default)]
pub struct ReportOpts {
    pub campaign_id: String,
    pub all_campaigns: bool,
}

#[derive(Debug, Clone)]
pub struct CampaignRow {
    pub id: String,
    pub created_at: String,
    pub kind: String,
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct RequestRow {
    pub id: i64,
    pub broker_id: String,
    pub channel: String,
    pub campaign_id: String,
    pub created_at: String,
    pub jurisdiction: String,
    pub template_id: String,
    pub current_status: String,
    pub sent_at: String,
    pub acknowledged_at: String,
    pub resolved_at: String,
    pub deadline_at: String,
    pub next_action_at: String,
    pub last_event_at: String,
    pub reminders_sent: i64,
    pub escalation_level: i64,
}

/// Go's layout `2006-01-02T15:04:05.999999-07:00` for the whole-second instants
/// this contract carries, which is what `format_iso` already produces.
///
/// ponytail: a sub-second `now` would keep its fraction in Go and lose it here;
/// the shared golden fixture and every caller pin whole seconds.
pub(crate) fn iso(t: DateTime<Utc>) -> String {
    format_iso(t)
}

/// Go's `pyTimestamp`: a trailing `Z` becomes `+00:00`, everything else is kept.
pub(crate) fn py_timestamp(value: &str) -> String {
    match value.strip_suffix('Z') {
        Some(head) => format!("{head}+00:00"),
        None => value.to_owned(),
    }
}

/// Go's `parseTime`: RFC3339 first, then the two naive layouts, then a naive
/// timestamp followed by a numeric offset (the zone abbreviation is ignored).
pub(crate) fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.with_timezone(&Utc));
    }
    for layout in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(value, layout) {
            return Some(naive.and_utc());
        }
    }
    if value.len() >= 25 {
        let naive = NaiveDateTime::parse_from_str(&value[..19], "%Y-%m-%d %H:%M:%S").ok();
        let offset = FixedOffset::from_str(&value[20..25]).ok();
        if let (Some(naive), Some(offset)) = (naive, offset) {
            return naive
                .and_local_timezone(offset)
                .single()
                .map(|parsed| parsed.with_timezone(&Utc));
        }
    }
    None
}

/// Go's `nullable`: an empty string becomes JSON null.
fn nullable(value: &str) -> Value {
    if value.is_empty() {
        Value::Null
    } else {
        Value::String(value.to_owned())
    }
}

/// Go's `round1`: round half away from zero at one decimal, truncating the
/// scaled value towards zero exactly like Go's `int(...)` conversion.
pub(crate) fn round1(value: f64) -> f64 {
    if value >= 0.0 {
        ((value * 10.0 + 0.5) as i64) as f64 / 10.0
    } else {
        ((value * 10.0 - 0.5) as i64) as f64 / 10.0
    }
}

fn load_campaigns(store: &Store, id: &str, all: bool) -> rusqlite::Result<Vec<CampaignRow>> {
    let mut query = String::from("SELECT id, created_at, kind, notes FROM campaigns");
    if !id.is_empty() && !all {
        query.push_str(" WHERE id = ?");
    } else {
        query.push_str(" ORDER BY created_at DESC");
        if !all {
            query.push_str(" LIMIT 1");
        }
    }
    let connection = store.connection();
    let mut statement = connection.prepare(&query)?;
    let map_row = |row: &rusqlite::Row<'_>| {
        let notes: Option<String> = row.get(3)?;
        let created_at: String = row.get(1)?;
        Ok(CampaignRow {
            id: row.get(0)?,
            created_at: py_timestamp(&created_at),
            kind: row.get(2)?,
            notes: notes.unwrap_or_default(),
        })
    };
    let rows = if !id.is_empty() && !all {
        statement.query_map([id], map_row)?
    } else {
        statement.query_map([], map_row)?
    };
    rows.collect()
}

fn load_requests(store: &Store, ids: &[String]) -> rusqlite::Result<Vec<RequestRow>> {
    let marks = marks_for(ids.len());
    let query = format!(
        "SELECT r.id,r.broker_id,r.channel,r.campaign_id,r.created_at,r.jurisdiction,r.template_id,\n\
         COALESCE(s.current_status,'PLANNED'),s.sent_at,s.acknowledged_at,s.resolved_at,s.deadline_at,s.next_action_at,s.last_event_at,\n\
         COALESCE(s.reminders_sent,0),COALESCE(s.escalation_level,0)\n\
         FROM removal_requests r LEFT JOIN request_state s ON s.request_id=r.id WHERE r.campaign_id IN ({marks}) ORDER BY r.created_at ASC"
    );
    let connection = store.connection();
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(rusqlite::params_from_iter(ids.iter()), |row| {
        let sent: Option<String> = row.get(8)?;
        let acknowledged: Option<String> = row.get(9)?;
        let resolved: Option<String> = row.get(10)?;
        let deadline: Option<String> = row.get(11)?;
        let next_action: Option<String> = row.get(12)?;
        let last_event: Option<String> = row.get(13)?;
        let created_at: String = row.get(4)?;
        Ok(RequestRow {
            id: row.get(0)?,
            broker_id: row.get(1)?,
            channel: row.get(2)?,
            campaign_id: row.get(3)?,
            created_at: py_timestamp(&created_at),
            jurisdiction: row.get(5)?,
            template_id: row.get(6)?,
            current_status: row.get(7)?,
            sent_at: py_timestamp(&sent.unwrap_or_default()),
            acknowledged_at: py_timestamp(&acknowledged.unwrap_or_default()),
            resolved_at: py_timestamp(&resolved.unwrap_or_default()),
            deadline_at: py_timestamp(&deadline.unwrap_or_default()),
            next_action_at: py_timestamp(&next_action.unwrap_or_default()),
            last_event_at: py_timestamp(&last_event.unwrap_or_default()),
            reminders_sent: row.get(14)?,
            escalation_level: row.get(15)?,
        })
    })?;
    rows.collect()
}

/// Go's `strings.TrimSuffix(strings.Repeat("?,", n), ",")`.
fn marks_for(count: usize) -> String {
    let mut marks = "?,".repeat(count);
    marks.pop();
    marks
}

/// Go's `countStatuses`: upper-cased, empty becomes PLANNED, absent stays absent.
pub(crate) fn count_statuses(rows: &[RequestRow]) -> BTreeMap<String, i64> {
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for row in rows {
        let status = row.current_status.to_uppercase();
        let status = if status.is_empty() {
            "PLANNED".to_owned()
        } else {
            status
        };
        *counts.entry(status).or_insert(0) += 1;
    }
    counts
}

fn counts_to_value(counts: &BTreeMap<String, i64>) -> Value {
    Value::Object(
        counts
            .iter()
            .map(|(key, value)| (key.clone(), json!(value)))
            .collect::<Map<String, Value>>(),
    )
}

/// Loads the campaigns and their requests the way every entry point does.
fn load_scope(
    store: &Store,
    campaign_id: &str,
) -> rusqlite::Result<(Vec<CampaignRow>, Vec<RequestRow>)> {
    let campaigns = load_campaigns(store, campaign_id, campaign_id.is_empty())?;
    if campaigns.is_empty() {
        return Ok((campaigns, Vec::new()));
    }
    let ids: Vec<String> = campaigns.iter().map(|row| row.id.clone()).collect();
    let requests = load_requests(store, &ids)?;
    Ok((campaigns, requests))
}

/// Mirrors Go's `GetCampaignStatus`.
pub fn get_campaign_status(
    store: &Store,
    campaign_id: &str,
    now: DateTime<Utc>,
) -> rusqlite::Result<Value> {
    let (_, requests) = load_scope(store, campaign_id)?;
    let statuses = count_statuses(&requests);
    let mut channels: BTreeMap<String, i64> = BTreeMap::new();
    let mut escalation: BTreeMap<i64, i64> = BTreeMap::new();
    for level in 0..=2 {
        escalation.insert(level, 0);
    }
    let (mut resolved, mut overdue, mut due7, mut due30, mut tick) = (0i64, 0i64, 0i64, 0i64, 0i64);
    for request in &requests {
        *channels.entry(request.channel.clone()).or_insert(0) += 1;
        *escalation.entry(request.escalation_level).or_insert(0) += 1;
        if !request.resolved_at.is_empty() {
            resolved += 1;
            continue;
        }
        if let Some(deadline) = parse_time(&request.deadline_at) {
            if deadline <= now {
                overdue += 1;
            }
            if deadline >= now && deadline <= now + chrono::Duration::days(7) {
                due7 += 1;
            }
            if deadline >= now && deadline <= now + chrono::Duration::days(30) {
                due30 += 1;
            }
        }
        if let Some(next_action) = parse_time(&request.next_action_at)
            && next_action <= now
        {
            tick += 1;
        }
    }
    let scope = if campaign_id.is_empty() {
        "all"
    } else {
        campaign_id
    };
    let total = requests.len() as i64;
    Ok(go_map_order(json!({
        "schema_version": 1,
        "as_of": iso(now),
        "scope": {"campaign_id": scope},
        "totals": {"requests": total, "resolved": resolved, "open": total - resolved},
        "by_status": counts_to_value(&statuses),
        "by_channel": counts_to_value(&channels),
        "escalation": {
            "none": escalation.get(&0).copied().unwrap_or(0),
            "reminder": escalation.get(&1).copied().unwrap_or(0),
            "dpa_pending": escalation.get(&2).copied().unwrap_or(0),
        },
        "upcoming": {
            "overdue": overdue,
            "deadline_due_within_7d": due7,
            "deadline_due_within_30d": due30,
            "tick_actions_ready": tick,
        },
    })))
}

/// Mirrors Go's `GetCalendar`: unresolved markers grouped by ISO week.
pub fn get_calendar(
    store: &Store,
    campaign_id: &str,
    weeks: i64,
    now: DateTime<Utc>,
) -> rusqlite::Result<Value> {
    let horizon = now + chrono::Duration::days(weeks * 7);
    let (_, requests) = load_scope(store, campaign_id)?;
    let mut buckets: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let (mut entries, mut overdue) = (0i64, 0i64);
    for request in &requests {
        if !request.resolved_at.is_empty() {
            continue;
        }
        let (marker, kind) = if request.next_action_at.is_empty() {
            (&request.deadline_at, "deadline")
        } else {
            (&request.next_action_at, "next_action")
        };
        let Some(at) = parse_time(marker) else {
            continue;
        };
        if at > horizon {
            continue;
        }
        let mut days = (at - now).num_hours() / 24;
        if at < now && chrono::Duration::days(days) != at - now {
            days -= 1;
        }
        let is_overdue = days < 0;
        if is_overdue {
            overdue += 1;
        }
        let entry = json!({
            "request_id": request.id,
            "broker_id": request.broker_id,
            "campaign_id": request.campaign_id,
            "jurisdiction": request.jurisdiction,
            "current_status": request.current_status,
            "marker": kind,
            "marker_at": marker,
            "days_from_now": days,
            "overdue": is_overdue,
            "deadline_at": nullable(&request.deadline_at),
            "next_action_at": nullable(&request.next_action_at),
            "escalation_level": request.escalation_level,
            "reminders_sent": request.reminders_sent,
        });
        let iso_week = at.iso_week();
        buckets
            .entry(format!("{:04}-W{:02}", iso_week.year(), iso_week.week()))
            .or_default()
            .push(entry);
        entries += 1;
    }
    let weeks_with_actions = buckets.len() as i64;
    let grouped: Vec<Value> = buckets
        .into_iter()
        .map(|(week, entries)| json!({"week": week, "entries": entries}))
        .collect();
    let scope = if campaign_id.is_empty() {
        "all"
    } else {
        campaign_id
    };
    Ok(go_map_order(json!({
        "schema_version": 1,
        "as_of": iso(now),
        "horizon_weeks": weeks,
        "horizon_until": iso(horizon),
        "scope": {"campaign_id": scope},
        "totals": {
            "entries": entries,
            "overdue": overdue,
            "weeks_with_actions": weeks_with_actions,
        },
        "weeks": grouped,
    })))
}

#[derive(Debug, Clone)]
pub struct EventRow {
    pub id: i64,
    pub request_id: i64,
    pub event_type: String,
    pub occurred_at: String,
    pub source: String,
}

fn load_events(store: &Store, requests: &[RequestRow]) -> rusqlite::Result<Vec<EventRow>> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = requests.iter().map(|row| row.id).collect();
    let marks = marks_for(ids.len());
    let query = format!(
        "SELECT id,request_id,event_type,occurred_at,source FROM request_events \
         WHERE request_id IN ({marks}) ORDER BY occurred_at ASC,id ASC"
    );
    let connection = store.connection();
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(rusqlite::params_from_iter(ids.iter()), |row| {
        let occurred_at: String = row.get(3)?;
        Ok(EventRow {
            id: row.get(0)?,
            request_id: row.get(1)?,
            event_type: row.get(2)?,
            occurred_at: py_timestamp(&occurred_at),
            source: row.get(4)?,
        })
    })?;
    rows.collect()
}

/// Go's `responseTimes`: sent→resolved distance in days where both parse.
fn response_times(rows: &[RequestRow]) -> Vec<f64> {
    let mut times = Vec::new();
    for row in rows {
        if let (Some(sent), Some(resolved)) =
            (parse_time(&row.sent_at), parse_time(&row.resolved_at))
        {
            times.push((resolved - sent).num_seconds() as f64 / 86_400.0);
        }
    }
    times
}

fn round1_or_null(value: Option<f64>) -> Value {
    match value {
        Some(number) => json!(round1(number)),
        None => Value::Null,
    }
}

/// Go's `requestMap`, including the request's events.
fn request_map(request: &RequestRow, events: &[EventRow]) -> Value {
    let events: Vec<Value> = events
        .iter()
        .map(|event| {
            json!({
                "id": event.id,
                "request_id": event.request_id,
                "event_type": event.event_type,
                "occurred_at": event.occurred_at,
                "source": event.source,
            })
        })
        .collect();
    json!({
        "id": request.id,
        "broker_id": request.broker_id,
        "channel": request.channel,
        "campaign_id": request.campaign_id,
        "created_at": request.created_at,
        "jurisdiction": request.jurisdiction,
        "template_id": request.template_id,
        "current_status": request.current_status,
        "sent_at": nullable(&request.sent_at),
        "acknowledged_at": nullable(&request.acknowledged_at),
        "resolved_at": nullable(&request.resolved_at),
        "deadline_at": nullable(&request.deadline_at),
        "reminders_sent": request.reminders_sent,
        "escalation_level": request.escalation_level,
        "events": events,
    })
}

/// Go's `dashboardRequestMap`: like `request_map` but with `last_event_at`
/// instead of the nested events.
fn dashboard_request_map(request: &RequestRow) -> Value {
    json!({
        "id": request.id,
        "broker_id": request.broker_id,
        "channel": request.channel,
        "campaign_id": request.campaign_id,
        "created_at": request.created_at,
        "jurisdiction": request.jurisdiction,
        "template_id": request.template_id,
        "current_status": request.current_status,
        "sent_at": nullable(&request.sent_at),
        "acknowledged_at": nullable(&request.acknowledged_at),
        "resolved_at": nullable(&request.resolved_at),
        "deadline_at": nullable(&request.deadline_at),
        "reminders_sent": request.reminders_sent,
        "escalation_level": request.escalation_level,
        "last_event_at": nullable(&request.last_event_at),
    })
}

fn count_of(counts: &BTreeMap<String, i64>, key: &str) -> i64 {
    counts.get(key).copied().unwrap_or(0)
}

fn confirmation_rate(counts: &BTreeMap<String, i64>, total: i64) -> f64 {
    round1(count_of(counts, "CONFIRMED") as f64 / total.max(1) as f64 * 100.0)
}

fn rejection_rate(counts: &BTreeMap<String, i64>, total: i64) -> f64 {
    round1(count_of(counts, "REJECTED_FINAL") as f64 / total.max(1) as f64 * 100.0)
}

/// Go's `aggregateCampaign`.
fn aggregate_campaign(
    campaign: &CampaignRow,
    requests: &[RequestRow],
    events: &BTreeMap<i64, Vec<EventRow>>,
) -> Value {
    let counts = count_statuses(requests);
    let times = response_times(requests);
    let average = if times.is_empty() {
        None
    } else {
        Some(times.iter().sum::<f64>() / times.len() as f64)
    };
    let mut reminders = 0;
    let request_values: Vec<Value> = requests
        .iter()
        .map(|request| {
            reminders += request.reminders_sent;
            let empty = Vec::new();
            let request_events = events.get(&request.id).unwrap_or(&empty);
            request_map(request, request_events)
        })
        .collect();
    let total = requests.len() as i64;
    json!({
        "campaign_id": campaign.id,
        "created_at": campaign.created_at,
        "kind": campaign.kind,
        "total": total,
        "status_counts": counts_to_value(&counts),
        "planned": count_of(&counts, "PLANNED"),
        "sent": count_of(&counts, "SENT"),
        "awaiting_ack": count_of(&counts, "AWAITING_ACK"),
        "awaiting_response": count_of(&counts, "AWAITING_RESPONSE"),
        "confirmed": count_of(&counts, "CONFIRMED"),
        "rejected": count_of(&counts, "REJECTED_FINAL"),
        "overdue": count_of(&counts, "OVERDUE"),
        "confirmation_rate": confirmation_rate(&counts, total),
        "rejection_rate": rejection_rate(&counts, total),
        "avg_response_time_days": round1_or_null(average),
        "total_reminders_sent": reminders,
        "requests": request_values,
    })
}

/// Go's `emptyReport`.
fn empty_report(id: &str, now: DateTime<Utc>) -> Value {
    let id = if id.is_empty() { "none" } else { id };
    json!({
        "generated_at": iso(now),
        "campaigns": [],
        "total_campaigns": 0,
        "total_requests": 0,
        "status_breakdown": {},
        "broker_leaderboard": [],
        "jurisdiction_stats": [],
        "timeline": [],
        "historical_comparison": {},
        "success_metrics": {},
        "error": format!("Campaign '{id}' not found or empty"),
    })
}

/// A broker aggregate before it is turned into JSON.
struct BrokerStat {
    broker_id: String,
    total: i64,
    confirmed: i64,
    rejected: i64,
    overdue: i64,
    pending: i64,
    times: Vec<f64>,
}

/// Go's `brokerLeaderboard`, ordered by total descending then broker_id.
///
/// Go derived its ordering slice from map iteration, so brokers with equal
/// totals came out in a random order; both sides now order equal totals by
/// `broker_id` ascending, so the result is a function of the data alone (#963).
fn broker_leaderboard(requests: &[RequestRow]) -> Vec<BrokerStat> {
    let mut stats: Vec<BrokerStat> = Vec::new();
    let position = |stats: &Vec<BrokerStat>, broker_id: &str| {
        stats.iter().position(|stat| stat.broker_id == broker_id)
    };
    for request in requests {
        let index = match position(&stats, &request.broker_id) {
            Some(index) => index,
            None => {
                stats.push(BrokerStat {
                    broker_id: request.broker_id.clone(),
                    total: 0,
                    confirmed: 0,
                    rejected: 0,
                    overdue: 0,
                    pending: 0,
                    times: Vec::new(),
                });
                stats.len() - 1
            }
        };
        let stat = &mut stats[index];
        stat.total += 1;
        match request.current_status.to_uppercase().as_str() {
            "CONFIRMED" => stat.confirmed += 1,
            "REJECTED_FINAL" => stat.rejected += 1,
            "OVERDUE" => stat.overdue += 1,
            _ => stat.pending += 1,
        }
        if let (Some(sent), Some(resolved)) = (
            parse_time(&request.sent_at),
            parse_time(&request.resolved_at),
        ) {
            stat.times
                .push((resolved - sent).num_seconds() as f64 / 86_400.0);
        }
    }
    // Total descending, then broker_id ascending. Go derives its ordering
    // slice from map iteration, so equal totals came out in a random order
    // there (#963); both sides now share this rule instead of this port
    // documenting a divergence.
    stats.sort_by(|left, right| {
        right
            .total
            .cmp(&left.total)
            .then_with(|| left.broker_id.cmp(&right.broker_id))
    });
    stats
}

fn broker_leaderboard_value(requests: &[RequestRow]) -> Vec<Value> {
    broker_leaderboard(requests)
        .into_iter()
        .map(|stat| {
            let average = if stat.times.is_empty() {
                None
            } else {
                Some(stat.times.iter().sum::<f64>() / stat.times.len() as f64)
            };
            json!({
                "broker_id": stat.broker_id,
                "total": stat.total,
                "confirmed": stat.confirmed,
                "rejected": stat.rejected,
                "overdue": stat.overdue,
                "pending": stat.pending,
                "success_rate": round1(stat.confirmed as f64 / stat.total.max(1) as f64 * 100.0),
                "avg_response_time_days": round1_or_null(average),
            })
        })
        .collect()
}

/// Go's `brokerDashboard`: the leaderboard without the rate fields.
fn broker_dashboard(requests: &[RequestRow]) -> Vec<Value> {
    broker_leaderboard(requests)
        .into_iter()
        .map(|stat| {
            json!({
                "broker_id": stat.broker_id,
                "total": stat.total,
                "confirmed": stat.confirmed,
                "rejected": stat.rejected,
                "overdue": stat.overdue,
                "pending": stat.pending,
            })
        })
        .collect()
}

/// Go's `jurisdictionBreakdown`, ordered by total descending.
fn jurisdiction_breakdown(requests: &[RequestRow]) -> Vec<Value> {
    struct JurisdictionStat {
        key: String,
        total: i64,
        confirmed: i64,
        rejected: i64,
        overdue: i64,
    }
    let mut stats: Vec<JurisdictionStat> = Vec::new();
    for request in requests {
        let key = {
            let upper = request.jurisdiction.to_uppercase();
            if upper.is_empty() {
                "UNKNOWN".to_owned()
            } else {
                upper
            }
        };
        let index = match stats.iter().position(|stat| stat.key == key) {
            Some(index) => index,
            None => {
                stats.push(JurisdictionStat {
                    key,
                    total: 0,
                    confirmed: 0,
                    rejected: 0,
                    overdue: 0,
                });
                stats.len() - 1
            }
        };
        let stat = &mut stats[index];
        stat.total += 1;
        match request.current_status.to_uppercase().as_str() {
            "CONFIRMED" => stat.confirmed += 1,
            "REJECTED_FINAL" => stat.rejected += 1,
            "OVERDUE" => stat.overdue += 1,
            _ => {}
        }
    }
    // Total descending, then jurisdiction ascending, mirroring Go: its
    // `order` slice is first-seen, so equal totals followed the input order
    // and the output was not a function of the data alone (#963).
    stats.sort_by(|left, right| {
        right
            .total
            .cmp(&left.total)
            .then_with(|| left.key.cmp(&right.key))
    });
    stats
        .into_iter()
        .map(|stat| {
            json!({
                "jurisdiction": stat.key,
                "total": stat.total,
                "confirmed": stat.confirmed,
                "rejected": stat.rejected,
                "overdue": stat.overdue,
                "confirmation_rate": round1(stat.confirmed as f64 / stat.total.max(1) as f64 * 100.0),
            })
        })
        .collect()
}

/// Go's `buildTimeline`: events grouped by their first ten timestamp characters.
fn build_timeline(events: &[EventRow]) -> Vec<Value> {
    let mut counts: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
    for event in events {
        let day = if event.occurred_at.len() >= 10 {
            event.occurred_at[..10].to_owned()
        } else {
            "unknown".to_owned()
        };
        *counts
            .entry(day)
            .or_default()
            .entry(event.event_type.clone())
            .or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(date, per_type)| {
            let total: i64 = per_type.values().sum();
            json!({
                "date": date,
                "total_events": total,
                "events": counts_to_value(&per_type),
            })
        })
        .collect()
}

/// Go's `historicalComparison`: the two newest campaign aggregates.
fn historical_comparison(aggregates: &[Value]) -> Value {
    if aggregates.len() < 2 {
        return json!({});
    }
    let latest = &aggregates[0];
    let previous = &aggregates[1];
    let number = |value: &Value, key: &str| value.get(key).and_then(Value::as_f64);
    let change = |key: &str| match (number(latest, key), number(previous, key)) {
        (Some(a), Some(b)) => json!(round1(a - b)),
        _ => Value::Null,
    };
    json!({
        "latest_campaign": latest.get("campaign_id").cloned().unwrap_or(Value::Null),
        "previous_campaign": previous.get("campaign_id").cloned().unwrap_or(Value::Null),
        "requests_change": number(latest, "total").unwrap_or(0.0) as i64
            - number(previous, "total").unwrap_or(0.0) as i64,
        "confirmation_rate_change": change("confirmation_rate"),
        "rejection_rate_change": change("rejection_rate"),
        "avg_response_time_change": change("avg_response_time_days"),
    })
}

/// Go's `successMetrics`. The median is the raw middle value, not rounded.
fn success_metrics(requests: &[RequestRow]) -> Value {
    if requests.is_empty() {
        return json!({});
    }
    let (mut confirmed, mut rejected, mut overdue) = (0i64, 0i64, 0i64);
    for request in requests {
        match request.current_status.to_uppercase().as_str() {
            "CONFIRMED" => confirmed += 1,
            "REJECTED_FINAL" => rejected += 1,
            "OVERDUE" => overdue += 1,
            _ => {}
        }
    }
    let mut times = response_times(requests);
    let (mut average, mut median) = (Value::Null, Value::Null);
    if !times.is_empty() {
        times.sort_by(|left, right| left.partial_cmp(right).expect("no NaN durations"));
        average = json!(round1(times.iter().sum::<f64>() / times.len() as f64));
        median = if times.len() % 2 == 1 {
            json!(times[times.len() / 2])
        } else {
            json!((times[times.len() / 2 - 1] + times[times.len() / 2]) / 2.0)
        };
    }
    let total = requests.len() as f64;
    json!({
        "total_requests": requests.len() as i64,
        "overall_confirmation_rate": round1(confirmed as f64 / total * 100.0),
        "overall_rejection_rate": round1(rejected as f64 / total * 100.0),
        "overdue_rate": round1(overdue as f64 / total * 100.0),
        "avg_response_time_days": average,
        "median_response_time_days": median,
    })
}

/// Mirrors Go's `GetReportData`.
pub fn get_report_data(
    store: &Store,
    opts: &ReportOpts,
    now: DateTime<Utc>,
) -> rusqlite::Result<Value> {
    let campaigns = load_campaigns(store, &opts.campaign_id, opts.all_campaigns)?;
    if campaigns.is_empty() {
        return Ok(empty_report(&opts.campaign_id, now));
    }
    let ids: Vec<String> = campaigns.iter().map(|row| row.id.clone()).collect();
    let requests = load_requests(store, &ids)?;
    let events = load_events(store, &requests)?;
    let mut by_campaign: BTreeMap<String, Vec<RequestRow>> = BTreeMap::new();
    for request in &requests {
        by_campaign
            .entry(request.campaign_id.clone())
            .or_default()
            .push(request.clone());
    }
    let mut by_request: BTreeMap<i64, Vec<EventRow>> = BTreeMap::new();
    for event in &events {
        by_request
            .entry(event.request_id)
            .or_default()
            .push(event.clone());
    }
    let aggregates: Vec<Value> = campaigns
        .iter()
        .map(|campaign| {
            let empty = Vec::new();
            let requests = by_campaign.get(&campaign.id).unwrap_or(&empty);
            aggregate_campaign(campaign, requests, &by_request)
        })
        .collect();
    let statuses = count_statuses(&requests);
    Ok(go_map_order(json!({
        "generated_at": iso(now),
        "campaigns": aggregates,
        "total_campaigns": aggregates.len() as i64,
        "total_requests": requests.len() as i64,
        "status_breakdown": counts_to_value(&statuses),
        "broker_leaderboard": broker_leaderboard_value(&requests),
        "jurisdiction_stats": jurisdiction_breakdown(&requests),
        "timeline": build_timeline(&events),
        "historical_comparison": historical_comparison(&aggregates),
        "success_metrics": success_metrics(&requests),
    })))
}

/// Go's `recentEvents`: the newest events joined with their broker.
fn recent_events(store: &Store, limit: i64) -> rusqlite::Result<Vec<Value>> {
    let query = "SELECT e.id,e.request_id,e.event_type,e.occurred_at,e.source,r.broker_id\n\
                 FROM request_events e JOIN removal_requests r ON r.id=e.request_id \
                 ORDER BY e.occurred_at DESC,e.id DESC LIMIT ?";
    let connection = store.connection();
    let mut statement = connection.prepare(query)?;
    let rows = statement.query_map([limit], |row| {
        let occurred_at: String = row.get(3)?;
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "request_id": row.get::<_, i64>(1)?,
            "event_type": row.get::<_, String>(2)?,
            "occurred_at": py_timestamp(&occurred_at),
            "source": row.get::<_, String>(4)?,
            "broker_id": row.get::<_, String>(5)?,
        }))
    })?;
    rows.collect()
}

/// Mirrors Go's `GetDashboardData`.
pub fn get_dashboard_data(
    store: &Store,
    campaign_id: &str,
    now: DateTime<Utc>,
) -> rusqlite::Result<Value> {
    let campaigns = load_campaigns(store, campaign_id, campaign_id.is_empty())?;
    let ids: Vec<String> = campaigns.iter().map(|row| row.id.clone()).collect();
    let requests = if ids.is_empty() {
        Vec::new()
    } else {
        load_requests(store, &ids)?
    };
    let mut by_campaign: BTreeMap<String, Vec<RequestRow>> = BTreeMap::new();
    for request in &requests {
        by_campaign
            .entry(request.campaign_id.clone())
            .or_default()
            .push(request.clone());
    }
    let campaign_values: Vec<Value> = campaigns
        .iter()
        .map(|campaign| {
            let empty = Vec::new();
            let scoped = by_campaign.get(&campaign.id).unwrap_or(&empty);
            let counts = count_statuses(scoped);
            let request_values: Vec<Value> = scoped.iter().map(dashboard_request_map).collect();
            json!({
                "id": campaign.id,
                "created_at": campaign.created_at,
                "kind": campaign.kind,
                "requests": request_values,
                "total": scoped.len() as i64,
                "planned": count_of(&counts, "PLANNED"),
                "sent": count_of(&counts, "SENT"),
                "awaiting_ack": count_of(&counts, "AWAITING_ACK"),
                "awaiting_response": count_of(&counts, "AWAITING_RESPONSE"),
                "confirmed": count_of(&counts, "CONFIRMED"),
                "rejected": count_of(&counts, "REJECTED_FINAL"),
                "overdue": count_of(&counts, "OVERDUE"),
            })
        })
        .collect();
    let events = recent_events(store, 50)?;
    let counts = count_statuses(&requests);
    Ok(go_map_order(json!({
        "campaigns": campaign_values,
        "total_requests": requests.len() as i64,
        "planned": count_of(&counts, "PLANNED"),
        "sent": count_of(&counts, "SENT"),
        "awaiting_ack": count_of(&counts, "AWAITING_ACK"),
        "awaiting_response": count_of(&counts, "AWAITING_RESPONSE"),
        "confirmed": count_of(&counts, "CONFIRMED"),
        "rejected": count_of(&counts, "REJECTED_FINAL"),
        "overdue": count_of(&counts, "OVERDUE"),
        "broker_status": broker_dashboard(&requests),
        "recent_events": events,
        "generated_at": iso(now),
    })))
}

/// Go's `GenerateDashboard`: render the dashboard template over the dashboard
/// data. `refresh` seeds `auto_refresh_seconds`, exactly like Go's extra vars.
pub fn generate_dashboard(
    data: &Value,
    refresh: i64,
    now: DateTime<Utc>,
) -> Result<String, String> {
    let context = RenderContext {
        data: data.clone(),
        now: FrozenDateTime::from_rfc3339(now.to_rfc3339()).map_err(|error| error.to_string())?,
        extra: BTreeMap::from([("auto_refresh_seconds".to_owned(), json!(refresh))]),
        ..RenderContext::default()
    };
    render("dashboard.html.j2", &context).map_err(|error| error.to_string())
}

/// Go's `GenerateReport`: JSON, CSV or HTML over the report data. The format
/// match is ASCII case-insensitive, like Go's `strings.ToLower`.
pub fn generate_report(data: &Value, format: &str, now: DateTime<Utc>) -> Result<String, String> {
    match format.to_ascii_lowercase().as_str() {
        "json" => export_json(data),
        "csv" => Ok(export_csv(data)),
        "html" => export_html(data, now),
        _ => Err(format!(
            "unsupported format: {format}; choose html, json, or csv"
        )),
    }
}

/// Go's `ExportJSON`: two-space indent, no HTML escaping, no trailing newline.
///
/// serde_json never escapes `<`, `>` or `&` (its escape table covers only
/// control characters, quotes and backslashes), which is exactly Go's
/// `SetEscapeHTML(false)` behavior. `to_string_pretty` indents with two
/// spaces and appends no trailing newline, matching Go's trimmed encoder.
fn export_json(data: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(data).map_err(|error| error.to_string())
}

/// Go's `ExportCSV`: CRLF rows, the fixed twelve-column header, one row per
/// request of every campaign. Quoting follows `encoding/csv`: a field is
/// quoted when it contains a comma, quote, `\r` or `\n`, and an embedded
/// quote is doubled.
fn export_csv(data: &Value) -> String {
    const HEADER: [&str; 12] = [
        "campaign_id",
        "request_id",
        "broker_id",
        "jurisdiction",
        "channel",
        "status",
        "sent_at",
        "acknowledged_at",
        "resolved_at",
        "deadline_at",
        "reminders_sent",
        "escalation_level",
    ];
    /// Go's `text`: nil renders empty, anything else renders as-is.
    fn cell(value: Option<&Value>) -> String {
        match value {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(text)) => text.clone(),
            Some(Value::Number(number)) => number.to_string(),
            Some(Value::Bool(flag)) => flag.to_string(),
            Some(Value::Array(_)) | Some(Value::Object(_)) => String::new(),
        }
    }
    fn field(value: &str, row: &mut String) {
        if value.contains([',', '"', '\r', '\n']) {
            row.push('"');
            row.push_str(&value.replace('"', "\"\""));
            row.push('"');
        } else {
            row.push_str(value);
        }
    }
    let mut out = String::new();
    let mut header = String::new();
    for (index, column) in HEADER.iter().enumerate() {
        if index > 0 {
            header.push(',');
        }
        header.push_str(column);
    }
    out.push_str(&header);
    out.push_str("\r\n");
    for campaign in data
        .get("campaigns")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let campaign_id = cell(campaign.get("campaign_id"));
        for request in campaign
            .get("requests")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let cells = [
                campaign_id.clone(),
                cell(request.get("id")),
                cell(request.get("broker_id")),
                cell(request.get("jurisdiction")),
                cell(request.get("channel")),
                cell(request.get("current_status")),
                cell(request.get("sent_at")),
                cell(request.get("acknowledged_at")),
                cell(request.get("resolved_at")),
                cell(request.get("deadline_at")),
                cell(request.get("reminders_sent")),
                cell(request.get("escalation_level")),
            ];
            let mut row = String::new();
            for (index, value) in cells.iter().enumerate() {
                if index > 0 {
                    row.push(',');
                }
                field(value, &mut row);
            }
            out.push_str(&row);
            out.push_str("\r\n");
        }
    }
    out
}

/// Go's `ExportHTML`: the report template over the report data.
fn export_html(data: &Value, now: DateTime<Utc>) -> Result<String, String> {
    // Protect data newlines while matching Go's control-tag whitespace. A
    // rendered report can contain caller-controlled multiline text, so only
    // blank lines emitted by the template may be compacted below.
    let mut template_data = data.clone();
    encode_data_newlines(&mut template_data);
    let context = RenderContext {
        data: template_data,
        now: FrozenDateTime::from_rfc3339(now.to_rfc3339()).map_err(|error| error.to_string())?,
        ..RenderContext::default()
    };
    let html = render("report.html.j2", &context).map_err(|error| error.to_string())?;
    let mut compact = String::with_capacity(html.len());
    let mut line_breaks = 0;
    for character in html.chars() {
        if character == '\n' {
            line_breaks += 1;
            if line_breaks <= 2 {
                compact.push(character);
            }
        } else {
            line_breaks = 0;
            compact.push(character);
        }
    }
    Ok(decode_data_newlines(&compact))
}

const NEWLINE_MARKER: char = '\u{e001}';
const ESCAPE_MARKER: char = '\u{e000}';

fn encode_data_newlines(value: &mut Value) {
    match value {
        Value::String(text) => {
            let mut encoded = String::with_capacity(text.len());
            for character in text.chars() {
                match character {
                    '\n' => encoded.push(NEWLINE_MARKER),
                    ESCAPE_MARKER => {
                        encoded.push(ESCAPE_MARKER);
                        encoded.push('0');
                    }
                    NEWLINE_MARKER => {
                        encoded.push(ESCAPE_MARKER);
                        encoded.push('1');
                    }
                    other => encoded.push(other),
                }
            }
            *text = encoded;
        }
        Value::Array(values) => values.iter_mut().for_each(encode_data_newlines),
        Value::Object(values) => values.values_mut().for_each(encode_data_newlines),
        _ => {}
    }
}

fn decode_data_newlines(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        match character {
            NEWLINE_MARKER => decoded.push('\n'),
            ESCAPE_MARKER => match characters.next() {
                Some('0') => decoded.push(ESCAPE_MARKER),
                Some('1') => decoded.push(NEWLINE_MARKER),
                Some(other) => {
                    decoded.push(ESCAPE_MARKER);
                    decoded.push(other);
                }
                None => decoded.push(ESCAPE_MARKER),
            },
            other => decoded.push(other),
        }
    }
    decoded
}

#[cfg(test)]
mod report_html_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn html_report_preserves_multiline_dynamic_fields() {
        let data = json!({"campaigns": [{"campaign_id": "first\n\n\nlast\u{e000}\u{e001}"}]});
        let now = DateTime::parse_from_rfc3339("2026-01-02T03:04:00Z")
            .unwrap()
            .to_utc();
        let html = export_html(&data, now).expect("render report");
        assert!(html.contains("Campaign: first\n\n\nlast\u{e000}\u{e001}"));
    }
}
