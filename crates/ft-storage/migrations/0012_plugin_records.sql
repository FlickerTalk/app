-- Plugins, phase 3 (2026-09-27): what a plugin keeps beyond its settings (notes, boards), still
-- apart from every other plugin and within the room the user granted it. Bytes, not text: a
-- board or a picture goes in as it is. Taking the plugin out takes its records with it.
CREATE TABLE plugin_records (
    plugin     TEXT    NOT NULL,
    key        TEXT    NOT NULL,
    value      BLOB    NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (plugin, key)
);

-- Local reminders (2026-09-27): a notification on this phone at a time a plugin picked. This
-- table is the truth; the OS is only the alarm clock, and is told again from here at every start.
CREATE TABLE reminders (
    plugin TEXT    NOT NULL,
    id     TEXT    NOT NULL,
    at     INTEGER NOT NULL,           -- milliseconds
    text   TEXT    NOT NULL DEFAULT '',-- what the notification says, if the user allows content
    PRIMARY KEY (plugin, id)
);

CREATE INDEX reminders_by_time ON reminders (at);

-- An opaque handle a plugin gets for the message it was opened with: it can ask the app to go
-- back to that conversation, and learns nothing of who it is with.
CREATE TABLE plugin_refs (
    ref        TEXT    PRIMARY KEY,
    plugin     TEXT    NOT NULL,
    contact    TEXT    NOT NULL,
    message_id TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);
