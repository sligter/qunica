CREATE TABLE group_templates (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  config_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(owner_id, name)
);

CREATE INDEX ix_group_templates_owner_updated
ON group_templates(owner_id, updated_at DESC);
