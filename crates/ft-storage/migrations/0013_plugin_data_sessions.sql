-- Hidden sessions and plugins (2026-10-01, Plan §108): what a plugin keeps while it is open inside
-- a hidden session belongs to that session. Its records, its settings and its reminders say which
-- session they belong to (NULL for the main list), the same key in two places is two things, and
-- they go with the session. What was kept before this says nothing of where it came from: it
-- stays in the main list.
--
-- SQLite cannot change a primary key, so each table is made again. A NULL session cannot be
-- told apart in a primary key, so the key is a unique index that reads it as ''.

CREATE TABLE plugin_records_by_session (
    plugin     TEXT    NOT NULL,
    session    TEXT    REFERENCES sessions (id) ON DELETE CASCADE, -- NULL for the main list
    key        TEXT    NOT NULL,
    value      BLOB    NOT NULL,
    updated_at INTEGER NOT NULL
);
INSERT INTO plugin_records_by_session (plugin, session, key, value, updated_at)
    SELECT plugin, NULL, key, value, updated_at FROM plugin_records;
DROP TABLE plugin_records;
ALTER TABLE plugin_records_by_session RENAME TO plugin_records;
CREATE UNIQUE INDEX plugin_records_key ON plugin_records (plugin, ifnull(session, ''), key);

CREATE TABLE plugin_memory_by_session (
    plugin  TEXT NOT NULL,
    session TEXT REFERENCES sessions (id) ON DELETE CASCADE, -- NULL for the main list
    key     TEXT NOT NULL,
    value   TEXT NOT NULL
);
INSERT INTO plugin_memory_by_session (plugin, session, key, value)
    SELECT plugin, NULL, key, value FROM plugin_memory;
DROP TABLE plugin_memory;
ALTER TABLE plugin_memory_by_session RENAME TO plugin_memory;
CREATE UNIQUE INDEX plugin_memory_key ON plugin_memory (plugin, ifnull(session, ''), key);

CREATE TABLE reminders_by_session (
    plugin  TEXT    NOT NULL,
    session TEXT    REFERENCES sessions (id) ON DELETE CASCADE, -- NULL for the main list
    id      TEXT    NOT NULL,
    at      INTEGER NOT NULL,            -- milliseconds
    text    TEXT    NOT NULL DEFAULT ''  -- what the notification says, if the user allows content
);
INSERT INTO reminders_by_session (plugin, session, id, at, text)
    SELECT plugin, NULL, id, at, text FROM reminders;
DROP TABLE reminders;
ALTER TABLE reminders_by_session RENAME TO reminders;
CREATE UNIQUE INDEX reminders_key ON reminders (plugin, ifnull(session, ''), id);
CREATE INDEX reminders_by_time ON reminders (at);
