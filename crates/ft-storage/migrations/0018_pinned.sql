-- Messages pinned on this phone (2026-10-05): a choice of this phone, told to nobody. It goes with
-- the message.
CREATE TABLE pinned (
    message_id TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    pinned_at  INTEGER NOT NULL
);
