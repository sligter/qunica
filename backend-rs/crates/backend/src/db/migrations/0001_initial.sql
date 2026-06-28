CREATE TABLE users (
  id TEXT PRIMARY KEY NOT NULL,
  email TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  name TEXT NOT NULL,
  avatar_url TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE workspaces (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  backend_type TEXT NOT NULL DEFAULT 'local',
  local_path TEXT,
  config_json TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE agents (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  workspace_id TEXT REFERENCES workspaces(id) ON DELETE SET NULL,
  name TEXT NOT NULL,
  description TEXT,
  system_prompt TEXT NOT NULL,
  runtime_kind TEXT NOT NULL DEFAULT 'llm_chat',
  provider_id TEXT,
  model_config_json TEXT,
  tool_config_json TEXT,
  external_runtime_json TEXT,
  skill_ids_json TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE groups (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  workspace_id TEXT REFERENCES workspaces(id) ON DELETE SET NULL,
  name TEXT NOT NULL,
  description TEXT,
  announcement TEXT,
  free_speech INTEGER NOT NULL DEFAULT 0,
  proactive_mode INTEGER NOT NULL DEFAULT 0,
  proactive_max_rounds INTEGER NOT NULL DEFAULT 1,
  proactive_reply_multiplier INTEGER NOT NULL DEFAULT 1,
  allow_agent_free_mention INTEGER NOT NULL DEFAULT 1,
  agent_free_mention_max_dispatches INTEGER NOT NULL DEFAULT 8,
  communication_mode TEXT NOT NULL DEFAULT 'mesh',
  muted_agent_ids_json TEXT,
  admin_agent_ids_json TEXT,
  muted_member_ids_json TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE group_members (
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'member',
  status TEXT NOT NULL DEFAULT 'active',
  joined_at TEXT NOT NULL,
  PRIMARY KEY (group_id, user_id)
);

CREATE TABLE group_agents (
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  display_name TEXT,
  role TEXT,
  topology_role TEXT,
  speaking_order INTEGER,
  response_mode TEXT NOT NULL DEFAULT 'mentioned_only',
  context_scope_json TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  joined_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (group_id, agent_id)
);

CREATE TABLE group_notes (
  id TEXT PRIMARY KEY NOT NULL,
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  author_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  content TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE threads (
  id TEXT PRIMARY KEY NOT NULL,
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
  status TEXT NOT NULL DEFAULT 'active',
  next_seq INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE messages (
  id TEXT PRIMARY KEY NOT NULL,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  sender_type TEXT NOT NULL,
  sender_id TEXT,
  message_type TEXT NOT NULL,
  content TEXT,
  content_json TEXT,
  status TEXT NOT NULL DEFAULT 'visible',
  created_at TEXT NOT NULL,
  UNIQUE(thread_id, seq)
);

CREATE TABLE stream_events (
  id TEXT PRIMARY KEY NOT NULL,
  stream_id TEXT NOT NULL,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  event_id TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE llm_providers (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  base_url TEXT,
  api_key TEXT NOT NULL,
  default_model TEXT NOT NULL,
  context_window_tokens INTEGER,
  context_output_reserve_ratio REAL,
  description TEXT,
  reasoning_passback INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE skills (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  body_markdown TEXT NOT NULL,
  metadata_json TEXT,
  source TEXT NOT NULL DEFAULT 'manual',
  files_json TEXT,
  storage_path TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE system_settings (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
  group_workspace_root TEXT,
  web_search_provider TEXT NOT NULL DEFAULT 'tavily',
  tavily_api_key TEXT,
  tavily_search_url TEXT NOT NULL DEFAULT 'https://api.tavily.com/search',
  tavily_max_results INTEGER NOT NULL DEFAULT 5,
  tavily_search_depth TEXT NOT NULL DEFAULT 'basic',
  tavily_include_answer INTEGER NOT NULL DEFAULT 1,
  tavily_include_raw_content INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE external_agent_runs (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  group_id TEXT REFERENCES groups(id) ON DELETE SET NULL,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL,
  adapter TEXT NOT NULL,
  cwd TEXT NOT NULL,
  status TEXT NOT NULL,
  argv_json TEXT NOT NULL,
  exit_code INTEGER,
  stdout_tail TEXT,
  stderr_tail TEXT,
  error_message TEXT,
  started_at TEXT NOT NULL,
  ended_at TEXT
);
