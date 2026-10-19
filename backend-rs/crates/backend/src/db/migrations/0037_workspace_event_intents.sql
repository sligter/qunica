-- Filesystem effects cannot join SQLite transactions. Persist their invalidation
-- intent before mutation and settle it on response, cancellation, or startup.
CREATE TABLE workspace_event_intents (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    group_id TEXT
);
CREATE TRIGGER account_workspace_effect AFTER DELETE ON workspace_event_intents
WHEN EXISTS(SELECT 1 FROM users WHERE id=OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(OLD.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,group_id,payload_json)
        SELECT OLD.owner_id,high_watermark,lower(hex(randomblob(16))),OLD.kind,OLD.resource_id,OLD.group_id,'{}'
        FROM account_event_counters WHERE owner_id=OLD.owner_id;
END;

-- A completed tool/CLI step invalidates files and Git without copying its output
-- or token stream into the account log. This also covers native tool mutations.
CREATE TRIGGER account_workspace_tool_effect AFTER INSERT ON stream_events
WHEN NEW.kind IN ('tool_call_result','acp_agent_run')
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark)
        SELECT g.owner_id,1 FROM groups g JOIN threads t ON t.group_id=g.id WHERE t.id=NEW.thread_id
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,group_id,thread_id,payload_json)
        SELECT g.owner_id,c.high_watermark,lower(hex(randomblob(16))),'file.changed',t.group_id,t.group_id,t.id,'{}'
        FROM threads t JOIN groups g ON g.id=t.group_id JOIN account_event_counters c ON c.owner_id=g.owner_id
        WHERE t.id=NEW.thread_id;
END;
