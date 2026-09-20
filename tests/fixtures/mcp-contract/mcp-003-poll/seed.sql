-- Frozen input rows for the MCP `poll_inbox` contract (rust-tests/parity/oracle/mcp-poll-inbox).
-- One statement per line; both the Go oracle and the Rust replay test execute this file
-- verbatim, so the rows both sides read are identical.
--
-- Why frozen rows: the Go write path stamps created_at/recorded_at with time.Now() and offers
-- no injection point, so a store the product wrote could never be pinned. These rows are
-- fixture input in the same sense as the registry YAML the `validate` cases read.
--
-- Two requests are active and matchable:
--   * request 1 (broker-a) carries a SENT event whose payload names the Message-ID
--     <sent-1@example.com>. The INBOX reply points at it with In-Reply-To, so it must match
--     by *thread*.
--   * request 2 (broker-b) has no recorded Message-ID, so only the normalized subject
--     ("Data Deletion Request — broker-b") can match it.
-- Request 3 is terminal and must not be matchable at all.
INSERT INTO removal_requests (id, broker_id, channel, campaign_id, created_at, jurisdiction, template_id, identity_snapshot_hash) VALUES (1, 'broker-a', 'email', 'campaign-1', '2026-08-06 10:00:00', 'GDPR', 'tpl-eu', 'hash-a');
INSERT INTO removal_requests (id, broker_id, channel, campaign_id, created_at, jurisdiction, template_id, identity_snapshot_hash) VALUES (2, 'broker-b', 'email', 'campaign-1', '2026-08-06 11:00:00', 'GDPR', 'tpl-eu', 'hash-b');
INSERT INTO removal_requests (id, broker_id, channel, campaign_id, created_at, jurisdiction, template_id, identity_snapshot_hash) VALUES (3, 'broker-c', 'email', 'campaign-1', '2026-08-06 12:00:00', 'GDPR', 'tpl-eu', 'hash-c');
INSERT INTO request_state (request_id, current_status, last_event_id, last_event_at, sent_at, acknowledged_at, resolved_at, deadline_at, next_action_at, reminders_sent, escalation_level) VALUES (1, 'SENT', 2, '2026-08-07 09:00:00', '2026-08-07 09:00:05', NULL, NULL, '2026-08-20 12:00:05', '2026-08-14 09:00:00', 0, 0);
INSERT INTO request_state (request_id, current_status, last_event_id, last_event_at, sent_at, acknowledged_at, resolved_at, deadline_at, next_action_at, reminders_sent, escalation_level) VALUES (2, 'SENT', 3, '2026-08-07 10:00:00', '2026-08-07 10:00:05', NULL, NULL, NULL, NULL, 0, 0);
INSERT INTO request_state (request_id, current_status, last_event_id, last_event_at, sent_at, acknowledged_at, resolved_at, deadline_at, next_action_at, reminders_sent, escalation_level) VALUES (3, 'CONFIRMED', 4, '2026-08-07 11:00:00', '2026-08-07 11:00:05', '2026-08-07 12:00:00', '2026-08-07 12:00:00', NULL, NULL, 0, 0);
INSERT INTO request_events (id, request_id, occurred_at, recorded_at, event_type, payload_json, source) VALUES (1, 1, '2026-08-07 08:59:00', '2026-08-07 08:59:01', 'PLANNED', '{}', 'system');
INSERT INTO request_events (id, request_id, occurred_at, recorded_at, event_type, payload_json, source) VALUES (2, 1, '2026-08-07 09:00:00', '2026-08-07 09:00:02', 'SENT', '{"message_id":"<sent-1@example.com>"}', 'user');
INSERT INTO request_events (id, request_id, occurred_at, recorded_at, event_type, payload_json, source) VALUES (3, 2, '2026-08-07 10:00:00', '2026-08-07 10:00:02', 'SENT', '{"account":"daniel@example.com"}', 'user');
INSERT INTO request_events (id, request_id, occurred_at, recorded_at, event_type, payload_json, source) VALUES (4, 3, '2026-08-07 12:00:00', '2026-08-07 12:00:02', 'CONFIRMED', '{}', 'inbox');