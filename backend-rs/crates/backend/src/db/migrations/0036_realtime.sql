CREATE TABLE server_identity (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    server_id TEXT NOT NULL UNIQUE
);
INSERT INTO server_identity VALUES (1, lower(hex(randomblob(16))));

CREATE TABLE account_event_counters (
    owner_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    high_watermark INTEGER NOT NULL DEFAULT 0,
    retained_after INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE account_events (
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    group_id TEXT,
    thread_id TEXT,
    payload_json TEXT NOT NULL,
    occurred_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY(owner_id, seq)
);
CREATE INDEX ix_account_events_retention ON account_events(owner_id, occurred_at, seq);
CREATE TABLE resource_versions (
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    resource_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    PRIMARY KEY(owner_id,resource_id)
);
CREATE TRIGGER account_resource_version AFTER INSERT ON account_events BEGIN
    INSERT INTO resource_versions(owner_id,resource_id,version) VALUES(NEW.owner_id,NEW.resource_id,NEW.seq)
    ON CONFLICT(owner_id,resource_id) DO UPDATE SET version=NEW.seq;
END;
CREATE TABLE realtime_tickets (
    ticket_hash TEXT PRIMARY KEY,
    server_id TEXT NOT NULL,
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES auth_sessions(id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE TABLE command_receipts (
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    request_id TEXT NOT NULL,
    command_name TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    response_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(owner_id, request_id)
);
-- Legacy stream keys remain non-executable even when their original request
-- payload can no longer be reconstructed (for example after a history clear).
INSERT INTO command_receipts(owner_id,request_id,command_name,payload_hash,response_json,created_at)
SELECT g.owner_id,e.stream_id,'message.send','legacy_tombstone',
    json_object('request_id',e.stream_id,'status','ended','stream_id',e.stream_id,'thread_id',e.thread_id,'legacy',json('true')),
    MIN(e.created_at)
FROM stream_events e JOIN threads t ON t.id=e.thread_id JOIN groups g ON g.id=t.group_id
GROUP BY g.owner_id,e.stream_id;
-- Note: group-wide exclusivity belongs to M3. M1 retains the existing thread
-- constraint and uses these records for durable acceptance, not a new scheduler.
CREATE TABLE execution_jobs (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    request_id TEXT NOT NULL,
    command_name TEXT NOT NULL,
    group_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    stream_id TEXT NOT NULL UNIQUE,
    message_id TEXT,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued','running','waiting_for_user','succeeded','failed','cancelled','interrupted')),
    generation INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(owner_id, request_id)
);
CREATE INDEX ix_execution_jobs_queue ON execution_jobs(status, created_at);
CREATE TABLE pending_approvals (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    group_id TEXT NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    tool_call_id TEXT NOT NULL,
    action_hash TEXT NOT NULL,
    action_json TEXT NOT NULL,
    request_json TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','resolved','cancelled')),
    decision_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(thread_id, tool_call_id)
);


-- Note: invalidation events contain identifiers and public state only. Triggers
-- cover API, internal tools and runtime writes in the same business transaction.

CREATE TRIGGER account_groups_insert AFTER INSERT ON groups
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'group.changed', NEW.id, NEW.id, NULL, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_groups_update AFTER UPDATE ON groups
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'group.changed', NEW.id, NEW.id, NULL, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_groups_delete AFTER DELETE ON groups
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (OLD.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT OLD.owner_id, high_watermark, lower(hex(randomblob(16))), 'group.changed', OLD.id, OLD.id, NULL, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = OLD.owner_id;
END;

CREATE TRIGGER account_threads_insert AFTER INSERT ON threads
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'thread.changed', NEW.id, NEW.group_id, NEW.id, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_threads_update AFTER UPDATE ON threads
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'thread.changed', NEW.id, NEW.group_id, NEW.id, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_threads_delete AFTER DELETE ON threads
WHEN (SELECT owner_id FROM groups WHERE id = OLD.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = OLD.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = OLD.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = OLD.group_id), high_watermark, lower(hex(randomblob(16))), 'thread.changed', OLD.id, OLD.group_id, OLD.id, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = OLD.group_id);
END;

CREATE TRIGGER account_messages_insert AFTER INSERT ON messages
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'message.changed', NEW.id, NEW.group_id, NEW.thread_id, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_messages_update AFTER UPDATE ON messages
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'message.changed', NEW.id, NEW.group_id, NEW.thread_id, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_messages_delete AFTER DELETE ON messages
WHEN (SELECT owner_id FROM groups WHERE id = OLD.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = OLD.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = OLD.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = OLD.group_id), high_watermark, lower(hex(randomblob(16))), 'message.changed', OLD.id, OLD.group_id, OLD.thread_id, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = OLD.group_id);
END;

CREATE TRIGGER account_group_turns_insert AFTER INSERT ON group_turns
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'turn.changed', NEW.id, NEW.group_id, NEW.thread_id, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_turns_update AFTER UPDATE ON group_turns
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'turn.changed', NEW.id, NEW.group_id, NEW.thread_id, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_turns_delete AFTER DELETE ON group_turns
WHEN (SELECT owner_id FROM groups WHERE id = OLD.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = OLD.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = OLD.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = OLD.group_id), high_watermark, lower(hex(randomblob(16))), 'turn.changed', OLD.id, OLD.group_id, OLD.thread_id, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = OLD.group_id);
END;

CREATE TRIGGER account_group_agents_insert AFTER INSERT ON group_agents
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'member.changed', NEW.agent_id, NEW.group_id, NULL, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_agents_update AFTER UPDATE ON group_agents
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'member.changed', NEW.agent_id, NEW.group_id, NULL, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_agents_delete AFTER DELETE ON group_agents
WHEN (SELECT owner_id FROM groups WHERE id = OLD.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = OLD.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = OLD.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = OLD.group_id), high_watermark, lower(hex(randomblob(16))), 'member.changed', OLD.agent_id, OLD.group_id, NULL, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = OLD.group_id);
END;

CREATE TRIGGER account_group_members_insert AFTER INSERT ON group_members
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'member.changed', NEW.user_id, NEW.group_id, NULL, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_members_update AFTER UPDATE ON group_members
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'member.changed', NEW.user_id, NEW.group_id, NULL, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_members_delete AFTER DELETE ON group_members
WHEN (SELECT owner_id FROM groups WHERE id = OLD.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = OLD.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = OLD.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = OLD.group_id), high_watermark, lower(hex(randomblob(16))), 'member.changed', OLD.user_id, OLD.group_id, NULL, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = OLD.group_id);
END;

CREATE TRIGGER account_group_notes_insert AFTER INSERT ON group_notes
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'note.changed', NEW.id, NEW.group_id, NULL, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_notes_update AFTER UPDATE ON group_notes
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'note.changed', NEW.id, NEW.group_id, NULL, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_notes_delete AFTER DELETE ON group_notes
WHEN (SELECT owner_id FROM groups WHERE id = OLD.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = OLD.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = OLD.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = OLD.group_id), high_watermark, lower(hex(randomblob(16))), 'note.changed', OLD.id, OLD.group_id, NULL, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = OLD.group_id);
END;

CREATE TRIGGER account_group_files_insert AFTER INSERT ON group_files
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'file.changed', NEW.id, NEW.group_id, NULL, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_files_update AFTER UPDATE ON group_files
WHEN (SELECT owner_id FROM groups WHERE id = NEW.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = NEW.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = NEW.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = NEW.group_id), high_watermark, lower(hex(randomblob(16))), 'file.changed', NEW.id, NEW.group_id, NULL, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = NEW.group_id);
END;

CREATE TRIGGER account_group_files_delete AFTER DELETE ON group_files
WHEN (SELECT owner_id FROM groups WHERE id = OLD.group_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = (SELECT owner_id FROM groups WHERE id = OLD.group_id))
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES ((SELECT owner_id FROM groups WHERE id = OLD.group_id), 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT (SELECT owner_id FROM groups WHERE id = OLD.group_id), high_watermark, lower(hex(randomblob(16))), 'file.changed', OLD.id, OLD.group_id, NULL, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = (SELECT owner_id FROM groups WHERE id = OLD.group_id);
END;

CREATE TRIGGER account_workspaces_insert AFTER INSERT ON workspaces
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'workspace.changed', NEW.id, NULL, NULL, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_workspaces_update AFTER UPDATE ON workspaces
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'workspace.changed', NEW.id, NULL, NULL, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_workspaces_delete AFTER DELETE ON workspaces
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (OLD.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT OLD.owner_id, high_watermark, lower(hex(randomblob(16))), 'workspace.changed', OLD.id, NULL, NULL, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = OLD.owner_id;
END;

CREATE TRIGGER account_agents_insert AFTER INSERT ON agents
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'agent.changed', NEW.id, NULL, NULL, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_agents_update AFTER UPDATE ON agents
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'agent.changed', NEW.id, NULL, NULL, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_agents_delete AFTER DELETE ON agents
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (OLD.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT OLD.owner_id, high_watermark, lower(hex(randomblob(16))), 'agent.changed', OLD.id, NULL, NULL, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = OLD.owner_id;
END;

CREATE TRIGGER account_system_settings_insert AFTER INSERT ON system_settings
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'settings.changed', NEW.id, NULL, NULL, json_object('operation', 'insert', 'status', NULL)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_system_settings_update AFTER UPDATE ON system_settings
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'settings.changed', NEW.id, NULL, NULL, json_object('operation', 'update', 'status', NULL)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_system_settings_delete AFTER DELETE ON system_settings
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (OLD.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT OLD.owner_id, high_watermark, lower(hex(randomblob(16))), 'settings.changed', OLD.id, NULL, NULL, json_object('operation', 'delete', 'status', NULL)
        FROM account_event_counters WHERE owner_id = OLD.owner_id;
END;

CREATE TRIGGER account_llm_providers_insert AFTER INSERT ON llm_providers
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'provider.changed', NEW.id, NULL, NULL, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_llm_providers_update AFTER UPDATE ON llm_providers
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'provider.changed', NEW.id, NULL, NULL, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_llm_providers_delete AFTER DELETE ON llm_providers
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (OLD.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT OLD.owner_id, high_watermark, lower(hex(randomblob(16))), 'provider.changed', OLD.id, NULL, NULL, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = OLD.owner_id;
END;

CREATE TRIGGER account_auth_sessions_insert AFTER INSERT ON auth_sessions
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'session.changed', NEW.id, NULL, NULL, json_object('operation', 'insert', 'status', NULL)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_auth_sessions_update AFTER UPDATE ON auth_sessions
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'session.changed', NEW.id, NULL, NULL, json_object('operation', 'update', 'status', NULL)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_auth_sessions_delete AFTER DELETE ON auth_sessions
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (OLD.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT OLD.owner_id, high_watermark, lower(hex(randomblob(16))), 'session.changed', OLD.id, NULL, NULL, json_object('operation', 'delete', 'status', NULL)
        FROM account_event_counters WHERE owner_id = OLD.owner_id;
END;

CREATE TRIGGER account_pending_approvals_insert AFTER INSERT ON pending_approvals
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'approval.changed', NEW.id, NEW.group_id, NEW.thread_id, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_pending_approvals_update AFTER UPDATE ON pending_approvals
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'approval.changed', NEW.id, NEW.group_id, NEW.thread_id, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_pending_approvals_delete AFTER DELETE ON pending_approvals
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (OLD.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT OLD.owner_id, high_watermark, lower(hex(randomblob(16))), 'approval.changed', OLD.id, OLD.group_id, OLD.thread_id, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = OLD.owner_id;
END;

CREATE TRIGGER account_execution_jobs_insert AFTER INSERT ON execution_jobs
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'job.changed', NEW.id, NEW.group_id, NEW.thread_id, json_object('operation', 'insert', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_execution_jobs_update AFTER UPDATE ON execution_jobs
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (NEW.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT NEW.owner_id, high_watermark, lower(hex(randomblob(16))), 'job.changed', NEW.id, NEW.group_id, NEW.thread_id, json_object('operation', 'update', 'status', NEW.status)
        FROM account_event_counters WHERE owner_id = NEW.owner_id;
END;

CREATE TRIGGER account_execution_jobs_delete AFTER DELETE ON execution_jobs
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id = OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark) VALUES (OLD.owner_id, 1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT OLD.owner_id, high_watermark, lower(hex(randomblob(16))), 'job.changed', OLD.id, OLD.group_id, OLD.thread_id, json_object('operation', 'delete', 'status', OLD.status)
        FROM account_event_counters WHERE owner_id = OLD.owner_id;
END;

CREATE TRIGGER account_run_lifecycle AFTER INSERT ON stream_events
WHEN NEW.kind IN ('turn_started','turn_completed','turn_failed','turn_cancelled','waiting_for_user','approval_required','done')
BEGIN
    INSERT INTO account_event_counters(owner_id, high_watermark)
        SELECT g.owner_id, 1 FROM groups g JOIN threads t ON t.group_id = g.id WHERE t.id = NEW.thread_id
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark = high_watermark + 1;
    INSERT INTO account_events(owner_id, seq, event_id, kind, resource_id, group_id, thread_id, payload_json)
        SELECT g.owner_id, c.high_watermark, lower(hex(randomblob(16))), 'run.changed', NEW.stream_id,
            t.group_id, t.id, json_object('kind', NEW.kind, 'stream_id', NEW.stream_id)
        FROM threads t JOIN groups g ON g.id = t.group_id JOIN account_event_counters c ON c.owner_id = g.owner_id
        WHERE t.id = NEW.thread_id;
END;

CREATE TRIGGER account_skills_insert AFTER INSERT ON skills
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id=NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'skill.changed',NEW.id,json_object('operation','insert','status',NEW.status)
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;

CREATE TRIGGER account_skills_update AFTER UPDATE ON skills
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id=NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'skill.changed',NEW.id,json_object('operation','update','status',NEW.status)
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;

CREATE TRIGGER account_skills_delete AFTER DELETE ON skills
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id=OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(OLD.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT OLD.owner_id,high_watermark,lower(hex(randomblob(16))),'skill.changed',OLD.id,json_object('operation','delete','status',OLD.status)
        FROM account_event_counters WHERE owner_id=OLD.owner_id;
END;

CREATE TRIGGER account_mcp_servers_insert AFTER INSERT ON mcp_servers
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id=NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'mcp_server.changed',NEW.id,json_object('operation','insert','status',NEW.status)
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;

CREATE TRIGGER account_mcp_servers_update AFTER UPDATE ON mcp_servers
WHEN NEW.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id=NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'mcp_server.changed',NEW.id,json_object('operation','update','status',NEW.status)
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;

CREATE TRIGGER account_mcp_servers_delete AFTER DELETE ON mcp_servers
WHEN OLD.owner_id IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id=OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(OLD.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT OLD.owner_id,high_watermark,lower(hex(randomblob(16))),'mcp_server.changed',OLD.id,json_object('operation','delete','status',OLD.status)
        FROM account_event_counters WHERE owner_id=OLD.owner_id;
END;

CREATE TRIGGER account_agent_workspaces_insert AFTER INSERT ON agent_workspaces
WHEN (SELECT owner_id FROM agents WHERE id=NEW.agent_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id=(SELECT owner_id FROM agents WHERE id=NEW.agent_id))
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES((SELECT owner_id FROM agents WHERE id=NEW.agent_id),1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT (SELECT owner_id FROM agents WHERE id=NEW.agent_id),high_watermark,lower(hex(randomblob(16))),'agent.changed',NEW.agent_id,json_object('operation','insert','status',NULL)
        FROM account_event_counters WHERE owner_id=(SELECT owner_id FROM agents WHERE id=NEW.agent_id);
END;

CREATE TRIGGER account_agent_workspaces_update AFTER UPDATE ON agent_workspaces
WHEN (SELECT owner_id FROM agents WHERE id=NEW.agent_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id=(SELECT owner_id FROM agents WHERE id=NEW.agent_id))
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES((SELECT owner_id FROM agents WHERE id=NEW.agent_id),1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT (SELECT owner_id FROM agents WHERE id=NEW.agent_id),high_watermark,lower(hex(randomblob(16))),'agent.changed',NEW.agent_id,json_object('operation','update','status',NULL)
        FROM account_event_counters WHERE owner_id=(SELECT owner_id FROM agents WHERE id=NEW.agent_id);
END;

CREATE TRIGGER account_agent_workspaces_delete AFTER DELETE ON agent_workspaces
WHEN (SELECT owner_id FROM agents WHERE id=OLD.agent_id) IS NOT NULL AND EXISTS(SELECT 1 FROM users WHERE id=(SELECT owner_id FROM agents WHERE id=OLD.agent_id))
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES((SELECT owner_id FROM agents WHERE id=OLD.agent_id),1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT (SELECT owner_id FROM agents WHERE id=OLD.agent_id),high_watermark,lower(hex(randomblob(16))),'agent.changed',OLD.agent_id,json_object('operation','delete','status',NULL)
        FROM account_event_counters WHERE owner_id=(SELECT owner_id FROM agents WHERE id=OLD.agent_id);
END;

CREATE TRIGGER account_profile_update AFTER UPDATE ON users
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT NEW.id,high_watermark,lower(hex(randomblob(16))),'profile.changed',NEW.id,'{}'
        FROM account_event_counters WHERE owner_id=NEW.id;
END;
