-- Note: Match Router report eligibility so clients suppress only results that have a summary.
DROP TRIGGER account_execution_jobs_update;
CREATE TRIGGER account_execution_jobs_update AFTER UPDATE ON execution_jobs
WHEN EXISTS(SELECT 1 FROM users WHERE id=NEW.owner_id)
BEGIN
    INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
        ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
    INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,group_id,thread_id,payload_json)
        SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'job.changed',NEW.id,NEW.group_id,NEW.thread_id,
        json_object('operation','update','status',NEW.status,'stream_id',NEW.stream_id,'turn_id',NEW.turn_id,'conversation_kind',(SELECT conversation_kind FROM groups WHERE id=NEW.group_id),
            'watchdog',json(CASE WHEN EXISTS(SELECT 1 FROM threads WHERE id=NEW.thread_id AND watchdog_automation_id IS NOT NULL) THEN 'true' ELSE 'false' END),
            'task_reported',json(CASE WHEN EXISTS(SELECT 1 FROM threads t JOIN groups g ON g.id=t.group_id JOIN threads o ON o.id=t.origin_thread_id WHERE t.id=NEW.thread_id AND t.origin_thread_id IS NOT NULL AND g.status='active' AND g.router_agent_id IS NOT NULL AND o.status IN ('active','running','paused')) THEN 'true' ELSE 'false' END))
        FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;
