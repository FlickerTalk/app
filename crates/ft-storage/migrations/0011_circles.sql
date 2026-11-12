-- Circles (2026-09-27): a small closed group of contacts, held as its signed card (ft-circles).
-- Nothing of this reaches the server: it never learns a circle exists.
CREATE TABLE circles (
    id          TEXT    PRIMARY KEY,        -- the card's random id
    name        TEXT    NOT NULL,
    card        BLOB    NOT NULL,           -- the current signed card
    revision    INTEGER NOT NULL,
    admins_only INTEGER NOT NULL DEFAULT 0, -- only the admins write
    session     TEXT    REFERENCES sessions (id) ON DELETE CASCADE, -- NULL for the main list
    left        INTEGER NOT NULL DEFAULT 0, -- this phone left, or was taken out: read only
    created_at  INTEGER NOT NULL
);

-- What the card says, spelled out for the lists and the checks.
CREATE TABLE circle_members (
    circle    TEXT    NOT NULL REFERENCES circles (id) ON DELETE CASCADE,
    device_id TEXT    NOT NULL,
    name      TEXT    NOT NULL,             -- as their own card names them
    admin     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (circle, device_id)
);

-- What was said in a circle. `sender` is a device id, this phone's for outgoing. `kind` is
-- `text`, or something that happened (`joined`, `left`, `removed`, `renamed`, `created`), or a
-- control packet that travels through the outbox and is never shown (`card`, `leave`).
CREATE TABLE circle_messages (
    message_id  TEXT    PRIMARY KEY,
    circle      TEXT    NOT NULL REFERENCES circles (id) ON DELETE CASCADE,
    sender      TEXT    NOT NULL,
    outgoing    INTEGER NOT NULL,
    kind        TEXT    NOT NULL DEFAULT 'text',
    body        TEXT    NOT NULL,
    sent_at     INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    state       INTEGER NOT NULL
);

CREATE INDEX circle_messages_by_arrival ON circle_messages (circle, received_at);

-- One entry per member a circle packet still has to reach; it goes with their receipt.
CREATE TABLE circle_outbox (
    message_id   TEXT    NOT NULL REFERENCES circle_messages (message_id) ON DELETE CASCADE,
    contact      TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    attempts     INTEGER NOT NULL DEFAULT 0,
    next_attempt INTEGER NOT NULL,
    in_mailbox   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (message_id, contact)
);

-- A contact known only because they are in a circle with this phone: reachable, but in no list
-- and in no requests until they write on their own or the user chooses them.
ALTER TABLE contacts ADD COLUMN via_circle INTEGER NOT NULL DEFAULT 0;
