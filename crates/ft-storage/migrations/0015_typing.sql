-- Typing travels only over the direct connection, never through the mailbox or the server.
ALTER TABLE contacts ADD COLUMN typing INTEGER NOT NULL DEFAULT 1;
