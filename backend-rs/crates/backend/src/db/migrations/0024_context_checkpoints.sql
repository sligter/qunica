-- The compacted provider prompt for one agent in one conversation.
--
-- Visible messages remain untouched for the UI and audit history.  The
-- checkpoint is only the smaller prompt replayed to this agent on its next
-- turn; rows appended after `through_seq` are rendered and added normally.
CREATE TABLE context_checkpoints (
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    through_seq INTEGER NOT NULL,
    messages_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (thread_id, agent_id)
);

-- Appending messages does not invalidate a checkpoint: their sequence is
-- greater than `through_seq` and they are added on load. Rewriting or hiding a
-- covered message does, otherwise deleted or resumed content could survive in
-- the model-only prompt.
CREATE TRIGGER invalidate_context_checkpoints_on_message_update
AFTER UPDATE OF sender_type, sender_id, content, content_json, status ON messages
BEGIN
    DELETE FROM context_checkpoints WHERE thread_id = OLD.thread_id;
END;

CREATE TRIGGER invalidate_context_checkpoints_on_message_delete
AFTER DELETE ON messages
BEGIN
    DELETE FROM context_checkpoints WHERE thread_id = OLD.thread_id;
END;
