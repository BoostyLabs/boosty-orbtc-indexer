# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) _(at least trying to)_.

## [0.5.3]

### Added

- Added route to get list of transactions in mempool.

## [0.5.2]

### Added

- Added integration with ord API to mark utxos that have inscriptions.
- Added extractor of utxo markers from ord'd db into CSV.
- Added filtration of utxo by inscription marker.
- Added request to get list of transactions by address.

## [0.5.1]

### Fixed

- Performance optimizations.
- Fix address table schema.

## [0.5.0]

### Changed

**BREAKING** changes of the db schema. This release can't be rolled out on existing database.

- Store hashes as a `BYTEA` instead of `VARCHAR(64)`.
- Don't store full address and pk_scrypt in the each tx output row.
- Address and pk_scypts are deduplicated and stored in the own table.

## [0.4.4]

### Added
- Add new routes to get extended Tx's inputs and outputs details: `/tx/{txid}/ins-outs`, `/tx/{txid}/ins-outs/runes`.

## [0.4.3]

### Added

- `min_fee_rate` config parameter. Now we can adjust min fee rate without code changes.

## [0.4.2]

### Added

- Separate binaries for api and indexer. This can be useful for bare-metal deployment in terms of process observability.
- Script to get binary release for linux using docker.
- Redis cache. Now we can keep utxo locks in redis.
- Cli command to generate new API key.

### Fixed

- Added table with API Keys. Now they are stored in database instead of config file.

## [0.4.1]

### Fixed

- TLRD; Fixed chain reorganization handler. BTC and Rune indexer work as two standalone services with same database. Their work is not synchronized, so when fork occures, btc indexer handles it, drops old and writes new block hashes to the `blocks` table. At that moment runes indexer also finds fork. It tries to find fork root using the `prev_block_hash`. Unfortunately this block was already (re)written to the db by the btc indexer. So runes indexer thinks that everything is ok (parent already in db) and continues. Now this behaviour fixed. Each indexer now keep own track of the indexed blocks.


## [0.4.0]

### Fixed

- Fixed runes indexer, now can get rid of legacy one.

## [0.3.0]

### Changed

- Rewrite how metrics are collected.
- Add check of the service heatlhiness, if indexers are out of sync return 503 status.
- **BREAKING**: delete standalone api for the legacy runes indexer. Now all requests should go through the same api-server.
- Do not format BigDecimal number with scientific notation.

### Fixed

- Return correct rune_id in case of legacy rune indexer.

## [0.2.7]

### Fixed

- Fill gaps in btc utxo response.

## [0.2.6]

### Changed

- Moved basic api types and helpers to the own crate.
- Change page query.
- Moved shared api types to the own crate. Now dex don't need to depend on btc-indexer directly.
- Filter runes utxos from btc utxo response.

### Fixed

- Spawn api jobs.

## [0.2.5]

### Added

- Added prometheus metrics. Extended `/status` response.
- Serve prometheus metrics using isolated HTTP server.
- Added validation of the page params.
- Added few more routes for runes api.

### Fixed

- Fix sentry integration.

## [0.2.4]

### Fixed

- Fix mints validation for the runes indexer.
- Fixed handler of the ctrl_c signal.

## [0.2.3]

### Added

- Add `blockheight` and `txnumber` to the `GET /tx/:tx_id` response.

## [0.2.2]

### Added

- API Handlers for requests `GET /tx/:tx_id` and `POST /tx` to get and submit raw transactions.
- `GET /status` handler for the legacy indexer.
- Helpers to use indexer service in the integration tests.

### Fixed

- Cleanup indexer's state on error to prevent memory leak on postgres fails.

## [0.2.1]

### Added

- API methods to get runes data and utxos.
- Legacy runes indexer. This is 1-to-1 copy-paste of the indexer that we have on production. It's battle-tested in pretty stable. It was added to speed-up migration from in-service indexer to standalone version.
- Added `force_migration` option for the database. Now we can override migration checksums. It's a very annoying issue on the real envs, that we can't event reformat old migration files. By default it is disabled and must be used carefully.

### Changed

- Change API errors.

## [0.2.0]

### Added

- Runes indexer.

### Changed

- Complex refactoring of the indexer runtime was made. `indexer::Rt` was introduced. Now we can easily add extra block indexers.

### Fixed

- Always index chain up to latest block. Indexer don't stay one block behind now.

## [0.1.0]

The very first initial version of BTC-Indexer Service.
