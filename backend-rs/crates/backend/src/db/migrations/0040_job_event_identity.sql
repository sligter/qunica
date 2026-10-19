-- A fast job can finish before another device's next snapshot. Its event must
-- name the run so discovery and a terminal notification do not depend on polling.
DROP TRIGGER account_execution_jobs_insert;
DROP TRIGGER account_execution_jobs_update;
DROP TRIGGER account_execution_jobs_delete;
CREATE TRIGGER account_execution_jobs_insert AFTER INSERT ON execution_jobs
WHEN EXISTS(SELECT 1 FROM users WHERE id=NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,group_id,thread_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'job.changed',NEW.id,NEW.group_id,NEW.thread_id,
        json_object('operation','insert','status',NEW.status,'stream_id',NEW.stream_id,'turn_id',NEW.turn_id,'conversation_kind',(SELECT conversation_kind FROM groups WHERE id=NEW.group_id))
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;
CREATE TRIGGER account_execution_jobs_update AFTER UPDATE ON execution_jobs
WHEN EXISTS(SELECT 1 FROM users WHERE id=NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,group_id,thread_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'job.changed',NEW.id,NEW.group_id,NEW.thread_id,
        json_object('operation','update','status',NEW.status,'stream_id',NEW.stream_id,'turn_id',NEW.turn_id,'conversation_kind',(SELECT conversation_kind FROM groups WHERE id=NEW.group_id))
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;
CREATE TRIGGER account_execution_jobs_delete AFTER DELETE ON execution_jobs
WHEN EXISTS(SELECT 1 FROM users WHERE id=OLD.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(OLD.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,group_id,thread_id,payload_json)
        SELECT OLD.owner_id,high_watermark,lower(hex(randomblob(16))),'job.changed',OLD.id,OLD.group_id,OLD.thread_id,
        json_object('operation','delete','status',OLD.status,'stream_id',OLD.stream_id,'turn_id',OLD.turn_id,'conversation_kind',(SELECT conversation_kind FROM groups WHERE id=OLD.group_id))
        FROM account_event_counters WHERE owner_id=OLD.owner_id;
END;
