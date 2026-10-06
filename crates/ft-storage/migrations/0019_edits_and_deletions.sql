-- A text said again with other words (2026-10-05), and a message taken back for both sides: only
-- the mark; the words live in the message. Both go with it.
CREATE TABLE message_edits (
    message_id TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    edited_at  INTEGER NOT NULL
);
CREATE TABLE message_deletions (
    message_id TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    deleted_at INTEGER NOT NULL
);
