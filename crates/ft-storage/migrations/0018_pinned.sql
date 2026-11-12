-- Pinning stays on this phone and is told to nobody.
CREATE TABLE pinned (
    message_id TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    pinned_at  INTEGER NOT NULL
);
