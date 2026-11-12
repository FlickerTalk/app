-- When this phone last unblocked the contact, by its own clock (ms); NULL if it never did
-- (app#146). What they wrote before it, while blocked, can still arrive after it and is dropped.
ALTER TABLE contacts ADD COLUMN unblocked_at INTEGER;
