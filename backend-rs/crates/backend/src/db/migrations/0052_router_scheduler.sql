-- Router scheduling: one member answers the main thread and dispatches work to
-- task threads pinned to a worker. `groups.scheduler_mode` keeps its original
-- CHECK constraint; a group is in router mode when `router_agent_id` is set.
ALTER TABLE groups ADD COLUMN router_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL;
ALTER TABLE groups ADD COLUMN max_router_tasks INTEGER NOT NULL DEFAULT 4
  CHECK (max_router_tasks BETWEEN 1 AND 16);

ALTER TABLE threads ADD COLUMN assigned_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL;
ALTER TABLE threads ADD COLUMN origin_thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL;
ALTER TABLE threads ADD COLUMN origin_turn_id TEXT;

CREATE INDEX ix_threads_origin ON threads(origin_thread_id)
WHERE origin_thread_id IS NOT NULL;
