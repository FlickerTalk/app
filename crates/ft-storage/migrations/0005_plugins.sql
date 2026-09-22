-- Plugins installed on this phone and what the user granted each one (issue app#3, §53).
-- Installing grants nothing: `granted` starts as the plugin asked and the user accepted, and an
-- update keeps it. The files of the plugin live in the app's folder, not here.
CREATE TABLE plugins (
    id           TEXT    PRIMARY KEY,   -- the manifest's id, e.g. com.example.code
    version      TEXT    NOT NULL,
    granted      TEXT    NOT NULL,      -- the granted permissions, as the manifest writes them
    installed_at INTEGER NOT NULL
);
