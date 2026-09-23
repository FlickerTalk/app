-- What this phone takes from each contact and what it tells them (issues app#4–#6). All of it
-- stays here: the contact never learns it and the server never sees it.
ALTER TABLE contacts ADD COLUMN muted INTEGER NOT NULL DEFAULT 0;         -- rings and shows, silently
ALTER TABLE contacts ADD COLUMN accepts_chat INTEGER NOT NULL DEFAULT 1;  -- their messages are kept
ALTER TABLE contacts ADD COLUMN accepts_calls INTEGER NOT NULL DEFAULT 1; -- their calls come in
ALTER TABLE contacts ADD COLUMN receipts INTEGER NOT NULL DEFAULT 1;      -- they see delivered and read
