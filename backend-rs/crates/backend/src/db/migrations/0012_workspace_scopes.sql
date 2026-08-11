CREATE TABLE agent_workspaces (
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL,
  PRIMARY KEY (agent_id, workspace_id)
);

CREATE INDEX ix_agent_workspaces_workspace_id
  ON agent_workspaces(workspace_id);

ALTER TABLE groups
  ADD COLUMN auto_share_workspace_with_new_agents INTEGER NOT NULL DEFAULT 1;
