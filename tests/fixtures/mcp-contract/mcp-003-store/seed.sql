-- Frozen input rows for the MCP store-read contract (`get_events`,
-- `list_requests`, `plan_show`). One statement per line; both the Go oracle
-- (`rust-tests/parity/oracle/mcp-tools-call`) and the Rust test execute this
-- file verbatim, so the rows both sides read are identical.
--
-- Why frozen rows instead of a store the product wrote: the Go write path fills
-- `created_at`/`recorded_at` from `time.Now()` with no injection point, so a
-- store seeded through the product API would carry a wall clock and could not
-- be pinned. These rows are fixture input in the same sense as the registry
-- YAML the `validate` cases read; the contract under test is the read path.
-- The write paths carry their own contracts (event-store goldens).
--
-- `request_events` id 2 and 3 share `occurred_at` on purpose: the read path
-- orders by `(occurred_at, id)`, and a tie is where a port can silently drift.
-- `request_events` id 5 carries an event type the catalogue does not know, and
-- request 3 has no `request_state` row at all (the LEFT JOIN branch).
INSERT INTO removal_requests (id, broker_id, channel, campaign_id, created_at, jurisdiction, template_id, identity_snapshot_hash) VALUES (1, 'broker-a', 'email', 'campaign-1', '2026-08-06 10:00:00', 'GDPR', 'tpl-eu', 'hash-a');
INSERT INTO removal_requests (id, broker_id, channel, campaign_id, created_at, jurisdiction, template_id, identity_snapshot_hash) VALUES (2, 'broker-b', 'web_form', 'campaign-1', '2026-08-06 11:00:00', 'CCPA', 'tpl-us', 'hash-b');
INSERT INTO removal_requests (id, broker_id, channel, campaign_id, created_at, jurisdiction, template_id, identity_snapshot_hash) VALUES (3, 'broker-a', 'email', 'campaign-2', '2026-08-06 12:00:00', 'GDPR', 'tpl-eu', 'hash-c');
INSERT INTO request_state (request_id, current_status, last_event_id, last_event_at, sent_at, acknowledged_at, resolved_at, deadline_at, next_action_at, reminders_sent, escalation_level) VALUES (1, 'SENT', 3, '2026-08-07 09:00:00', '2026-08-06 12:00:05', NULL, NULL, '2026-08-20 12:00:05', '2026-08-14 09:00:00', 0, 0);
INSERT INTO request_state (request_id, current_status, last_event_id, last_event_at, sent_at, acknowledged_at, resolved_at, deadline_at, next_action_at, reminders_sent, escalation_level) VALUES (2, 'HUMAN_ACTION_REQUIRED', 4, '2026-08-08 08:00:00', '2026-08-07 08:00:00', NULL, NULL, NULL, NULL, 1, 1);
INSERT INTO request_events (id, request_id, occurred_at, recorded_at, event_type, payload_json, source) VALUES (1, 1, '2026-08-06 12:00:00', '2026-08-06 12:00:01', 'PLANNED', '{}', 'system');
INSERT INTO request_events (id, request_id, occurred_at, recorded_at, event_type, payload_json, source) VALUES (2, 1, '2026-08-07 09:00:00', '2026-08-07 09:00:02', 'SENT', '{"account":"daniel@example.com"}', 'user');
INSERT INTO request_events (id, request_id, occurred_at, recorded_at, event_type, payload_json, source) VALUES (3, 1, '2026-08-07 09:00:00', '2026-08-07 09:00:03', 'ACK', '{}', 'inbox');
INSERT INTO request_events (id, request_id, occurred_at, recorded_at, event_type, payload_json, source) VALUES (4, 2, '2026-08-08 08:00:00', '2026-08-08 08:00:04', 'BOUNCE', '{"reason":"mailbox_full"}', 'inbox');
INSERT INTO request_events (id, request_id, occurred_at, recorded_at, event_type, payload_json, source) VALUES (5, 3, '2026-08-09 07:00:00', '2026-08-09 07:00:05', 'NEWER_EVENT_TYPE', '{"detail":"forward compat"}', 'scheduler');
