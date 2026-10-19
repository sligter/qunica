CREATE TABLE computers (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    binding_kind TEXT NOT NULL CHECK(binding_kind IN ('team','private')),
    binding_id TEXT NOT NULL,
    name TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);
CREATE INDEX ix_computers_owner ON computers(owner_id);
ALTER TABLE groups ADD COLUMN execution_environment_id TEXT REFERENCES computers(id);
ALTER TABLE agents ADD COLUMN execution_environment_id TEXT REFERENCES computers(id);
CREATE TABLE computer_run_bindings (
    turn_id TEXT NOT NULL REFERENCES group_turns(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL,
    computer_id TEXT NOT NULL REFERENCES computers(id),
    generation TEXT NOT NULL,
    PRIMARY KEY(turn_id,agent_id,computer_id,generation)
);
-- The HTTP waiter can disappear while the Supervisor still owns an exec.
CREATE TABLE computer_operations (
    id TEXT PRIMARY KEY,
    turn_id TEXT NOT NULL REFERENCES group_turns(id) ON DELETE CASCADE,
    computer_id TEXT NOT NULL REFERENCES computers(id),
    status TEXT NOT NULL CHECK(status IN ('running','completed','uncertain','acknowledged'))
);
CREATE INDEX ix_computer_operations_turn ON computer_operations(turn_id,status);
CREATE TRIGGER account_computers_insert AFTER INSERT ON computers BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'computer.changed',NEW.id,
            json_object('operation','insert','revision',NEW.revision)
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;
CREATE TRIGGER account_computers_update AFTER UPDATE ON computers BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'computer.changed',NEW.id,
            json_object('operation','update','revision',NEW.revision)
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;
