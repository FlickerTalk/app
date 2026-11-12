-- No foreign keys: a message or conversation may be a contact's or a circle's, so the code deletes these rows.
CREATE TABLE message_notes (
    message_id TEXT    NOT NULL,
    plugin     TEXT    NOT NULL,
    text       TEXT    NOT NULL,
    at         INTEGER NOT NULL,
    PRIMARY KEY (message_id, plugin)
);
-- conversation is a contact or circle id; lang is a BCP-47 tag, NULL until the tool finds it.
CREATE TABLE chat_hooks (
    conversation  TEXT    NOT NULL,
    plugin        TEXT    NOT NULL,
    auto_message  INTEGER NOT NULL DEFAULT 0,
    auto_composer INTEGER NOT NULL DEFAULT 0,
    lang          TEXT,
    PRIMARY KEY (conversation, plugin)
);
