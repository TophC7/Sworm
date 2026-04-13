-- Add archive support for sessions.
-- Archived sessions are hidden from the main session list but retained
-- in the database for history. Users can restore or permanently delete them.
ALTER TABLE sessions ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
