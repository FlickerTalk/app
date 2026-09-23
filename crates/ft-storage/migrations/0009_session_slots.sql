-- Hidden sessions, phase 2 (app#9): seven spare route capabilities besides the device's own. The
-- router always gets all eight, so it cannot tell how many are in use. A session hands out the
-- capability of its slot in its contact cards, so a wake-up says which session it is for.
CREATE TABLE spare_capabilities (
    slot       INTEGER PRIMARY KEY CHECK (slot BETWEEN 1 AND 7),
    capability BLOB    NOT NULL
);

-- NULL for sessions made before this migration: the core gives them a slot when they open.
ALTER TABLE sessions ADD COLUMN slot INTEGER;
CREATE UNIQUE INDEX sessions_by_slot ON sessions (slot) WHERE slot IS NOT NULL;
