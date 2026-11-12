-- One emoji per person and message (2026-10-05): ours (mine = 1) and theirs (mine = 0). It goes
-- with the message.
CREATE TABLE reactions (
    message_id TEXT    NOT NULL REFERENCES messages (message_id) ON DELETE CASCADE,
    mine       INTEGER NOT NULL,
    emoji      TEXT    NOT NULL,
    PRIMARY KEY (message_id, mine)
);
