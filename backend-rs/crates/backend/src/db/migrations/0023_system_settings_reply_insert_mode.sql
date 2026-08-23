-- What happens to a message typed while a reply is still streaming.
--
-- 'instant' is the behaviour that shipped: the message is sent immediately and
-- lands inside the running turn, steering it. 'queue' holds the message until
-- the current reply finishes and then sends it on its own, so a follow-up that
-- was meant as the *next* question does not rewrite the answer in flight.
ALTER TABLE system_settings
ADD COLUMN reply_insert_mode TEXT NOT NULL DEFAULT 'instant';
