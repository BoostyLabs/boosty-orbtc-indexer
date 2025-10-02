CREATE TABLE IF NOT EXISTS runes (
    block          BIGINT     NOT NULL,
    tx_id          INTEGER    NOT NULL,
    rune_id        VARCHAR    NOT NULL, -- "block:tx_id"
    name           VARCHAR    NOT NULL,
    display_name   VARCHAR    NOT NULL,
    symbol         VARCHAR(4) NOT NULL,
    divisibility   INTEGER    NOT NULL DEFAULT 0,
    max_supply     NUMERIC    NOT NULL,
    mints          INTEGER    NOT NULL DEFAULT 0,
    premine        NUMERIC    NOT NULL DEFAULT 0,
    burned         NUMERIC    NOT NULL DEFAULT 0,
    minted         NUMERIC    NOT NULL DEFAULT 0,
    in_circulation NUMERIC    NOT NULL DEFAULT 0,
    turbo          BOOLEAN    NOT NULL DEFAULT FALSE,
    cenotaph       BOOLEAN    NOT NULL DEFAULT FALSE,
    is_featured    BOOLEAN    NOT NULL DEFAULT FALSE,
    block_time     BIGINT     NOT NULL DEFAULT 0,
    etching_tx     BYTEA      NOT NULL,
    commitment_tx  BYTEA      NOT NULL,
    raw_data       BYTEA      NOT NULL,

    PRIMARY KEY (block, tx_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_runes_name ON runes (name);
CREATE UNIQUE INDEX IF NOT EXISTS idx_runes_rune_id ON runes (rune_id);

CREATE TABLE IF NOT EXISTS runes_outputs (
    id         BIGSERIAL PRIMARY KEY,
    block      BIGINT    NOT NULL,
    tx_id      INT       NOT NULL,
    tx_hash    BYTEA     NOT NULL,
    vout       INT       NOT NULL,
    rune       VARCHAR   NOT NULL REFERENCES runes (name),
    rune_id    VARCHAR   NOT NULL REFERENCES runes (rune_id),
    address    VARCHAR   NOT NULL,
    amount     NUMERIC   NOT NULL,
    btc_amount BIGINT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runes_outputs_block ON runes_outputs (block);
CREATE INDEX IF NOT EXISTS idx_runes_outputs_tx_vout ON runes_outputs (tx_hash, vout);
CREATE INDEX IF NOT EXISTS idx_runes_outputs_address ON runes_outputs (address);
CREATE INDEX IF NOT EXISTS idx_runes_outputs_rune ON runes_outputs (rune);
CREATE INDEX IF NOT EXISTS idx_runes_outputs_rune_id ON runes_outputs (rune_id);
CREATE INDEX IF NOT EXISTS idx_runes_outputs_amount ON runes_outputs (amount);

CREATE OR REPLACE VIEW runes_utxos AS
SELECT
    o.id,
    o.block,
    o.tx_id,
    o.tx_hash,
    o.vout,
    o.rune_id,
    o.rune,
    a.address,
    a.pk_script,
    o.amount,
    o.btc_amount
FROM runes_outputs AS o
    LEFT JOIN inputs AS i
        ON o.tx_hash = i.parent_tx AND o.vout = i.parent_vout
    LEFT JOIN addresses AS a
        ON o.address = a.address
WHERE i.parent_tx IS NULL AND i.parent_vout IS NULL;

CREATE OR REPLACE VIEW runes_balances AS
SELECT
    address,
    rune_id,
    rune,
    sum(amount) AS balance,
    sum(btc_amount) AS btc_balance,
    count(*) AS utxo_count
FROM runes_utxos
GROUP BY address, rune_id, rune;
