ALTER TABLE groups ADD COLUMN scheduler_mode TEXT NOT NULL DEFAULT 'bounded'
  CHECK (scheduler_mode IN ('bounded', 'automatic'));
