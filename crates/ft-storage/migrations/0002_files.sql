-- Files (Plan §62–63): the message carries the name; this row, what the transfer needs. The bytes
-- are in a file of the app's storage, never in the database.
CREATE TABLE files (
    message_id  TEXT    PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    size        INTEGER NOT NULL,
    mime        TEXT    NOT NULL,
    hash        BLOB    NOT NULL,           -- BLAKE3 of the whole file
    chunk       INTEGER NOT NULL,           -- bytes per chunk
    path        TEXT    NOT NULL,           -- where the bytes are on this device
    chunks_done INTEGER NOT NULL DEFAULT 0, -- outgoing: chunks sent; incoming: chunks received in order
    complete    INTEGER NOT NULL DEFAULT 0, -- the receiver has it all and the hash matches
    failed      INTEGER NOT NULL DEFAULT 0  -- the bytes did not match the hash: given up
);
