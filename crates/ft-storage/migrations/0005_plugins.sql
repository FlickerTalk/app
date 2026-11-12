-- Installing grants nothing: `granted` holds what the user accepted, and an update keeps it.
CREATE TABLE plugins (
    id           TEXT    PRIMARY KEY,   -- the manifest's id, e.g. com.example.code
    version      TEXT    NOT NULL,
    granted      TEXT    NOT NULL,      -- the granted permissions, as the manifest writes them
    installed_at INTEGER NOT NULL
);
