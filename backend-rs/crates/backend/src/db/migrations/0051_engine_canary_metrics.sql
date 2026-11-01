-- Snapshots describe executions, never the agent's current configuration.
CREATE TABLE engine_rollouts (
    owner_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    rig_paused INTEGER NOT NULL DEFAULT 0 CHECK(rig_paused IN (0,1)),
    updated_at TEXT NOT NULL
);
CREATE TABLE engine_executions (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    engine TEXT NOT NULL CHECK(engine IN ('native','rig')),
    app_version TEXT NOT NULL,
    engine_version TEXT NOT NULL,
    provider_id TEXT,
    provider_kind TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK(status IN ('running','completed','failed','cancelled','waiting_for_user','interrupted','outcome_unknown','blocked')),
    coverage_complete INTEGER NOT NULL DEFAULT 1 CHECK(coverage_complete IN (0,1)),
    invocations INTEGER NOT NULL DEFAULT 1,
    active_duration_ms INTEGER NOT NULL DEFAULT 0,
    tool_calls INTEGER NOT NULL DEFAULT 0,
    settled_tool_calls INTEGER NOT NULL DEFAULT 0,
    tool_errors INTEGER NOT NULL DEFAULT 0,
    unsettled_tools INTEGER NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX engine_executions_owner_time ON engine_executions(owner_id,started_at);
ALTER TABLE token_usage_records ADD COLUMN execution_id TEXT;
ALTER TABLE token_usage_records ADD COLUMN engine TEXT;
CREATE INDEX token_usage_execution ON token_usage_records(execution_id);
