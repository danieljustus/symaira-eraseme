//! Safe deletion of a never-sent campaign (`plan delete`, #1297).
//!
//! A campaign created by mistake (for example `plan create` run with the wrong
//! `--max`) leaves PLANNED requests that no command can remove. This module
//! deletes such a campaign, and only such a campaign: every request must still
//! be PLANNED and no table may hold evidence about any of its requests.
//!
//! The check and the deletion run in one `IMMEDIATE` transaction, so a
//! concurrent writer cannot add evidence between the two, and a failing
//! statement rolls everything back.

use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use serde::Serialize;

use super::store::Store;

/// Row counts for one deletion (or, for a dry run, the rows it would delete).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CampaignDeleteCounts {
    pub campaigns: i64,
    pub removal_requests: i64,
    pub request_events: i64,
    pub request_state: i64,
}

/// Outcome of [`delete_never_sent_campaign`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampaignDeleteResult {
    pub campaign_id: String,
    pub dry_run: bool,
    pub counts: CampaignDeleteCounts,
}

/// Why a campaign was not deleted.
#[derive(Debug)]
pub enum CampaignDeleteError {
    /// Neither a campaign row nor a request carries the id.
    NotFound(String),
    /// The campaign has requests beyond PLANNED or evidence attached.
    Refused {
        campaign_id: String,
        reasons: Vec<String>,
    },
    Database(rusqlite::Error),
}

impl std::fmt::Display for CampaignDeleteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(formatter, "campaign {id:?} not found"),
            Self::Refused {
                campaign_id,
                reasons,
            } => write!(
                formatter,
                "refusing to delete campaign {campaign_id:?}: {}",
                reasons.join("; ")
            ),
            Self::Database(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for CampaignDeleteError {}

impl From<rusqlite::Error> for CampaignDeleteError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

/// Deletes `campaign_id` with its requests, events and projections, but only
/// when it was never sent. With `dry_run` it checks and counts without
/// deleting.
pub fn delete_never_sent_campaign(
    store: &Store,
    campaign_id: &str,
    dry_run: bool,
) -> Result<CampaignDeleteResult, CampaignDeleteError> {
    delete_in_connection(store.connection(), campaign_id, dry_run)
}

fn delete_in_connection(
    connection: &Connection,
    campaign_id: &str,
    dry_run: bool,
) -> Result<CampaignDeleteResult, CampaignDeleteError> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let counts = inspect(&transaction, campaign_id)?;
    if dry_run {
        // Dropping the transaction rolls it back; nothing was written.
        return Ok(CampaignDeleteResult {
            campaign_id: campaign_id.to_owned(),
            dry_run,
            counts,
        });
    }
    let requests = "SELECT id FROM removal_requests WHERE campaign_id = ?1";
    transaction.execute(
        &format!("DELETE FROM request_events WHERE request_id IN ({requests})"),
        params![campaign_id],
    )?;
    transaction.execute(
        &format!("DELETE FROM request_state WHERE request_id IN ({requests})"),
        params![campaign_id],
    )?;
    transaction.execute(
        "DELETE FROM removal_requests WHERE campaign_id = ?1",
        params![campaign_id],
    )?;
    transaction.execute("DELETE FROM campaigns WHERE id = ?1", params![campaign_id])?;
    transaction.commit()?;
    Ok(CampaignDeleteResult {
        campaign_id: campaign_id.to_owned(),
        dry_run,
        counts,
    })
}

fn count(transaction: &Transaction<'_>, sql: &str, campaign_id: &str) -> rusqlite::Result<i64> {
    transaction.query_row(sql, params![campaign_id], |row| row.get(0))
}

/// Verifies the campaign is deletable and counts the rows a deletion removes.
fn inspect(
    transaction: &Transaction<'_>,
    campaign_id: &str,
) -> Result<CampaignDeleteCounts, CampaignDeleteError> {
    const REQUESTS: &str = "(SELECT id FROM removal_requests WHERE campaign_id = ?1)";
    let campaigns = count(
        transaction,
        "SELECT COUNT(*) FROM campaigns WHERE id = ?1",
        campaign_id,
    )?;
    let removal_requests = count(
        transaction,
        "SELECT COUNT(*) FROM removal_requests WHERE campaign_id = ?1",
        campaign_id,
    )?;
    if campaigns == 0 && removal_requests == 0 {
        return Err(CampaignDeleteError::NotFound(campaign_id.to_owned()));
    }

    let mut reasons = Vec::new();

    // Every request must still be PLANNED.
    let mut statement = transaction.prepare(
        "SELECT r.id, COALESCE(s.current_status, '(no state)')
         FROM removal_requests r LEFT JOIN request_state s ON s.request_id = r.id
         WHERE r.campaign_id = ?1 AND COALESCE(s.current_status, '') <> 'PLANNED'
         ORDER BY r.id",
    )?;
    let not_planned = statement
        .query_map(params![campaign_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    if !not_planned.is_empty() {
        let shown = not_planned
            .iter()
            .take(5)
            .map(|(id, status)| format!("#{id} is {status}"))
            .collect::<Vec<_>>()
            .join(", ");
        let more = not_planned.len().saturating_sub(5);
        reasons.push(format!(
            "{} request(s) are not PLANNED ({shown}{})",
            not_planned.len(),
            if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            }
        ));
    }

    // The only event a never-sent request may have is its PLANNED event.
    let other_events = count(
        transaction,
        &format!(
            "SELECT COUNT(*) FROM request_events
             WHERE request_id IN {REQUESTS} AND event_type <> 'PLANNED'"
        ),
        campaign_id,
    )?;
    if other_events > 0 {
        reasons.push(format!("{other_events} non-PLANNED event(s) recorded"));
    }

    // Projected delivery evidence must be empty.
    let progressed = count(
        transaction,
        &format!(
            "SELECT COUNT(*) FROM request_state
             WHERE request_id IN {REQUESTS}
               AND (sent_at IS NOT NULL OR acknowledged_at IS NOT NULL
                    OR resolved_at IS NOT NULL OR reminders_sent <> 0
                    OR escalation_level <> 0)"
        ),
        campaign_id,
    )?;
    if progressed > 0 {
        reasons.push(format!(
            "{progressed} request(s) carry sent/acknowledged/resolved/reminder state"
        ));
    }

    // Tables that reference a removal request: replies, drafts, manual tasks.
    for (table, label) in [
        ("inbox_replies", "inbox reply"),
        ("reply_drafts", "reply draft"),
        ("manual_tasks", "manual task"),
    ] {
        let found = count(
            transaction,
            &format!("SELECT COUNT(*) FROM {table} WHERE request_id IN {REQUESTS}"),
            campaign_id,
        )?;
        if found > 0 {
            reasons.push(format!("{found} {label}(s) attached"));
        }
    }

    if !reasons.is_empty() {
        return Err(CampaignDeleteError::Refused {
            campaign_id: campaign_id.to_owned(),
            reasons,
        });
    }

    Ok(CampaignDeleteCounts {
        campaigns,
        removal_requests,
        request_events: count(
            transaction,
            &format!("SELECT COUNT(*) FROM request_events WHERE request_id IN {REQUESTS}"),
            campaign_id,
        )?,
        request_state: count(
            transaction,
            &format!("SELECT COUNT(*) FROM request_state WHERE request_id IN {REQUESTS}"),
            campaign_id,
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::{EventType, Source};
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::Map;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap()
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(dir.path().join("symeraseme.db")).expect("store");
        (dir, store)
    }

    fn plan(store: &Store, campaign: &str, brokers: &[&str]) -> Vec<i64> {
        store
            .connection()
            .execute(
                "INSERT OR IGNORE INTO campaigns (id, kind, notes) VALUES (?1, 'initial', '')",
                [campaign],
            )
            .unwrap();
        brokers
            .iter()
            .map(|broker| {
                store
                    .connection()
                    .execute(
                        "INSERT INTO removal_requests (broker_id, campaign_id, jurisdiction)
                         VALUES (?1, ?2, 'eu')",
                        params![broker, campaign],
                    )
                    .unwrap();
                let id = store.connection().last_insert_rowid();
                store
                    .append_and_project(
                        id,
                        &EventType::Planned,
                        &Map::new(),
                        &Source::System,
                        fixed_now(),
                    )
                    .unwrap();
                id
            })
            .collect()
    }

    fn table_counts(store: &Store) -> Vec<i64> {
        [
            "campaigns",
            "removal_requests",
            "request_events",
            "request_state",
        ]
        .iter()
        .map(|table| {
            store
                .connection()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap()
        })
        .collect()
    }

    fn refused(error: CampaignDeleteError) -> String {
        match error {
            CampaignDeleteError::Refused { reasons, .. } => reasons.join("; "),
            other => panic!("expected a refusal, got {other}"),
        }
    }

    #[test]
    fn unknown_campaign_is_an_error() {
        let (_dir, store) = store();
        let error = delete_never_sent_campaign(&store, "nope", false).unwrap_err();
        assert!(matches!(error, CampaignDeleteError::NotFound(_)));
        assert_eq!(error.to_string(), "campaign \"nope\" not found");
    }

    #[test]
    fn dry_run_counts_and_changes_nothing() {
        let (_dir, store) = store();
        plan(&store, "wrong", &["a", "b", "c"]);
        plan(&store, "keep", &["d"]);
        let before = table_counts(&store);
        let result = delete_never_sent_campaign(&store, "wrong", true).unwrap();
        assert!(result.dry_run);
        assert_eq!(
            result.counts,
            CampaignDeleteCounts {
                campaigns: 1,
                removal_requests: 3,
                request_events: 3,
                request_state: 3
            }
        );
        assert_eq!(table_counts(&store), before);
    }

    #[test]
    fn delete_removes_exactly_that_campaign() {
        let (_dir, store) = store();
        plan(&store, "wrong", &["a", "b"]);
        let kept = plan(&store, "keep", &["c"]);
        let result = delete_never_sent_campaign(&store, "wrong", false).unwrap();
        assert!(!result.dry_run);
        assert_eq!(result.counts.removal_requests, 2);
        assert_eq!(table_counts(&store), vec![1, 1, 1, 1]);
        let remaining: i64 = store
            .connection()
            .query_row("SELECT id FROM removal_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, kept[0]);
        assert!(matches!(
            delete_never_sent_campaign(&store, "wrong", false),
            Err(CampaignDeleteError::NotFound(_))
        ));
    }

    #[test]
    fn campaign_without_requests_can_be_deleted() {
        let (_dir, store) = store();
        plan(&store, "empty", &[]);
        let result = delete_never_sent_campaign(&store, "empty", false).unwrap();
        assert_eq!(result.counts.campaigns, 1);
        assert_eq!(table_counts(&store), vec![0, 0, 0, 0]);
    }

    #[test]
    fn refuses_a_request_that_is_not_planned() {
        let (_dir, store) = store();
        let ids = plan(&store, "sent", &["a", "b"]);
        store
            .append_and_project(
                ids[0],
                &EventType::Sent,
                &Map::new(),
                &Source::System,
                fixed_now(),
            )
            .unwrap();
        let before = table_counts(&store);
        let reasons = refused(delete_never_sent_campaign(&store, "sent", false).unwrap_err());
        assert!(
            reasons.contains("1 request(s) are not PLANNED"),
            "{reasons}"
        );
        assert!(reasons.contains("non-PLANNED event"), "{reasons}");
        assert_eq!(table_counts(&store), before);
    }

    #[test]
    fn refuses_attached_replies_drafts_and_manual_tasks() {
        // (expected reason fragment, statements; `?1` is the request id)
        let cases: [(&str, &[&str]); 3] = [
            (
                "inbox reply",
                &["INSERT INTO inbox_replies (request_id, message_id) VALUES (?1, 'm1')"],
            ),
            (
                "reply draft",
                &[
                    "INSERT INTO inbox_replies (id, message_id) VALUES (50, 'm2')",
                    "INSERT INTO reply_drafts (reply_id, request_id, draft_body) VALUES (50, ?1, 'x')",
                ],
            ),
            (
                "manual task",
                &["INSERT INTO manual_tasks (request_id, broker_id) VALUES (?1, 'a')"],
            ),
        ];
        for (label, statements) in cases {
            let (_dir, store) = store();
            let ids = plan(&store, "c", &["a"]);
            for statement in statements {
                let parameters: &[i64] = if statement.contains("?1") { &ids } else { &[] };
                store
                    .connection()
                    .execute(statement, rusqlite::params_from_iter(parameters))
                    .unwrap();
            }
            let before = table_counts(&store);
            let reasons = refused(delete_never_sent_campaign(&store, "c", false).unwrap_err());
            assert!(reasons.contains(label), "{label}: {reasons}");
            assert_eq!(table_counts(&store), before, "{label}");
        }
    }

    #[test]
    fn a_failing_statement_rolls_the_whole_deletion_back() {
        let (_dir, store) = store();
        plan(&store, "wrong", &["a", "b"]);
        // The last statement fails after events, state and requests are gone.
        store
            .connection()
            .execute_batch(
                "CREATE TRIGGER block_campaign_delete BEFORE DELETE ON campaigns
                 BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
            )
            .unwrap();
        let before = table_counts(&store);
        let error = delete_never_sent_campaign(&store, "wrong", false).unwrap_err();
        assert!(error.to_string().contains("blocked"), "{error}");
        assert_eq!(table_counts(&store), before);
        store
            .connection()
            .execute_batch("DROP TRIGGER block_campaign_delete")
            .unwrap();
        assert!(delete_never_sent_campaign(&store, "wrong", false).is_ok());
    }
}
