-- This phone's clock, ms: what they wrote before it, while blocked, may still arrive and is dropped.
ALTER TABLE contacts ADD COLUMN unblocked_at INTEGER;
