-- Note: expiry discovery starts with time across accounts; the old owner-first
-- index forced maintenance to visit recent events even when nothing expired.
DROP INDEX ix_account_events_retention;
CREATE INDEX ix_account_events_retention ON account_events(occurred_at, owner_id, seq);
CREATE INDEX ix_realtime_tickets_expiry ON realtime_tickets(expires_at, ticket_hash);

-- Note: snapshots only need these keys, not potentially large event payloads.
-- This also serves thread-only lookups, replacing the narrower index.
CREATE INDEX ix_stream_events_thread_stream_seq ON stream_events(thread_id, stream_id, seq);
DROP INDEX ix_stream_events_thread_id;
