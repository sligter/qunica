ALTER TABLE token_usage_records
ADD COLUMN cached_input_tokens INTEGER
CHECK (cached_input_tokens IS NULL OR cached_input_tokens >= 0);
