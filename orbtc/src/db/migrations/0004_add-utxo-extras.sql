CREATE TABLE IF NOT EXISTS outputs_extras (
    id               BIGINT  PRIMARY KEY REFERENCES outputs (id),
    has_runes        BOOLEAN NOT NULL    DEFAULT FALSE,
    has_inscriptions BOOLEAN NOT NULL    DEFAULT FALSE
);


CREATE TABLE IF NOT EXISTS outputs_runes_ext (
    id          BIGINT  PRIMARY KEY REFERENCES outputs (id),
    rune        VARCHAR NOT NULL    REFERENCES runes (name),
    rune_id     VARCHAR NOT NULL    REFERENCES runes (rune_id),
    rune_amount NUMERIC NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_outputs_runes_ext_rune ON outputs_runes_ext (rune);
CREATE INDEX IF NOT EXISTS idx_outputs_runes_ext_rune_id ON outputs_runes_ext (rune_id);
CREATE INDEX IF NOT EXISTS idx_outputs_runes_ext_amount ON outputs_runes_ext (rune_amount);
