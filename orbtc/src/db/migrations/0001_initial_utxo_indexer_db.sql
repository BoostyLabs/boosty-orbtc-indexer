CREATE TABLE IF NOT EXISTS last_indexed_block (
    indexer VARCHAR NOT NULL UNIQUE PRIMARY KEY,
    height  BIGINT  NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS blocks (
    height    BIGINT  NOT NULL,
    indexer   VARCHAR NOT NULL,
    hash      BYTEA   NOT NULL,
    blocktime BIGINT  NOT NULL,

    PRIMARY KEY (height, indexer)
);

CREATE INDEX IF NOT EXISTS idx_blocks_hash ON blocks (hash);
CREATE INDEX IF NOT EXISTS idx_blocks_height ON blocks (height);

CREATE TABLE IF NOT EXISTS orphaned_blocks (
    height    BIGINT NOT NULL PRIMARY KEY,
    hash      BYTEA  NOT NULL,
    blocktime BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS addresses (
    id           BIGSERIAL PRIMARY KEY,
    address      VARCHAR   NOT NULL UNIQUE,
    address_type VARCHAR   NOT NULL,
    pk_script    BYTEA     NOT NULL
);

CREATE TABLE IF NOT EXISTS outputs (
    id       BIGSERIAL PRIMARY KEY,
    block    BIGINT    NOT NULL,
    tx_id    INT       NOT NULL,
    tx_hash  BYTEA     NOT NULL,
    vout     INT       NOT NULL,
    address  VARCHAR   NOT NULL,
    amount   BIGINT    NOT NULL,
    coinbase BOOLEAN   NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_outputs_block ON outputs (block);
CREATE INDEX IF NOT EXISTS idx_outputs_tx_vout ON outputs (tx_hash, vout);
CREATE INDEX IF NOT EXISTS idx_outputs_address ON outputs (address);
CREATE INDEX IF NOT EXISTS idx_outputs_amount ON outputs (amount);

CREATE TABLE IF NOT EXISTS inputs (
    id          BIGSERIAL PRIMARY KEY,
    block       BIGINT    NOT NULL,
    tx_id       INT       NOT NULL,
    tx_hash     BYTEA     NOT NULL,
    vin         INT       NOT NULL,
    parent_tx   BYTEA     NOT NULL,
    parent_vout INT       NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_inputs_block ON inputs (block);
CREATE INDEX IF NOT EXISTS idx_inputs_tx_vout ON inputs (tx_hash, vin);
CREATE INDEX IF NOT EXISTS idx_inputs_parent_tx_vout ON inputs (parent_tx, parent_vout);

CREATE OR REPLACE VIEW utxos AS
SELECT
    o.id,
    o.block,
    o.tx_id,
    o.tx_hash,
    o.vout,
    a.address,
    a.pk_script,
    o.amount,
    o.coinbase,
    false AS spend
FROM outputs AS o
    LEFT JOIN inputs AS i
        ON o.tx_hash = i.parent_tx AND o.vout = i.parent_vout
    LEFT JOIN addresses AS a
        ON o.address = a.address
WHERE i.parent_tx IS null AND i.parent_vout IS null;

CREATE OR REPLACE VIEW balances AS
SELECT
    address,
    sum(amount) AS balance,
    count(*) AS utxo_count
FROM utxos
GROUP BY address;
