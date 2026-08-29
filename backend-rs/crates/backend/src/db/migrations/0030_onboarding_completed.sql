ALTER TABLE system_settings
ADD COLUMN onboarding_completed INTEGER NOT NULL DEFAULT 0
  CHECK (onboarding_completed IN (0, 1));

-- Existing accounts have already entered Qunica without this guide. Only rows
-- created after this migration should be treated as a first run.
UPDATE system_settings SET onboarding_completed = 1;

-- Older accounts may never have opened System Settings, whose row is created
-- lazily. Seed those accounts now so their next login is not mistaken for a
-- brand-new installation.
INSERT INTO system_settings (id, owner_id, onboarding_completed, created_at, updated_at)
SELECT lower(hex(randomblob(16))), users.id, 1, users.created_at, users.updated_at
FROM users
WHERE NOT EXISTS (
  SELECT 1 FROM system_settings WHERE system_settings.owner_id = users.id
);
