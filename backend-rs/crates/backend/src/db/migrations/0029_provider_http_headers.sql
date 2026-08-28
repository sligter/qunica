ALTER TABLE llm_providers ADD COLUMN headers_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE llm_providers ADD COLUMN user_agent TEXT;
