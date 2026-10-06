-- Messages written now to be sent later (2026-10-06): when. The outbox entry waits until then.
-- It goes with the message.
CREATE TABLE scheduled (
    message_id TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    send_at    INTEGER NOT NULL
);
