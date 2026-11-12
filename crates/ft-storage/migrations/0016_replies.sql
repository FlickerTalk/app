-- Which message a text answers (2026-10-05): the quote over the bubble. Its own table, so a
-- message that answers nothing has no row; it goes with the message.
CREATE TABLE message_replies (
    message_id TEXT PRIMARY KEY REFERENCES messages (message_id) ON DELETE CASCADE,
    reply_to   TEXT NOT NULL
);
