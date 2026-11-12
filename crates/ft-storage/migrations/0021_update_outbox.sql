-- Edits, takings back and reactions to tell a contact (2026-10-06): the packet as it was made,
-- tried again like a message until the contact's receipt, so this phone never shows as done what
-- the other has not heard of (§84). It goes with the contact.
CREATE TABLE update_outbox (
    packet_id    TEXT    PRIMARY KEY,
    contact      TEXT    NOT NULL REFERENCES contacts (device_id) ON DELETE CASCADE,
    packet       BLOB    NOT NULL,
    created_at   INTEGER NOT NULL,
    attempts     INTEGER NOT NULL DEFAULT 0,
    next_attempt INTEGER NOT NULL,
    in_mailbox   INTEGER NOT NULL DEFAULT 0
);

-- When each side's reaction was made, by its author's clock (ms): one made earlier and arriving
-- later does not replace it. A reaction taken back stays as an empty emoji, so that an older one
-- arriving after it does not bring it back.
ALTER TABLE reactions ADD COLUMN reacted_at INTEGER NOT NULL DEFAULT 0;
