-- Only the PIN's keyed hash, with the key in the phone's key store: this file alone cannot try PINs.
CREATE TABLE sessions (
    id         TEXT    PRIMARY KEY,
    pin_hash   BLOB    NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

-- A contact belongs to the main list (NULL) or to one session, and never moves.
ALTER TABLE contacts ADD COLUMN session TEXT REFERENCES sessions (id) ON DELETE CASCADE;
