CREATE TABLE token_usage_records (
  id TEXT PRIMARY KEY NOT NULL,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  group_id TEXT,
  group_name TEXT NOT NULL,
  conversation_kind TEXT NOT NULL,
  thread_id TEXT,
  agent_id TEXT,
  agent_name TEXT NOT NULL,
  provider_id TEXT,
  provider_name TEXT NOT NULL,
  model TEXT NOT NULL,
  input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
  output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
  total_tokens INTEGER NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX ix_token_usage_owner_created
ON token_usage_records(owner_id, created_at);

-- Preserve the usage already present in agent messages. Older Anthropic rows
-- may only contain the final output count; future calls are recorded directly.
INSERT INTO token_usage_records (
  id, owner_id, group_id, group_name, conversation_kind, thread_id,
  agent_id, agent_name, provider_id, provider_name, model,
  input_tokens, output_tokens, total_tokens, created_at, updated_at
)
SELECT
  'legacy:' || messages.id,
  groups.owner_id,
  groups.id,
  groups.name,
  groups.conversation_kind,
  messages.thread_id,
  messages.sender_id,
  COALESCE(agents.name, 'Unknown agent'),
  agents.provider_id,
  COALESCE(llm_providers.name, CASE WHEN agents.runtime_kind = 'acp' THEN 'ACP' ELSE 'Unknown provider' END),
  COALESCE(
    json_extract(agents.model_config_json, '$.model'),
    llm_providers.default_model,
    CASE WHEN agents.runtime_kind = 'acp' THEN 'ACP runtime' ELSE 'Unknown model' END
  ),
  MAX(COALESCE(CAST(json_extract(messages.content_json, '$.context_usage.input_tokens') AS INTEGER), 0), 0),
  MAX(COALESCE(CAST(json_extract(messages.content_json, '$.context_usage.output_tokens') AS INTEGER), 0), 0),
  MAX(COALESCE(
    CAST(json_extract(messages.content_json, '$.context_usage.total_tokens') AS INTEGER),
    COALESCE(CAST(json_extract(messages.content_json, '$.context_usage.input_tokens') AS INTEGER), 0)
      + COALESCE(CAST(json_extract(messages.content_json, '$.context_usage.output_tokens') AS INTEGER), 0),
    0
  ), 0),
  messages.created_at,
  messages.created_at
FROM messages
JOIN groups ON groups.id = messages.group_id
LEFT JOIN agents ON agents.id = messages.sender_id
LEFT JOIN llm_providers ON llm_providers.id = agents.provider_id
WHERE messages.sender_type = 'agent'
  AND json_valid(messages.content_json)
  AND json_type(messages.content_json, '$.context_usage') = 'object';
