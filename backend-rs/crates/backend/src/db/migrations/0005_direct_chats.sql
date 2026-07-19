ALTER TABLE groups
ADD COLUMN conversation_kind TEXT NOT NULL DEFAULT 'group';

ALTER TABLE groups
ADD COLUMN direct_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL;

ALTER TABLE groups
ADD COLUMN title_source TEXT NOT NULL DEFAULT 'manual';

CREATE INDEX ix_groups_owner_kind_activity
ON groups(owner_id, conversation_kind, status, updated_at DESC);

CREATE INDEX ix_groups_direct_agent
ON groups(direct_agent_id)
WHERE conversation_kind = 'direct';
