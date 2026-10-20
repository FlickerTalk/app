-- The chat's hooks (2026-10-20, plan of the translator §8): what a tool said about a message (a
-- translation, or the original of what was sent translated), one per tool and message; and, for
-- each conversation (a contact or a circle, by its id) and tool, whether the tool runs by itself on
-- every message received and on every text sent, and the conversation's language as the tool found
-- it (a BCP-47 tag; NULL until it says). Neither hangs from a table, since a message or a
-- conversation may be of a contact or of a circle: whatever takes a message, a conversation or the
-- tool away takes them too, and an edit or a taking back takes the message's notes. Both stay on
-- this phone.
CREATE TABLE message_notes (
    message_id TEXT    NOT NULL,
    plugin     TEXT    NOT NULL,
    text       TEXT    NOT NULL,
    at         INTEGER NOT NULL,
    PRIMARY KEY (message_id, plugin)
);
CREATE TABLE chat_hooks (
    conversation  TEXT    NOT NULL,
    plugin        TEXT    NOT NULL,
    auto_message  INTEGER NOT NULL DEFAULT 0,
    auto_composer INTEGER NOT NULL DEFAULT 0,
    lang          TEXT,
    PRIMARY KEY (conversation, plugin)
);
