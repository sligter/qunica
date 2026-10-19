-- Tool progress is not a filesystem effect. A long ACP command reports progress
-- thousands of times as `tool_call_result` with status `started` (or no status),
-- and each one wrote a `file.changed` account event that every device turned
-- into a file-list refetch. Only finished steps invalidate files and Git now.
DROP TRIGGER account_workspace_tool_effect;

CREATE TRIGGER account_workspace_tool_effect AFTER INSERT ON stream_events
WHEN (NEW.kind = 'tool_call_result'
        AND COALESCE(json_extract(NEW.payload_json, '$.status'), 'started') <> 'started')
    OR (NEW.kind = 'acp_agent_run'
        AND COALESCE(json_extract(NEW.payload_json, '$.status'), '') <> 'running')
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark)
        SELECT g.owner_id,1 FROM groups g JOIN threads t ON t.group_id=g.id WHERE t.id=NEW.thread_id
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,group_id,thread_id,payload_json)
        SELECT g.owner_id,c.high_watermark,lower(hex(randomblob(16))),'file.changed',t.group_id,t.group_id,t.id,'{}'
        FROM threads t JOIN groups g ON g.id=t.group_id JOIN account_event_counters c ON c.owner_id=g.owner_id
        WHERE t.id=NEW.thread_id;
END;
