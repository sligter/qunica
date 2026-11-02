-- Watchdog: a scheduled automation whose runs inspect a router group's tasks
-- with read-only tools instead of carrying out a prompt. Its runs share one
-- thread per automation, recognised by `watchdog_automation_id`.
ALTER TABLE automations ADD COLUMN kind TEXT NOT NULL DEFAULT 'prompt'
  CHECK (kind IN ('prompt','watchdog'));

ALTER TABLE threads ADD COLUMN watchdog_automation_id TEXT
  REFERENCES automations(id) ON DELETE SET NULL;

CREATE INDEX ix_threads_watchdog ON threads(watchdog_automation_id)
WHERE watchdog_automation_id IS NOT NULL;
