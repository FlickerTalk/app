-- One emoji per side and message: ours (mine = 1) and theirs (mine = 0).
CREATE TABLE reactions (
    message_id TEXT    NOT NULL REFERENCES messages (message_id) ON DELETE CASCADE,
    mine       INTEGER NOT NULL,
    emoji      TEXT    NOT NULL,
    PRIMARY KEY (message_id, mine)
);
