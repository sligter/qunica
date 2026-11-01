-- Note: rows written before serde_json's exact float parser was pinned hashed
-- legacy-parsed floats. Version 1 marks them so webhook dedup can match their
-- possible legacy digests; new rows store version 2 and match exactly.
ALTER TABLE automation_occurrences ADD COLUMN digest_version INTEGER NOT NULL DEFAULT 1;
