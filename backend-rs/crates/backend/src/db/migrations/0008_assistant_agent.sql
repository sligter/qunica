-- The built-in Assistant is an ordinary agent row so the whole direct-chat
-- harness (SSE streaming, resume, interruption, turn traces) applies to it
-- unchanged. `is_system` is the only thing that distinguishes it, and it
-- doubles as the write guard on the generic agent routes.
ALTER TABLE agents ADD COLUMN is_system INTEGER NOT NULL DEFAULT 0
  CHECK (is_system IN (0, 1));

-- Every configuration change the Assistant proposes lands here first. Nothing
-- is applied until the owner approves the row through the approval endpoint,
-- so the ledger is both the approval queue and the audit trail.
CREATE TABLE app_actions (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  conversation_id TEXT REFERENCES groups(id) ON DELETE SET NULL,
  tool_call_id TEXT,
  target_kind TEXT NOT NULL,
  action TEXT NOT NULL,
  target_id TEXT,
  summary TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending', 'approved', 'rejected', 'applied', 'failed', 'expired')),
  result_json TEXT,
  created_at TEXT NOT NULL,
  resolved_at TEXT
);

CREATE INDEX ix_app_actions_owner_status
ON app_actions(owner_id, status, created_at DESC);
