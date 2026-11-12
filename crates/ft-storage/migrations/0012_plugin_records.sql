-- Bytes, so a board or a picture goes in as it is; removing the plugin removes its records.
CREATE TABLE plugin_records (
    plugin     TEXT    NOT NULL,
    key        TEXT    NOT NULL,
    value      BLOB    NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (plugin, key)
);

-- This table is the truth; the OS is only the alarm clock and is told again from here at every start.
CREATE TABLE reminders (
    plugin TEXT    NOT NULL,
    id     TEXT    NOT NULL,
    at     INTEGER NOT NULL,           -- milliseconds
    text   TEXT    NOT NULL DEFAULT '',-- what the notification says, if the user allows content
    PRIMARY KEY (plugin, id)
);

CREATE INDEX reminders_by_time ON reminders (at);

-- An opaque handle for the message a plugin was opened with; it learns nothing of who it is with.
CREATE TABLE plugin_refs (
    ref        TEXT    PRIMARY KEY,
    plugin     TEXT    NOT NULL,
    contact    TEXT    NOT NULL,
    message_id TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);
