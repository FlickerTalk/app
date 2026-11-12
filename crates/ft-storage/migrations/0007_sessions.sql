-- Hidden sessions (Plan, 2026-09-23): a space of its own, opened with a 6-digit PIN and nothing
-- else. Only the PIN's keyed hash is kept: the key lives in the phone's key store, so a copy of
-- this database alone cannot try the million PINs. Nothing here has a name: nothing to read.
CREATE TABLE sessions (
    id         TEXT    PRIMARY KEY,
    pin_hash   BLOB    NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

-- A contact belongs to the main list (NULL) or to one session, and never moves.
ALTER TABLE contacts ADD COLUMN session TEXT REFERENCES sessions (id) ON DELETE CASCADE;
