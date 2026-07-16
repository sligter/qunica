ALTER TABLE groups ADD COLUMN scheduler_enabled INTEGER NOT NULL DEFAULT 0 CHECK (scheduler_enabled IN (0, 1));
ALTER TABLE groups ADD COLUMN agent_mention_policy TEXT NOT NULL DEFAULT 'display_only' CHECK (agent_mention_policy IN ('display_only', 'bounded_schedule'));
ALTER TABLE groups ADD COLUMN max_agent_steps INTEGER CHECK (max_agent_steps IS NULL OR max_agent_steps >= 1);
ALTER TABLE groups ADD COLUMN max_steps_per_agent INTEGER NOT NULL DEFAULT 3 CHECK (max_steps_per_agent >= 1);
ALTER TABLE groups ADD COLUMN max_scheduler_hops INTEGER NOT NULL DEFAULT 5 CHECK (max_scheduler_hops >= 0);
ALTER TABLE groups ADD COLUMN max_moderator_calls INTEGER NOT NULL DEFAULT 4 CHECK (max_moderator_calls >= 0);
ALTER TABLE groups ADD COLUMN max_consecutive_failures INTEGER NOT NULL DEFAULT 3 CHECK (max_consecutive_failures >= 1);
ALTER TABLE groups ADD COLUMN max_total_failures INTEGER NOT NULL DEFAULT 6 CHECK (max_total_failures >= 1);
ALTER TABLE groups ADD COLUMN max_total_tokens INTEGER NOT NULL DEFAULT 120000 CHECK (max_total_tokens >= 1);
ALTER TABLE groups ADD COLUMN turn_timeout_seconds INTEGER NOT NULL DEFAULT 300 CHECK (turn_timeout_seconds BETWEEN 1 AND 3600);
ALTER TABLE groups ADD COLUMN moderator_enabled INTEGER NOT NULL DEFAULT 0 CHECK (moderator_enabled IN (0, 1));
ALTER TABLE groups ADD COLUMN moderator_provider_id TEXT REFERENCES llm_providers(id) ON DELETE SET NULL;
ALTER TABLE groups ADD COLUMN moderator_model TEXT;

CREATE TABLE group_turns (
  id TEXT PRIMARY KEY NOT NULL,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  trigger_message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
  status TEXT NOT NULL,
  scheduler_strategy TEXT NOT NULL,
  config_snapshot_json TEXT NOT NULL,
  topology_snapshot_json TEXT NOT NULL,
  agent_steps INTEGER NOT NULL DEFAULT 0,
  moderator_calls INTEGER NOT NULL DEFAULT 0,
  consecutive_failures INTEGER NOT NULL DEFAULT 0,
  total_failures INTEGER NOT NULL DEFAULT 0,
  total_tokens INTEGER NOT NULL DEFAULT 0,
  termination_reason TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE agent_dispatches (
  id TEXT PRIMARY KEY NOT NULL,
  turn_id TEXT NOT NULL REFERENCES group_turns(id) ON DELETE CASCADE,
  parent_dispatch_id TEXT REFERENCES agent_dispatches(id) ON DELETE SET NULL,
  source_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
  target_agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  selection_reason TEXT NOT NULL,
  action_kind TEXT NOT NULL,
  hop INTEGER NOT NULL,
  status TEXT NOT NULL,
  input_message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
  output_message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
  artifact_json TEXT,
  total_tokens INTEGER NOT NULL DEFAULT 0,
  failure_code TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL
);

ALTER TABLE messages ADD COLUMN turn_id TEXT REFERENCES group_turns(id) ON DELETE SET NULL;
ALTER TABLE messages ADD COLUMN dispatch_id TEXT REFERENCES agent_dispatches(id) ON DELETE SET NULL;
ALTER TABLE messages ADD COLUMN reply_to_message_id TEXT REFERENCES messages(id) ON DELETE SET NULL;

CREATE INDEX ix_group_turns_thread_created ON group_turns(thread_id, created_at);
CREATE UNIQUE INDEX ux_group_turns_one_active_thread ON group_turns(thread_id) WHERE status IN ('pending', 'running', 'waiting_for_user');
CREATE INDEX ix_agent_dispatches_turn_created ON agent_dispatches(turn_id, created_at);
