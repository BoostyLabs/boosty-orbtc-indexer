CREATE TABLE IF NOT EXISTS api_keys (
    name          VARCHAR NOT NULL PRIMARY KEY,
    key           VARCHAR NOT NULL,
    blocked       BOOLEAN NOT NULL DEFAULT false,
    can_lock_utxo BOOLEAN NOT NULL DEFAULT false
);
