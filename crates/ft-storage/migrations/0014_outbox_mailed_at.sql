-- When the last copy of a message went to the mailbox (2026-10-01, milliseconds): a message
-- already there is still tried directly at every retry, but gets a new copy only once a day.
-- NULL on what was queued before this column: its next retry leaves a copy, as before.
ALTER TABLE pending_outbox ADD COLUMN mailed_at INTEGER;
