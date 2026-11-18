-- Sunrise's own starting state for a created character. Each value is the one the default save
-- gives its characters. The items go ahead of the packages' new-character table, which names no
-- subclass. Classes: 0 Titan, 1 Hunter, 2 Warlock.
CREATE TABLE starter_character (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    level INTEGER NOT NULL CHECK (level BETWEEN 0 AND 255),
    preview_available INTEGER NOT NULL CHECK (preview_available IN (0, 1)),
    appearance_value REAL NOT NULL,
    last_orbited_destination INTEGER NOT NULL,
    content_bypass INTEGER NOT NULL CHECK (content_bypass IN (0, 1))
) STRICT;

CREATE TABLE starter_items (
    class INTEGER NOT NULL CHECK (class BETWEEN 0 AND 2),
    position INTEGER NOT NULL CHECK (position >= 0),
    definition_hash INTEGER NOT NULL CHECK (definition_hash BETWEEN 1 AND 4294967295),
    PRIMARY KEY (class, position)
) STRICT;

INSERT INTO starter_character VALUES (1, 50, 1, 1.0, 308080871, 1);
INSERT INTO starter_items VALUES (0, 0, 3105935002);
INSERT INTO starter_items VALUES (1, 0, 3635991036);
INSERT INTO starter_items VALUES (2, 0, 3481861797);
