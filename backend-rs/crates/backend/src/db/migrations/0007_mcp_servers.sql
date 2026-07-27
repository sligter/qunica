CREATE TABLE mcp_servers (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  transport TEXT NOT NULL,
  command TEXT,
  args_json TEXT,
  env_json TEXT,
  cwd TEXT,
  url TEXT,
  headers_json TEXT,
  timeout_seconds INTEGER NOT NULL DEFAULT 60,
  tool_filter_json TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX idx_mcp_servers_owner ON mcp_servers(owner_id);

CREATE UNIQUE INDEX idx_mcp_servers_owner_name ON mcp_servers(owner_id, name);
