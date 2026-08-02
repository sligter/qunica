ALTER TABLE system_settings
ADD COLUMN media_base_url TEXT NOT NULL DEFAULT 'https://api.openai.com';

ALTER TABLE system_settings
ADD COLUMN media_api_key TEXT;

ALTER TABLE system_settings
ADD COLUMN image_generation_model TEXT;

ALTER TABLE system_settings
ADD COLUMN image_generation_endpoint TEXT NOT NULL DEFAULT '/v1/images/generations';

ALTER TABLE system_settings
ADD COLUMN video_generation_model TEXT;

ALTER TABLE system_settings
ADD COLUMN video_generation_endpoint TEXT NOT NULL DEFAULT '/v1/videos';

ALTER TABLE system_settings
ADD COLUMN video_status_endpoint TEXT NOT NULL DEFAULT '/v1/videos/{id}';

ALTER TABLE system_settings
ADD COLUMN video_content_endpoint TEXT NOT NULL DEFAULT '/v1/videos/{id}/content';
