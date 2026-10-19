-- Client visibility and model history admission have different lifetimes.
-- Queued input must stay out of older turns, including compacted checkpoints.
ALTER TABLE messages ADD COLUMN context_admitted INTEGER NOT NULL DEFAULT 1
    CHECK(context_admitted IN (0,1));
UPDATE messages SET context_admitted=0
WHERE id IN (SELECT message_id FROM execution_jobs
             WHERE command_name='message.send'
               AND (status='queued' OR (status='cancelled' AND generation=0)));
-- A checkpoint made by an older build may already contain queued input.
DELETE FROM context_checkpoints WHERE thread_id IN
    (SELECT thread_id FROM messages WHERE context_admitted=0);
