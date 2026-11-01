-- Note: execution checkpoints update rows beyond a saved summary's cursor.
-- Those rows are loaded as incremental history and do not change the summary.
DROP TRIGGER invalidate_context_checkpoints_on_message_update;
DROP TRIGGER invalidate_context_checkpoints_on_message_delete;

CREATE TRIGGER invalidate_context_checkpoints_on_message_update
AFTER UPDATE OF sender_type, sender_id, content, content_json, status, context_admitted, thread_id, seq ON messages
BEGIN
    DELETE FROM context_checkpoints
    WHERE (thread_id = OLD.thread_id AND through_seq >= OLD.seq)
       OR (thread_id = NEW.thread_id AND through_seq >= NEW.seq);
END;

CREATE TRIGGER invalidate_context_checkpoints_on_message_delete
AFTER DELETE ON messages
BEGIN
    DELETE FROM context_checkpoints
    WHERE thread_id = OLD.thread_id AND through_seq >= OLD.seq;
END;
