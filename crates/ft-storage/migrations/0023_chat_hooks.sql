-- The chat's hooks (2026-10-20, plan of the translator §8): what a tool said about a message (a
-- translation), shown under it, one per tool; it goes with the message, and an edit or a taking
-- back takes it too. And, for each contact and tool, whether the tool runs by itself on every
-- message received and on every text sent. Both stay on this phone and go with the tool.
CREATE TABLE message_notes (
    message_id TEXT    NOT NULL REFERENCES messages (message_id) ON DELETE CASCADE,
    plugin     TEXT    NOT NULL,
    text       TEXT    NOT NULL,
    at         INTEGER NOT NULL,
    PRIMARY KEY (message_id, plugin)
);
CREATE TABLE chat_hooks (
    contact       TEXT    NOT NULL REFERENCES contacts (device_id) ON DELETE CASCADE,
    plugin        TEXT    NOT NULL,
    auto_message  INTEGER NOT NULL DEFAULT 0,
    auto_composer INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (contact, plugin)
);
