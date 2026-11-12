-- A plugin's frame has no origin and so no browser storage; the core keeps it here, per plugin.
CREATE TABLE plugin_memory (
    plugin TEXT NOT NULL,
    key    TEXT NOT NULL,
    value  TEXT NOT NULL,
    PRIMARY KEY (plugin, key)
);
