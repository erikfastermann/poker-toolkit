PRAGMA encoding = 'UTF-8';
PRAGMA synchronous = EXTRA;
PRAGMA foreign_keys = ON;

CREATE TABLE hands_data(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    hand_data TEXT NOT NULL
) STRICT;

CREATE TABLE hands(
    id INTEGER NOT NULL PRIMARY KEY,
    unit TEXT,
    max_players INTEGER,
    game_location TEXT,
    game_date DATETIME,
    table_name TEXT,
    hand_name TEXT UNIQUE,

    small_blind INTEGER NOT NULL,
    big_blind INTEGER NOT NULL,
    button_index INTEGER NOT NULL,

    hero INTEGER,
    hero_at_showdown BOOLEAN,
    hero_pot_contribution INTEGER,
    hero_win_loss INTEGER,

    first_flop TEXT,
    first_turn TEXT,
    first_river TEXT,

    pot_kind TEXT NOT NULL, -- 'limped', 'srp', '3-bet', '4-bet', '5-bet+'
    pre_flop_limping BOOLEAN NOT NULL,
    pre_flop_cold_calling BOOLEAN NOT NULL, -- can only be TRUE in 3-bet+ pot

    players_post_flop INTEGER,
    went_to_showdown BOOLEAN NOT NULL,
    single_winner INTEGER,

    final_full_pot_size INTEGER NOT NULL,

    FOREIGN KEY(id) REFERENCES hands(id)
) STRICT;

CREATE TABLE hands_players(
    hand_id INTEGER NOT NULL,
    player INTEGER NOT NULL,

    player_name TEXT,
    seat INTEGER,
    hand TEXT,
    hand_kind_at_showdown TEXT,
    hand_score_at_showdown INTEGER,

    starting_stack INTEGER NOT NULL,
    showdown_stack INTEGER NOT NULL,

    pre_flop_action TEXT NOT NULL,
    flop_action TEXT,
    turn_action TEXT,
    river_action TEXT,

    PRIMARY KEY (hand_id, player),
    FOREIGN KEY(hand_id) REFERENCES hands(id)
) STRICT WITHOUT ROWID;
