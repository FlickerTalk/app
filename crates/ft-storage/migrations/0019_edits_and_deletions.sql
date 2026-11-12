-- Only the marks: the words live in the message.
CREATE TABLE message_edits (
    message_id TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    edited_at  INTEGER NOT NULL
);
CREATE TABLE message_deletions (
    message_id TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    deleted_at INTEGER NOT NULL
);
