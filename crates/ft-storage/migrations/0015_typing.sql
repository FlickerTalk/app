-- Whether the contact is told when the user is writing to them (2026-10-05). It travels only over
-- the direct connection, never through the mailbox or the server, and this switch stays here.
ALTER TABLE contacts ADD COLUMN typing INTEGER NOT NULL DEFAULT 1;
