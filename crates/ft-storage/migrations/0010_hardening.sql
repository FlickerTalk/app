-- The envelope key, sealed at rest like the Olm account; NULL until the core makes one.
ALTER TABLE identity ADD COLUMN envelope TEXT;

-- Whether the user chose this contact (scanned, or accepted their request); everyone known before was.
ALTER TABLE contacts ADD COLUMN accepted INTEGER NOT NULL DEFAULT 1;

-- When the message reached this phone, by its own clock; history and order go by it.
ALTER TABLE messages ADD COLUMN received_at INTEGER NOT NULL DEFAULT 0;
UPDATE messages SET received_at = sent_at WHERE received_at = 0;
CREATE INDEX messages_by_arrival ON messages (contact, received_at);

-- An incoming file bigger than the user downloads on their own waits until they ask.
ALTER TABLE files ADD COLUMN waiting INTEGER NOT NULL DEFAULT 0;
