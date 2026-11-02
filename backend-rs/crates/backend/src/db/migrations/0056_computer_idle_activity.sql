-- Activity is metadata on the existing resources/intents, not a second runtime
-- state store. NULL generation grants legacy computers a full observation grace.
ALTER TABLE computers ADD COLUMN idle_observed_generation TEXT;
ALTER TABLE computers ADD COLUMN last_activity_at_ms INTEGER;
ALTER TABLE computer_operations ADD COLUMN last_activity_at_ms INTEGER;
CREATE INDEX ix_computer_operations_computer ON computer_operations(computer_id,status);
CREATE TRIGGER computer_operation_activity_insert AFTER INSERT ON computer_operations BEGIN
    UPDATE computer_operations SET last_activity_at_ms=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER) WHERE id=NEW.id;
END;
CREATE TRIGGER computer_operation_activity_update AFTER UPDATE OF status ON computer_operations BEGIN
    UPDATE computer_operations SET last_activity_at_ms=CAST((julianday('now')-2440587.5)*86400000 AS INTEGER) WHERE id=NEW.id;
END;
-- Lease renewals and observation metadata must not flood account event history.
DROP TRIGGER account_computers_update;
CREATE TRIGGER account_computers_update AFTER UPDATE OF revision ON computers BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'computer.changed',NEW.id,
            json_object('operation','update','revision',NEW.revision)
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;
