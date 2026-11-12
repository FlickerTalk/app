-- The message's outbox entry waits until send_at.
CREATE TABLE scheduled (
    message_id TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    send_at    INTEGER NOT NULL
);
