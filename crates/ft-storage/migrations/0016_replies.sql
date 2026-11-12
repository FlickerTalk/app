CREATE TABLE message_replies (
    message_id TEXT PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    reply_to   TEXT NOT NULL
);
