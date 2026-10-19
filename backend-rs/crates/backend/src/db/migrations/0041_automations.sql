-- Note: group ownership is separate from job status. Cancellation cannot hand
-- the group to another worker until the old worker has actually finished.
ALTER TABLE execution_jobs ADD COLUMN occupies_group INTEGER NOT NULL DEFAULT 0 CHECK(occupies_group IN (0,1));
ALTER TABLE execution_jobs ADD COLUMN worker_active INTEGER NOT NULL DEFAULT 0 CHECK(worker_active IN (0,1));
ALTER TABLE execution_jobs ADD COLUMN automation_id TEXT;
ALTER TABLE execution_jobs ADD COLUMN occurrence_id TEXT;
ALTER TABLE execution_jobs ADD COLUMN automation_snapshot_json TEXT;
-- Existing M1 databases can have several waiting jobs in one group. Preserve
-- every checkpoint; recovery and claiming serialize them without dropping one.
UPDATE execution_jobs SET worker_active=1 WHERE status='running';
UPDATE execution_jobs SET occupies_group=1 WHERE status IN ('running','waiting_for_user')
 AND id=(SELECT j.id FROM execution_jobs j WHERE j.owner_id=execution_jobs.owner_id
 AND j.group_id=execution_jobs.group_id AND j.status IN ('running','waiting_for_user')
 ORDER BY j.created_at,j.id LIMIT 1);
CREATE UNIQUE INDEX ux_execution_jobs_group_owner ON execution_jobs(owner_id,group_id) WHERE occupies_group=1;
CREATE INDEX ix_execution_jobs_automation ON execution_jobs(automation_id,status);

CREATE TABLE automations (
 id TEXT PRIMARY KEY,
 owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 group_id TEXT NOT NULL,
 name TEXT NOT NULL,
 prompt TEXT NOT NULL,
 trigger_json TEXT NOT NULL,
 timezone TEXT NOT NULL,
 missed_policy TEXT NOT NULL CHECK(missed_policy IN ('coalesce_latest','skip')),
 budget_json TEXT NOT NULL,
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
 version INTEGER NOT NULL DEFAULT 1,
 next_run_at INTEGER,
 hook_id TEXT NOT NULL UNIQUE,
 hook_secret TEXT,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE INDEX ix_automations_due ON automations(enabled,next_run_at);
CREATE TABLE automation_occurrences (
 id TEXT PRIMARY KEY,
 automation_id TEXT NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
 automation_version INTEGER NOT NULL,
 source TEXT NOT NULL CHECK(source IN ('timer','webhook','skipped')),
 event_key TEXT NOT NULL,
 payload_hash TEXT NOT NULL,
 scheduled_at INTEGER,
 job_id TEXT,
 response_json TEXT NOT NULL,
 created_at TEXT NOT NULL,
 UNIQUE(automation_id,source,event_key)
);
CREATE INDEX ix_automation_occurrences_history ON automation_occurrences(automation_id,created_at DESC);

CREATE TRIGGER account_automations_insert AFTER INSERT ON automations
BEGIN
 INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
 ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
 INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,group_id,payload_json)
 SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'automation.changed',NEW.id,NEW.group_id,
 json_object('operation','insert','version',NEW.version,'enabled',NEW.enabled)
 FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;
CREATE TRIGGER account_automations_update AFTER UPDATE ON automations
BEGIN
 INSERT INTO account_event_counters(owner_id,high_watermark) VALUES(NEW.owner_id,1)
 ON CONFLICT(owner_id) DO UPDATE SET high_watermark=high_watermark+1;
 INSERT INTO account_events(owner_id,seq,event_id,kind,resource_id,group_id,payload_json)
 SELECT NEW.owner_id,high_watermark,lower(hex(randomblob(16))),'automation.changed',NEW.id,NEW.group_id,
 json_object('operation','update','version',NEW.version,'enabled',NEW.enabled)
 FROM account_event_counters WHERE owner_id=NEW.owner_id;
END;
