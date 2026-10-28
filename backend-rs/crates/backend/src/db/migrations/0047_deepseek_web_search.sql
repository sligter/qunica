-- Keep provider credentials independent so switching preserves both configurations.
ALTER TABLE system_settings ADD COLUMN deepseek_api_key TEXT;
ALTER TABLE system_settings ADD COLUMN deepseek_search_url TEXT NOT NULL DEFAULT 'https://api.deepseek.com/v1/tools/search';
ALTER TABLE system_settings ADD COLUMN deepseek_fetch_url TEXT NOT NULL DEFAULT 'https://api.deepseek.com/v1/tools/fetch';
ALTER TABLE system_settings ADD COLUMN deepseek_max_results INTEGER NOT NULL DEFAULT 5;
ALTER TABLE system_settings ADD COLUMN deepseek_need_content INTEGER NOT NULL DEFAULT 0;
