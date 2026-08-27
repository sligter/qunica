CREATE INDEX IF NOT EXISTS ix_stream_events_stream_seq
ON stream_events(stream_id, seq);

CREATE INDEX IF NOT EXISTS ix_stream_events_thread_id
ON stream_events(thread_id);
