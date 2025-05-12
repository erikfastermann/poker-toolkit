BEGIN;

CREATE TABLE IF NOT EXISTS hands_data(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    hand_data TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS hands(
    id INTEGER NOT NULL PRIMARY KEY,
    unit TEXT,
    max_players INTEGER,
    game_location TEXT,
    game_date TEXT,
    table_name TEXT,
    hand_name TEXT UNIQUE,
    hero_index INTEGER,

    small_blind INTEGER NOT NULL,
    big_blind INTEGER NOT NULL,
    button_index INTEGER NOT NULL,

    first_flop TEXT,
    first_turn TEXT,
    first_river TEXT,

    pot_kind TEXT NOT NULL, -- 'walk', 'limped', 'srp', '3-bet', '4-bet', '5-bet+'
    posting INTEGER NOT NULL, -- BOOLEAN
    straddling INTEGER NOT NULL, -- BOOLEAN
    pre_flop_limping INTEGER NOT NULL, -- BOOLEAN
    pre_flop_cold_calling INTEGER NOT NULL, -- BOOLEAN, can only be TRUE in 3-bet+ pot

    players_post_flop INTEGER,
    players_at_showdown INTEGER,
    single_winner INTEGER,
    final_full_pot_size INTEGER NOT NULL,

    FOREIGN KEY(id) REFERENCES hands(id)
) STRICT;

CREATE TABLE IF NOT EXISTS hands_players(
    hand_id INTEGER NOT NULL,
    player INTEGER NOT NULL,

    player_name TEXT,
    seat INTEGER,
    hand TEXT,
    went_to_showdown INTEGER NOT NULL, -- BOOLEAN

    starting_stack INTEGER NOT NULL,
    pot_contribution INTEGER NOT NULL,
    showdown_stack INTEGER NOT NULL,

    -- p: post, s:straddle, f: fold, x: check, c: call, b: bet, r: raise
    pre_flop_action TEXT NOT NULL,
    flop_action TEXT,
    turn_action TEXT,
    river_action TEXT,

    PRIMARY KEY (hand_id, player),
    FOREIGN KEY(hand_id) REFERENCES hands(id)
) WITHOUT ROWID, STRICT;

COMMIT;
