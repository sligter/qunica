ALTER TABLE threads
ADD COLUMN title TEXT CHECK (title IS NULL OR length(trim(title)) BETWEEN 1 AND 80);

-- Context resets used `cleared` before task threads were user-visible. Keep
-- those histories reachable, while truly cleared message threads stay hidden.
UPDATE threads
SET status = 'archived'
WHERE agent_id IS NULL
  AND status = 'cleared'
  AND EXISTS (
    SELECT 1
    FROM groups
    WHERE groups.id = threads.group_id
      AND groups.conversation_kind = 'group'
  )
  AND EXISTS (
    SELECT 1
    FROM messages
    WHERE messages.thread_id = threads.id
      AND messages.status IN ('visible', 'interrupted')
  );

-- ponytail: legacy titles use the first user message; users can name new tasks explicitly.
UPDATE threads
SET title = COALESCE(
  NULLIF(substr(trim((
    SELECT content
    FROM messages
    WHERE messages.thread_id = threads.id
      AND messages.sender_type = 'user'
      AND messages.status IN ('visible', 'interrupted')
    ORDER BY messages.seq
    LIMIT 1
  )), 1, 80), ''),
  'Task'
)
WHERE agent_id IS NULL
  AND title IS NULL
  AND EXISTS (
    SELECT 1
    FROM groups
    WHERE groups.id = threads.group_id
      AND groups.conversation_kind = 'group'
  );

CREATE INDEX ix_threads_group_tasks
ON threads(group_id, agent_id, status, updated_at);
