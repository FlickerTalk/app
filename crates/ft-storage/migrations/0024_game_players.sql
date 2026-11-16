CREATE TABLE game_players (
    session     TEXT REFERENCES sessions (id) ON DELETE CASCADE,
    who         TEXT NOT NULL,
    hair        TEXT NOT NULL,
    hair_colour TEXT NOT NULL,
    name        TEXT NOT NULL
);
CREATE UNIQUE INDEX game_players_place ON game_players (ifnull(session, ''));
