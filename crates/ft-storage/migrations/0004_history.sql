-- Per-contact history rules (issue app#1). They belong to this phone: nothing of this travels to
-- the contact or to the server. Seconds; 0 means "keep forever" and "never burn".
ALTER TABLE contacts ADD COLUMN keep_for INTEGER NOT NULL DEFAULT 0;
ALTER TABLE contacts ADD COLUMN burn_after_read INTEGER NOT NULL DEFAULT 0;

-- When the message became read, so it can disappear a while later. Milliseconds.
ALTER TABLE messages ADD COLUMN read_at INTEGER;
