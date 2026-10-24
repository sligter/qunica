-- Note: tasks in one group have independent histories and ACP sessions. Reserve
-- the task thread until its worker and effects settle, so another task can run
-- without allowing two workers to act on the same thread.
DROP INDEX ux_execution_jobs_group_owner;
ALTER TABLE execution_jobs RENAME COLUMN occupies_group TO occupies_thread;
CREATE UNIQUE INDEX ux_execution_jobs_thread_owner
 ON execution_jobs(owner_id,thread_id) WHERE occupies_thread=1;
-- Include unreserved legacy checkpoints and cancelled workers in thread lookups.
CREATE INDEX ix_execution_jobs_thread ON execution_jobs(owner_id,thread_id);
CREATE INDEX ix_execution_jobs_automation_owner ON execution_jobs(owner_id,automation_id);
