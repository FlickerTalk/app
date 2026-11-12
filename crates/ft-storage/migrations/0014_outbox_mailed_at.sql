-- ms of the last mailbox copy, renewed at most once a day; NULL means the next retry leaves one.
ALTER TABLE pending_outbox ADD COLUMN mailed_at INTEGER;
