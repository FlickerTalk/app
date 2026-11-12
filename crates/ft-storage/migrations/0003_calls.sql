-- Call history (Plan §66): only on the phone. Times in milliseconds.
CREATE TABLE calls (
    call_id     TEXT    PRIMARY KEY,        -- UUIDv7 made by the caller
    contact     TEXT    NOT NULL REFERENCES contacts (device_id) ON DELETE CASCADE,
    outgoing    INTEGER NOT NULL,
    video       INTEGER NOT NULL,
    started_at  INTEGER NOT NULL,
    answered_at INTEGER,
    ended_at    INTEGER,
    outcome     TEXT                        -- NULL while it goes on; see CallOutcome
);

CREATE INDEX calls_by_time ON calls (started_at);
