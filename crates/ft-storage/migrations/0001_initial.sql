-- Local database of the device (Plan §25–27). Everything here stays on the phone.

-- This device's identity: the sealed Olm account and its route capability (§34).
CREATE TABLE identity (
    id               INTEGER PRIMARY KEY CHECK (id = 1),
    sealed           TEXT    NOT NULL,
    route_capability BLOB    NOT NULL,
    created_at       INTEGER NOT NULL
);

-- One row per contact; a 1:1 conversation is identified by its contact (§25).
CREATE TABLE contacts (
    device_id TEXT    PRIMARY KEY,
    name      TEXT    NOT NULL,
    card      BLOB    NOT NULL,            -- their signed Contact Card
    channel   TEXT,                        -- sealed Olm sessions with them (ft-crypto)
    mailbox   INTEGER NOT NULL,            -- whether they use the mailbox (§19)
    blocked   INTEGER NOT NULL DEFAULT 0,  -- §35
    introduced INTEGER NOT NULL DEFAULT 0, -- they have written back, so they know our card
    added_at  INTEGER NOT NULL
);

-- state: 0 pending, 1 sent, 2 delivered, 3 read. Incoming messages start delivered (2) and become
-- read (3) once shown. It only ever moves forward.
CREATE TABLE messages (
    message_id TEXT    PRIMARY KEY,        -- UUIDv7; unique, so retries are idempotent (§27)
    contact    TEXT    NOT NULL REFERENCES contacts (device_id) ON DELETE CASCADE,
    outgoing   INTEGER NOT NULL,
    body       TEXT    NOT NULL,
    sent_at    INTEGER NOT NULL,           -- sender's clock, milliseconds
    state      INTEGER NOT NULL
);

CREATE INDEX messages_by_contact ON messages (contact, sent_at);

-- Messages not yet delivered (§26): they stay here until the DELIVERED receipt arrives.
CREATE TABLE pending_outbox (
    message_id   TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    contact      TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    attempts     INTEGER NOT NULL DEFAULT 0,
    next_attempt INTEGER NOT NULL,
    in_mailbox   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
