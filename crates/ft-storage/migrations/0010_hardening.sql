-- Hardening pack (review of 2026-09-24).

-- A1: the envelope key, sealed at rest like the Olm account; NULL until the core makes one.
ALTER TABLE identity ADD COLUMN envelope TEXT;

-- A5: whether the user chose this contact (scanned them, or accepted their request). Everyone
-- known so far was either scanned or has been talking already: chosen.
ALTER TABLE contacts ADD COLUMN accepted INTEGER NOT NULL DEFAULT 1;

-- M5: when the message reached this phone, by its own clock; what history and order go by.
-- What is already here is stamped with the sender's time: the best guess there is.
ALTER TABLE messages ADD COLUMN received_at INTEGER NOT NULL DEFAULT 0;
UPDATE messages SET received_at = sent_at WHERE received_at = 0;
CREATE INDEX messages_by_arrival ON messages (contact, received_at);

-- A4: an incoming file bigger than the user downloads on their own waits until they ask.
ALTER TABLE files ADD COLUMN waiting INTEGER NOT NULL DEFAULT 0;
