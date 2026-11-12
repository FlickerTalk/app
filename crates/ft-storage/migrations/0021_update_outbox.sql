-- Retried until the contact's receipt, so nothing shows as done that they have not heard of.
CREATE TABLE update_outbox (
    packet_id    TEXT    PRIMARY KEY,
    contact      TEXT    NOT NULL REFERENCES contacts (device_id) ON DELETE CASCADE,
    packet       BLOB    NOT NULL,
    created_at   INTEGER NOT NULL,
    attempts     INTEGER NOT NULL DEFAULT 0,
    next_attempt INTEGER NOT NULL,
    in_mailbox   INTEGER NOT NULL DEFAULT 0
);

-- Author's clock, ms: an older reaction never replaces a newer one; a removed one stays as an empty emoji.
ALTER TABLE reactions ADD COLUMN reacted_at INTEGER NOT NULL DEFAULT 0;
