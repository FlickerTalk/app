-- What each plugin remembers (issue app#4, §53). A plugin runs in a frame with no origin of its
-- own, so the browser gives it no storage: it asks the core, and the core keeps it here, apart
-- from every other plugin. Taking the plugin out takes its memory with it.
CREATE TABLE plugin_memory (
    plugin TEXT NOT NULL,
    key    TEXT NOT NULL,
    value  TEXT NOT NULL,
    PRIMARY KEY (plugin, key)
);
