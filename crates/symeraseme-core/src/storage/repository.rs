//! Typed read and small write queries for the event-store schema.

use super::{store::Store, types};
use crate::timeutil;
use rusqlite::{OptionalExtension, Row, params, params_from_iter, types::Value};
use serde_json::{Map, Value as JsonValue};
use std::collections::BTreeMap;

/// Optional filters and pagination for removal-request listing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListRemovalRequestsOptions {
    pub campaign_id: Option<String>,
    pub status: Option<String>,
    pub broker_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Query façade over a [`Store`].
pub struct Repository<'store> {
    store: &'store Store,
}

impl<'store> Repository<'store> {
    pub fn new(store: &'store Store) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &'store Store {
        self.store
    }

    /// Inserts a campaign, returning false when the id already exists.
    pub fn create_campaign(&self, id: &str, kind: &str, notes: &str) -> rusqlite::Result<bool> {
        let kind = if kind.is_empty() { "initial" } else { kind };
        let changed = self.store.connection().execute(
            "INSERT OR IGNORE INTO campaigns (id, kind, notes) VALUES (?1, ?2, ?3)",
            params![id, kind, notes],
        )?;
        Ok(changed == 1)
    }

    /// Lists campaigns in the Go oracle's newest-first order.
    pub fn list_campaigns(&self) -> rusqlite::Result<Vec<types::Campaign>> {
        let mut statement = self.store.connection().prepare(
            "SELECT id, created_at, kind, notes FROM campaigns ORDER BY created_at DESC",
        )?;
        statement
            .query_map([], |row| {
                Ok(types::Campaign {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    kind: row.get(2)?,
                    notes: row.get(3)?,
                })
            })?
            .collect()
    }

    /// Inserts a removal request and returns its SQLite row id.
    pub fn create_removal_request(
        &self,
        broker_id: &str,
        channel: &str,
        campaign_id: &str,
        jurisdiction: &str,
        template_id: &str,
        identity_snapshot_hash: &str,
    ) -> rusqlite::Result<i64> {
        let channel = if channel.is_empty() { "email" } else { channel };
        self.store.connection().execute(
            "INSERT INTO removal_requests
             (broker_id, channel, campaign_id, jurisdiction, template_id, identity_snapshot_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                broker_id,
                channel,
                campaign_id,
                jurisdiction,
                template_id,
                identity_snapshot_hash
            ],
        )?;
        Ok(self.store.connection().last_insert_rowid())
    }

    /// Fetches one removal request, returning None for a missing id.
    pub fn get_removal_request(&self, id: i64) -> rusqlite::Result<Option<types::RemovalRequest>> {
        self.store
            .connection()
            .query_row(
                "SELECT id, broker_id, channel, campaign_id, created_at, jurisdiction,
                        template_id, identity_snapshot_hash
                 FROM removal_requests WHERE id = ?1",
                params![id],
                |row| {
                    Ok(types::RemovalRequest {
                        id: row.get(0)?,
                        broker_id: row.get(1)?,
                        channel: row.get(2)?,
                        campaign_id: row.get(3)?,
                        created_at: row.get(4)?,
                        jurisdiction: row.get(5)?,
                        template_id: row.get(6)?,
                        identity_snapshot_hash: row.get(7)?,
                    })
                },
            )
            .optional()
    }

    /// Lists requests with the same optional filters, pagination and order as
    /// the Go repository.
    pub fn list_removal_requests(
        &self,
        options: ListRemovalRequestsOptions,
    ) -> rusqlite::Result<Vec<types::RemovalRequestRow>> {
        let mut query = String::from(
            "SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
                    r.jurisdiction, r.template_id, r.identity_snapshot_hash,
                    s.current_status, s.last_event_at, s.sent_at, s.acknowledged_at,
                    s.resolved_at, s.deadline_at, s.next_action_at, s.reminders_sent,
                    s.escalation_level
             FROM removal_requests r
             LEFT JOIN request_state s ON s.request_id = r.id
             WHERE (?1 IS NULL OR r.campaign_id = ?2)
               AND (?3 IS NULL OR s.current_status = ?4)
               AND (?5 IS NULL OR r.broker_id = ?6)
             ORDER BY r.created_at ASC",
        );
        let mut arguments = vec![
            nullable_text(options.campaign_id.as_deref()),
            nullable_text(options.campaign_id.as_deref()),
            nullable_text(options.status.as_deref()),
            nullable_text(options.status.as_deref()),
            nullable_text(options.broker_id.as_deref()),
            nullable_text(options.broker_id.as_deref()),
        ];
        if options.limit.is_some() {
            query.push_str(" LIMIT ?");
            arguments.push(Value::Integer(options.limit.unwrap_or_default()));
            if let Some(offset) = options.offset {
                query.push_str(" OFFSET ?");
                arguments.push(Value::Integer(offset));
            }
        } else if let Some(offset) = options.offset {
            query.push_str(" LIMIT -1 OFFSET ?");
            arguments.push(Value::Integer(offset));
        }

        let mut statement = self.store.connection().prepare(&query)?;
        let rows = statement.query_map(params_from_iter(arguments), request_row)?;
        rows.collect()
    }

    /// Counts requests with optional campaign and status filters.
    pub fn count_removal_requests(
        &self,
        campaign_id: Option<&str>,
        status: Option<&str>,
    ) -> rusqlite::Result<i64> {
        self.store.connection().query_row(
            "SELECT COUNT(*) FROM removal_requests r
             LEFT JOIN request_state s ON s.request_id = r.id
             WHERE (?1 IS NULL OR r.campaign_id = ?2)
               AND (?3 IS NULL OR s.current_status = ?4)",
            params![campaign_id, campaign_id, status, status],
            |row| row.get(0),
        )
    }

    /// Lists requests that are not in either terminal status.
    pub fn get_active_matchable_requests(
        &self,
        campaign_id: Option<&str>,
    ) -> rusqlite::Result<Vec<types::RemovalRequestRow>> {
        let mut statement = self.store.connection().prepare(
            "SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
                    r.jurisdiction, r.template_id, r.identity_snapshot_hash,
                    s.current_status, s.last_event_at, s.sent_at, s.acknowledged_at,
                    s.resolved_at, s.deadline_at, s.next_action_at, s.reminders_sent,
                    s.escalation_level
             FROM removal_requests r
             LEFT JOIN request_state s ON s.request_id = r.id
             WHERE (?1 IS NULL OR r.campaign_id = ?2)
               AND (s.current_status IS NULL
                    OR s.current_status NOT IN ('CONFIRMED', 'REJECTED_FINAL'))
             ORDER BY r.created_at ASC",
        )?;
        statement
            .query_map(params![campaign_id, campaign_id], request_row)?
            .collect()
    }

    /// Reads one request's events in replay order, skipping rows the Go
    /// decoder cannot materialise.
    pub fn get_events(
        &self,
        request_id: i64,
        after_event_id: i64,
    ) -> rusqlite::Result<Vec<types::EventRecord>> {
        let mut query = String::from(
            "SELECT id, request_id, occurred_at, recorded_at, event_type, payload_json, source
             FROM request_events WHERE request_id = ?1",
        );
        let mut arguments = vec![Value::Integer(request_id)];
        if after_event_id > 0 {
            query.push_str(" AND id > ?");
            arguments.push(Value::Integer(after_event_id));
        }
        query.push_str(" ORDER BY occurred_at ASC, id ASC");

        let mut statement = self.store.connection().prepare(&query)?;
        let mut rows = statement.query(params_from_iter(arguments))?;
        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            if let Some(event) = decode_event(row) {
                events.push(event);
            }
        }
        Ok(events)
    }

    /// Reads events for several requests, preserving an empty bucket for each
    /// requested id and the oracle's `(occurred_at,id)` order.
    pub fn get_events_for_requests(
        &self,
        request_ids: &[i64],
        event_type: Option<&types::EventType>,
    ) -> rusqlite::Result<BTreeMap<i64, Vec<types::EventRecord>>> {
        let mut result = BTreeMap::new();
        for request_id in request_ids {
            result.entry(*request_id).or_insert_with(Vec::new);
        }
        if request_ids.is_empty() {
            return Ok(result);
        }

        let placeholders = std::iter::repeat_n("?", request_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let mut query = format!(
            "SELECT id, request_id, occurred_at, recorded_at, event_type, payload_json, source
             FROM request_events WHERE request_id IN ({placeholders})"
        );
        let mut arguments = request_ids
            .iter()
            .copied()
            .map(Value::Integer)
            .collect::<Vec<_>>();
        if let Some(event_type) = event_type {
            query.push_str(" AND event_type = ?");
            arguments.push(Value::Text(event_type.as_str().to_owned()));
        }
        query.push_str(" ORDER BY occurred_at ASC, id ASC");

        let mut statement = self.store.connection().prepare(&query)?;
        let mut rows = statement.query(params_from_iter(arguments))?;
        while let Some(row) = rows.next()? {
            if let Some(event) = decode_event(row) {
                result
                    .entry(event.request_id)
                    .or_insert_with(Vec::new)
                    .push(event);
            }
        }
        Ok(result)
    }

    /// Reads due or unscheduled requests in the Go tick-query order.
    pub fn fetch_tick_candidates(
        &self,
        now_iso: &str,
        batch_size: i64,
    ) -> rusqlite::Result<Vec<types::TickCandidate>> {
        let mut query = String::from(
            "SELECT r.id, r.broker_id, r.campaign_id, r.jurisdiction,
                    s.current_status, s.sent_at, s.deadline_at, s.next_action_at,
                    s.acknowledged_at, s.resolved_at, s.reminders_sent,
                    s.escalation_level
             FROM removal_requests r
             JOIN request_state s ON s.request_id = r.id
             WHERE s.next_action_at IS NULL OR s.next_action_at <= ?1
             ORDER BY s.next_action_at ASC",
        );
        if batch_size > 0 {
            query.push_str(" LIMIT ?");
        }

        let mut statement = self.store.connection().prepare(&query)?;
        let rows = if batch_size > 0 {
            statement.query_map(params![now_iso, batch_size], tick_candidate)?
        } else {
            statement.query_map(params![now_iso], tick_candidate)?
        };
        rows.collect()
    }
}

fn nullable_text(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |value| Value::Text(value.to_owned()))
}

fn request_row(row: &Row<'_>) -> rusqlite::Result<types::RemovalRequestRow> {
    Ok(types::RemovalRequestRow {
        id: row.get(0)?,
        broker_id: row.get(1)?,
        channel: row.get(2)?,
        campaign_id: row.get(3)?,
        created_at: row.get(4)?,
        jurisdiction: row.get(5)?,
        template_id: row.get(6)?,
        identity_snapshot_hash: row.get(7)?,
        current_status: row.get(8)?,
        last_event_at: row.get(9)?,
        sent_at: row.get(10)?,
        acknowledged_at: row.get(11)?,
        resolved_at: row.get(12)?,
        deadline_at: row.get(13)?,
        next_action_at: row.get(14)?,
        reminders_sent: row.get::<_, Option<i64>>(15)?.unwrap_or_default(),
        escalation_level: row.get::<_, Option<i64>>(16)?.unwrap_or_default(),
    })
}

fn decode_event(row: &Row<'_>) -> Option<types::EventRecord> {
    let occurred_at: String = row.get(2).ok()?;
    let recorded_at: String = row.get(3).ok()?;
    let occurred_at = timeutil::parse_timestamp(&occurred_at).ok()?;
    let recorded_at = timeutil::parse_timestamp(&recorded_at).ok()?;
    let payload_json: String = row.get(5).ok()?;
    let payload = match payload_json.trim() {
        "" | "null" => Map::new(),
        value => match serde_json::from_str::<JsonValue>(value).ok()? {
            JsonValue::Object(payload) => payload,
            _ => return None,
        },
    };
    Some(types::EventRecord {
        id: row.get(0).ok()?,
        request_id: row.get(1).ok()?,
        occurred_at,
        recorded_at,
        event_type: types::EventType::from_wire(row.get(4).ok()?),
        payload,
        source: types::Source::from_wire(row.get(6).ok()?),
    })
}

fn tick_candidate(row: &Row<'_>) -> rusqlite::Result<types::TickCandidate> {
    Ok(types::TickCandidate {
        id: row.get(0)?,
        broker_id: row.get(1)?,
        campaign_id: row.get(2)?,
        jurisdiction: row.get(3)?,
        current_status: row.get(4)?,
        sent_at: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        deadline_at: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        next_action_at: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        acknowledged_at: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        resolved_at: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
        reminders_sent: row.get(10)?,
        escalation_level: row.get(11)?,
    })
}
