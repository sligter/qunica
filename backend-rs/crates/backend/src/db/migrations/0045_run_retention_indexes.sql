-- Note: retention searches old events and completion markers without reading
-- every payload in a stream for every candidate while holding the writer.
CREATE INDEX ix_stream_events_retention ON stream_events(created_at, id);
CREATE INDEX ix_stream_events_done ON stream_events(stream_id) WHERE kind = 'done';
